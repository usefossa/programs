import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { LOADER_V3_PROGRAM_ADDRESS } from "@solana-program/loader-v3";
import { Connection, PublicKey, type AccountInfo, type Signer } from "@solana/web3.js";
import { loadConfig, loadKeypair, options, type Config } from "./config";
import {
  ELF_HEADER_CODEC,
  ELF_PROGRAM_HEADER_CODEC,
  ELF_SECTION_HEADER_CODEC,
  LOADER_BUFFER_CODEC,
  LOADER_PROGRAM_CODEC,
  LOADER_PROGRAM_DATA_CODEC,
} from "./formats";

export const LOADER = new PublicKey(LOADER_V3_PROGRAM_ADDRESS);

export function validateElf(artifact: Buffer) {
  const invalid = () => { throw Error("Program artifact must be a complete ELF64 little-endian SBF binary."); };
  try {
    const header = ELF_HEADER_CODEC.decode(artifact);
    if (!Buffer.from(header.ident.magic).equals(Buffer.from([127, 69, 76, 70]))
      || header.ident.class !== 2 || header.ident.endianness !== 1 || header.ident.version !== 1
      || header.type !== 3 || header.machine !== 263 || header.version !== 1
      || header.headerSize !== ELF_HEADER_CODEC.fixedSize) invalid();
    const range = (offset: bigint, size: bigint) => {
      if (offset > BigInt(artifact.length) || size > BigInt(artifact.length) - offset) invalid();
      return Number(offset);
    };
    const programs = header.programCount, sections = header.sectionCount, names = header.sectionNames;
    if (!programs || programs === 0xffff || header.programOffset < BigInt(ELF_HEADER_CODEC.fixedSize)
      || header.programEntrySize !== ELF_PROGRAM_HEADER_CODEC.fixedSize) invalid();
    const programOffset = range(header.programOffset, BigInt(programs) * BigInt(ELF_PROGRAM_HEADER_CODEC.fixedSize));
    if (sections
      ? header.sectionOffset < BigInt(ELF_HEADER_CODEC.fixedSize)
        || header.sectionEntrySize !== ELF_SECTION_HEADER_CODEC.fixedSize || names >= sections
      : header.sectionOffset !== 0n || names !== 0) invalid();
    const sectionOffset = range(header.sectionOffset, BigInt(sections) * BigInt(ELF_SECTION_HEADER_CODEC.fixedSize));
    let executable = false;
    for (let i = 0; i < programs; i++) {
      const program = ELF_PROGRAM_HEADER_CODEC.decode(artifact, programOffset + i * ELF_PROGRAM_HEADER_CODEC.fixedSize);
      range(program.offset, program.fileSize);
      if (program.type === 1 && program.memorySize < program.fileSize) invalid();
      if (program.type === 1 && (program.flags & 1) && program.fileSize > 0n
        && header.entry >= program.virtualAddress && header.entry - program.virtualAddress < program.fileSize) executable = true;
    }
    if (!executable) invalid();
    for (let i = 0; i < sections; i++) {
      const section = ELF_SECTION_HEADER_CODEC.decode(artifact, sectionOffset + i * ELF_SECTION_HEADER_CODEC.fixedSize);
      if (section.type !== 8) range(section.offset, section.size);
      if (section.link >= sections) invalid();
    }
  } catch {
    invalid();
  }
}

export function deployed(account: AccountInfo<Buffer> | null): PublicKey {
  if (!account?.executable || !account.owner.equals(LOADER) || account.data.length !== LOADER_PROGRAM_CODEC.fixedSize) throw Error("Expected an upgradeable program account.");
  const state = LOADER_PROGRAM_CODEC.decode(account.data);
  if (state.state !== 2) throw Error("Expected an upgradeable program account.");
  return new PublicKey(state.programData);
}

export function authority(account: AccountInfo<Buffer> | null): PublicKey {
  if (!account || account.executable || !account.owner.equals(LOADER) || account.data.length < LOADER_PROGRAM_DATA_CODEC.fixedSize) throw Error("Program is immutable or ProgramData is invalid.");
  const state = LOADER_PROGRAM_DATA_CODEC.decode(account.data);
  if (state.state !== 3 || state.hasAuthority !== 1) throw Error("Program is immutable or ProgramData is invalid.");
  return new PublicKey(state.authority);
}

export function extension(artifactBytes: number, dataBytes: number) {
  return artifactBytes + LOADER_PROGRAM_DATA_CODEC.fixedSize > dataBytes
    ? Math.max(artifactBytes + LOADER_PROGRAM_DATA_CODEC.fixedSize - dataBytes, 10_240) : 0;
}

export function deployArgs(program: PublicKey, artifact: string, signer: string, buffer: string, rpc: string, payer = signer) {
  return ["solana", "program", "deploy", artifact, "--program-id", program.toBase58(), "--upgrade-authority", signer, "--buffer", buffer, "--keypair", payer, "--url", rpc, "--use-rpc", "--commitment", "confirmed", "--output", "json"];
}

async function runCli(args: string[], rpcUrl: string) {
  const child = Bun.spawn(args, { stdout: "pipe", stderr: "pipe" });
  const [code, stdout, stderr] = await Promise.all([child.exited, new Response(child.stdout).text(), new Response(child.stderr).text()]);
  const output = (stdout + stderr).replaceAll(rpcUrl, "[configured rpc]");
  if (code !== 0) throw Error(`Solana CLI failed; retain buffer keypair and reconcile before retrying. ${output}`);
  console.log(output);
}

