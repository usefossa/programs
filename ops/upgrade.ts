import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, resolve } from "node:path";
import { Connection, PublicKey, type AccountInfo, type Signer } from "@solana/web3.js";
import { loadConfig, loadKeypair, options, type Config } from "./config";

const LOADER = new PublicKey("BPFLoaderUpgradeab1e11111111111111111111111");

export function validateElf(artifact: Buffer) {
  const invalid = () => { throw Error("Program artifact must be a complete ELF64 little-endian SBF binary."); };
  if (artifact.length < 64 || !artifact.subarray(0, 7).equals(Buffer.from([127, 69, 76, 70, 2, 1, 1]))
    || artifact.readUInt16LE(16) !== 3 || ![247, 263].includes(artifact.readUInt16LE(18))
    || artifact.readUInt32LE(20) !== 1 || artifact.readUInt16LE(52) !== 64) invalid();
  const range = (offset: bigint, size: bigint) => {
    if (offset > BigInt(artifact.length) || size > BigInt(artifact.length) - offset) invalid();
    return Number(offset);
  };
  const programs = artifact.readUInt16LE(56), sections = artifact.readUInt16LE(60), names = artifact.readUInt16LE(62);
  const programOffset = artifact.readBigUInt64LE(32), sectionOffset = artifact.readBigUInt64LE(40);
  if (!programs || programs === 0xffff || programOffset < 64n || artifact.readUInt16LE(54) !== 56) invalid();
  const ph = range(programOffset, BigInt(programs) * 56n);
  if (sections ? sectionOffset < 64n || artifact.readUInt16LE(58) !== 64 || names >= sections : sectionOffset !== 0n || names !== 0) invalid();
  const sh = range(sectionOffset, BigInt(sections) * 64n), entry = artifact.readBigUInt64LE(24);
  let executable = false;
  for (let i = 0; i < programs; i++) {
    const start = ph + i * 56, kind = artifact.readUInt32LE(start), flags = artifact.readUInt32LE(start + 4);
    const offset = artifact.readBigUInt64LE(start + 8), address = artifact.readBigUInt64LE(start + 16);
    const size = artifact.readBigUInt64LE(start + 32), memory = artifact.readBigUInt64LE(start + 40);
    range(offset, size);
    if (kind === 1 && memory < size) invalid();
    if (kind === 1 && (flags & 1) && size > 0n && entry >= address && entry - address < size) executable = true;
  }
  if (!executable) invalid();
  for (let i = 0; i < sections; i++) {
    const start = sh + i * 64, kind = artifact.readUInt32LE(start + 4), link = artifact.readUInt32LE(start + 40);
    const offset = artifact.readBigUInt64LE(start + 24), size = artifact.readBigUInt64LE(start + 32);
    if (kind !== 8) range(offset, size); // nobits sections occupy memory, not file bytes.
    if (link >= sections) invalid();
  }
}

export function deployed(account: AccountInfo<Buffer> | null): PublicKey {
  if (!account?.executable || !account.owner.equals(LOADER) || account.data.length !== 36 || account.data.readUInt32LE() !== 2) throw Error("Expected an upgradeable program account.");
  return new PublicKey(account.data.subarray(4, 36));
}

export function authority(account: AccountInfo<Buffer> | null): PublicKey {
  if (!account || account.executable || !account.owner.equals(LOADER) || account.data.length < 45 || account.data.readUInt32LE() !== 3 || account.data[12] !== 1) throw Error("Program is immutable or ProgramData is invalid.");
  return new PublicKey(account.data.subarray(13, 45));
}

export function extension(artifactBytes: number, dataBytes: number) {
  return artifactBytes + 45 > dataBytes ? Math.max(artifactBytes + 45 - dataBytes, 10_240) : 0;
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
    return account!.data.subarray(45, 45 + artifact.length).equals(artifact)
      && account!.data.subarray(45 + artifact.length).every(byte => byte === 0);
  };
  if (matches(data)) {
    if (!matches(await rpc.getAccountInfo(dataAddress, "finalized"))) throw Error("Matching upgrade awaits finality; reconcile before retrying.");
    return { program: program.toBase58(), alreadyDeployed: true, finalized: true, artifactSha256: hash };
  }
  const extensionBytes = extension(artifact.length, data!.data.length);
  const [rent, bufferRent, balance] = await Promise.all([
    rpc.getMinimumBalanceForRentExemption(data!.data.length + extensionBytes),
    rpc.getMinimumBalanceForRentExemption(artifact.length + 37),
    config.operator?.payer ? Promise.resolve(null) : rpc.getBalance(expected),
  ]);
  const extensionLamports = Math.max(0, rent - data!.lamports), required = extensionLamports + bufferRent + 2_000_000;
  const plan = { program: program.toBase58(), authority: expected.toBase58(), artifactSha256: hash, bytes: artifact.length, extensionBytes, extensionLamports, temporaryBufferLamports: bufferRent, bufferFundingEstimated: true, feeReserveLamports: 2_000_000, balanceLamports: balance, requiredLamports: required, shortfallLamports: balance === null ? null : Math.max(0, required - balance) };
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
    if (retained && (retained.executable || !retained.owner.equals(LOADER) || retained.data.length !== artifact.length + 37
      || retained.data.readUInt32LE() !== 1 || retained.data[4] !== 1 || !retained.data.subarray(5, 37).equals(expected.toBuffer()))) {
      throw Error("Retained upgrade buffer has invalid owner, layout, authority, or size; use a matching buffer or a new buffer keypair.");
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