export async function upgrade(
  config: Config, execute = false,
  rpc = new Connection(config.solana.rpc_url, "confirmed"),
  path = process.env.FOSSA_SBF ? resolve(process.env.FOSSA_SBF) : resolve(import.meta.dir, "../target/deploy/inference_router_program.so"),
  run = runCli,
) {
  const artifact = readFileSync(path), hash = createHash("sha256").update(artifact).digest("hex");
  validateElf(artifact);
  const program = new PublicKey(config.marketplace.program_id), dataAddress = deployed(await rpc.getAccountInfo(program));
  const data = await rpc.getAccountInfo(dataAddress), expected = authority(data);
  const matches = (account: AccountInfo<Buffer> | null) => {
    authority(account);
    const offset = LOADER_PROGRAM_DATA_CODEC.fixedSize;
    return account!.data.subarray(offset, offset + artifact.length).equals(artifact)
      && account!.data.subarray(offset + artifact.length).every(byte => byte === 0);
  };
  if (matches(data)) {
    if (!matches(await rpc.getAccountInfo(dataAddress, "finalized"))) throw Error("Matching upgrade awaits finality; reconcile before retrying.");
    return { program: program.toBase58(), alreadyDeployed: true, finalized: true, artifactSha256: hash };
  }
  const extensionBytes = extension(artifact.length, data!.data.length);
  const [rent, bufferRent, balance] = await Promise.all([
    rpc.getMinimumBalanceForRentExemption(data!.data.length + extensionBytes),
    rpc.getMinimumBalanceForRentExemption(artifact.length + LOADER_BUFFER_CODEC.fixedSize),
    config.operator?.payer ? Promise.resolve(null) : rpc.getBalance(expected),
  ]);
  const feeReserveLamports = 2_000_000 + Math.ceil(artifact.length / 512) * (config.operator?.payer ? 2 : 1) * 5_000;
  const extensionLamports = Math.max(0, rent - data!.lamports), required = extensionLamports + bufferRent + feeReserveLamports;
  const plan = { program: program.toBase58(), authority: expected.toBase58(), artifactSha256: hash, bytes: artifact.length, extensionBytes, extensionLamports, temporaryBufferLamports: bufferRent, bufferFundingEstimated: true, feeReserveLamports, balanceLamports: balance, requiredLamports: required, shortfallLamports: balance === null ? null : Math.max(0, required - balance) };
  if (!execute) return plan;
  const keys: Signer[] = [];
  let snapshot: string | undefined;
  try {
    keys.push(loadKeypair(config.base, config.operator?.keypair, expected));
    const buffer = loadKeypair(config.base, config.operator?.buffer); keys.push(buffer);
    if (config.operator?.payer) keys.push(loadKeypair(config.base, config.operator.payer));
    const signerPath = resolve(config.base, config.operator!.keypair!), bufferPath = resolve(config.base, config.operator!.buffer!);
    const payerPath = config.operator?.payer ? resolve(config.base, config.operator.payer) : signerPath;
    const payer = config.operator?.payer ? keys[2]! : keys[0]!;
    if ([keys[0]!, payer].some(key => key.publicKey.equals(buffer.publicKey))) throw Error("Buffer must differ from authority and fee payer.");
    const retained = await rpc.getAccountInfo(buffer.publicKey);
    if (retained) {
      const state = retained.data.length >= LOADER_BUFFER_CODEC.fixedSize
        ? LOADER_BUFFER_CODEC.decode(retained.data) : null;
      if (retained.executable || !retained.owner.equals(LOADER)
        || retained.data.length !== artifact.length + LOADER_BUFFER_CODEC.fixedSize
        || state?.state !== 1 || state.hasAuthority !== 1
        || !Buffer.from(state.authority).equals(expected.toBuffer())) {
        throw Error("Retained upgrade buffer has invalid owner, layout, authority, or size; use a matching buffer or a new buffer keypair.");
      }
    }
    plan.temporaryBufferLamports = Math.max(0, bufferRent - (retained?.lamports ?? 0));
    plan.bufferFundingEstimated = false;
    plan.requiredLamports = extensionLamports + plan.temporaryBufferLamports + plan.feeReserveLamports;
    const payerBalance = config.operator?.payer ? await rpc.getBalance(payer.publicKey) : balance!;
    if (payerBalance < plan.requiredLamports) throw Error("Upgrade funding insufficient; no transaction sent.");
    snapshot = mkdtempSync(join(tmpdir(), "fossa-upgrade-"));
    const artifactPath = join(snapshot, "program.so");
    writeFileSync(artifactPath, artifact, { flag: "wx", mode: 0o400 });
    if (extensionBytes) await run(["solana", "program", "extend", program.toBase58(), String(extensionBytes), "--keypair", payerPath, "--url", config.solana.rpc_url, "--commitment", "finalized", "--output", "json"], config.solana.rpc_url);
    await run(deployArgs(program, artifactPath, signerPath, bufferPath, config.solana.rpc_url, payerPath), config.solana.rpc_url);
    for (let attempt = 0; attempt < 12; attempt++) {
      if (matches(await rpc.getAccountInfo(dataAddress, "finalized"))) return { ...plan, balanceLamports: payerBalance, shortfallLamports: 0, finalized: true, buffer: buffer.publicKey.toBase58() };
      if (attempt < 11) await Bun.sleep(5_000);
    }
    throw Error("Program bytes await finalization; reconcile before another upgrade.");
  } finally {
    for (const key of keys) key.secretKey.fill(0);
    if (snapshot) rmSync(snapshot, { recursive: true, force: true });
  }
}

if (import.meta.main) {
  const args = options(process.argv.slice(2));
  console.log(JSON.stringify(await upgrade(loadConfig(args.path), args.execute), null, 2));
}
