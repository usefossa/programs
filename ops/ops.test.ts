import { afterEach, expect, test } from "bun:test";
import { existsSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { Connection, Keypair, PublicKey, SystemProgram, Transaction, type AccountInfo } from "@solana/web3.js";
import { loadConfig, loadKeypair, options, type Config } from "./config";
import {
  ELF_HEADER_CODEC,
  ELF_PROGRAM_HEADER_CODEC,
  ELF_SECTION_HEADER_CODEC,
  LOADER_BUFFER_CODEC,
  LOADER_PROGRAM_CODEC,
  LOADER_PROGRAM_DATA_CODEC,
  type ElfHeader,
  type ElfProgramHeader,
  type ElfSectionHeader,
} from "./formats";
import { authority, deployed, deployArgs, extension, LOADER, upgrade, validateElf } from "./upgrade";

const directories: string[] = [];
afterEach(() => { for (const path of directories.splice(0)) rmSync(path, { recursive: true, force: true }); });
const key = (seed: number) => Keypair.fromSeed(new Uint8Array(32).fill(seed));
const program = key(1).publicKey, market = key(2).publicKey, provider = key(3).publicKey;
function fixture() {
  const base = mkdtempSync(join(tmpdir(), "fossa-upgrade-")); directories.push(base);
  const config: Config = { base, solana: { rpc_url: "http://rpc.invalid" }, marketplace: { program_id: program.toBase58() }, operator: { keypair: "missing-key.json" } };
  return { base, config };
}
type EncodedBytes = ReturnType<typeof LOADER_PROGRAM_CODEC.encode>;
function account(data: EncodedBytes | Uint8Array | number, owner = program): AccountInfo<Buffer> {
  return { data: typeof data === "number" ? Buffer.alloc(data) : Buffer.from(data), owner, lamports: 1_000_000, executable: false, rentEpoch: 0 };
}
function rpcFixture(accounts: Map<string, AccountInfo<Buffer>>) {
  const calls: string[] = [];
  const rpc = {
    getAccountInfo: async (address: PublicKey) => { calls.push(address.toBase58()); return accounts.get(address.toBase58()) ?? null; },
    getMultipleAccountsInfo: async (addresses: PublicKey[]) => addresses.map(address => accounts.get(address.toBase58()) ?? null),
    getMinimumBalanceForRentExemption: async (bytes: number) => bytes * 10,
    getBalance: async () => 1_000_000_000,
    sendTransaction: async () => { throw Error("Unexpected broadcast in a read-only plan."); },
  } as unknown as Connection;
  return { rpc, calls };
}

test("configuration resolves relative to its file and needs no provider, Redis, or key for upgrade inspection", () => {
  const { base } = fixture(), path = join(base, "operator.toml");
  writeFileSync(path, `[solana]\nrpc_url = "http://rpc.invalid"\n[marketplace]\nprogram_id = "${program}"\n`);
  expect(loadConfig(path).base).toBe(base);
  writeFileSync(path, `[solana]\nrpc_url = "file:///tmp/rpc"\n[marketplace]\nprogram_id = "${program}"\n`);
  expect(() => loadConfig(path)).toThrow("HTTP");
});

test("command options require explicit execution and reject typos or missing values", () => {
  expect(options([])).toEqual({ path: undefined, execute: false });
  expect(options(["--config", "a path/operator.toml", "--execute"])).toEqual({ path: "a path/operator.toml", execute: true });
  for (const args of [["--config"], ["--config", "--execute"], ["--exectue"], ["extra"]]) expect(() => options(args)).toThrow();
});

test("standard Solana keypair files produce usable, clearable signers", () => {
  const { base } = fixture(), source = key(7);
  writeFileSync(join(base, "signer.json"), JSON.stringify([...source.secretKey]));
  const signer = loadKeypair(base, "signer.json", source.publicKey);
  const tx = new Transaction({ feePayer: signer.publicKey, recentBlockhash: key(8).publicKey.toBase58() }).add(SystemProgram.transfer({ fromPubkey: signer.publicKey, toPubkey: market, lamports: 1 }));
  tx.sign(signer); expect(tx.verifySignatures()).toBe(true);
  expect(() => loadKeypair(base, "signer.json", provider)).toThrow("authority");
  signer.secretKey.fill(0); expect(signer.secretKey.every(byte => byte === 0)).toBe(true);
});

test.each([null, [], Array(63).fill(0), Array(64).fill(-1), Array(64).fill(256), Array(64).fill(0.5), Array(64).fill("1"), Array(64).fill(0)].map(value => [value]))("signer validation rejects malformed key data %#", value => {
  const { base } = fixture(); writeFileSync(join(base, "bad.json"), JSON.stringify(value));
  expect(() => loadKeypair(base, "bad.json")).toThrow();
});

const ELF_IDENT: ElfHeader["ident"] = {
  magic: new Uint8Array([127, 69, 76, 70]), class: 2, endianness: 1,
  version: 1, osAbi: 0, abiVersion: 0, padding: new Uint8Array(7),
};
function elf(options: {
  header?: Partial<ElfHeader>;
  program?: Partial<ElfProgramHeader>;
  section?: Partial<ElfSectionHeader>;
} = {}) {
  const header: ElfHeader = {
    ident: ELF_IDENT,
    type: 3, machine: 263, version: 1, entry: 120n,
    programOffset: BigInt(ELF_HEADER_CODEC.fixedSize), sectionOffset: 128n, flags: 0,
    headerSize: ELF_HEADER_CODEC.fixedSize, programEntrySize: ELF_PROGRAM_HEADER_CODEC.fixedSize,
    programCount: 1, sectionEntrySize: ELF_SECTION_HEADER_CODEC.fixedSize, sectionCount: 2,
    sectionNames: 0, ...options.header,
  };
  const programHeader: ElfProgramHeader = {
    type: 1, flags: 5, offset: 120n, virtualAddress: 120n, physicalAddress: 0n,
    fileSize: 8n, memorySize: 8n, alignment: 0n, ...options.program,
  };
  const section: ElfSectionHeader = {
    name: 0, type: 1, flags: 0n, address: 0n, offset: 120n, size: 8n,
    link: 0, info: 0, alignment: 0n, entrySize: 0n, ...options.section,
  };
  const artifact = Buffer.alloc(256);
  ELF_HEADER_CODEC.write(header, artifact, 0);
  ELF_PROGRAM_HEADER_CODEC.write(programHeader, artifact, ELF_HEADER_CODEC.fixedSize);
  ELF_SECTION_HEADER_CODEC.write(section, artifact, 128 + ELF_SECTION_HEADER_CODEC.fixedSize);
  artifact.fill(9, 120, 128);
  return artifact;
}
function programData(artifact = Buffer.alloc(0), authority = provider, padding = 0, hasAuthority = 1) {
  return Buffer.concat([
    Buffer.from(LOADER_PROGRAM_DATA_CODEC.encode({ state: 3, slot: 0n, hasAuthority, authority: authority.toBytes() })),
    artifact,
    Buffer.alloc(padding),
  ]);
}
function bufferData(bytes: number, authority = provider, state = 1, hasAuthority = 1) {
  return Buffer.concat([
    Buffer.from(LOADER_BUFFER_CODEC.encode({ state, hasAuthority, authority: authority.toBytes() })),
    Buffer.alloc(bytes),
  ]);
}
function upgradeFixture() {
  const { config, base } = fixture(), artifact = elf(), bytes = artifact.length;
  const path = join(base, "program.so"); writeFileSync(path, artifact);
  const dataAddress = key(10).publicKey;
  const executable = account(LOADER_PROGRAM_CODEC.encode({ state: 2, programData: dataAddress.toBytes() }), LOADER);
  executable.executable = true;
  const data = account(programData(Buffer.alloc(bytes)), LOADER);
  const accounts = new Map([[program.toBase58(), executable], [dataAddress.toBase58(), data]]), { rpc } = rpcFixture(accounts);
  return { config, path, rpc, executable, data, artifact, accounts };
}
function executionFixture() {
  const value = upgradeFixture(), buffer = key(11);
  value.config.operator = { keypair: "authority.json", buffer: "buffer.json" };
  writeFileSync(join(value.config.base, "authority.json"), JSON.stringify([...key(3).secretKey]));
  writeFileSync(join(value.config.base, "buffer.json"), JSON.stringify([...buffer.secretKey]));
  return { ...value, buffer };
}

test.each([false, true])("upgrade fee reserve scales with artifact upload and separate payer %s", async separatePayer => {
  const { config, path, rpc, artifact } = upgradeFixture();
  const bytes = 1_122_856;
  writeFileSync(path, Buffer.concat([artifact, Buffer.alloc(bytes - artifact.length)]));
  if (separatePayer) config.operator = { payer: "unread-plan-only-key.json" };
  const result = await upgrade(config, false, rpc, path);
  const expected = 2_000_000 + 2_194 * (separatePayer ? 2 : 1) * 5_000;
  expect(result).toMatchObject({ bytes, feeReserveLamports: expected });
});

test("ELF validation checks complete headers, tables, segments, sections, and SBF entry point", () => {
  validateElf(elf());
  for (const bytes of [4, 63, 119, 127, 255]) expect(() => validateElf(elf().subarray(0, bytes))).toThrow("ELF");
  const invalid = [
    elf({ header: { ident: { ...ELF_IDENT, class: 1 } } }),
    elf({ header: { machine: 62 } }),
    elf({ header: { programOffset: 0xffff_ffff_ffff_ffffn } }),
    elf({ header: { programCount: 0xffff } }),
    elf({ header: { sectionNames: 2 } }),
    elf({ program: { flags: 4 } }),
    elf({ header: { entry: 128n } }),
    elf({ program: { fileSize: 1000n } }),
    elf({ program: { memorySize: 7n } }),
    elf({ section: { size: 1000n } }),
    elf({ section: { link: 2 } }),
  ];
  for (const artifact of invalid) expect(() => validateElf(artifact)).toThrow("ELF");
  validateElf(elf({ section: { type: 8, size: 1000n } }));
});

test("upgrade inspection validates loader state and rejects immutable or malformed programs", () => {
  const { executable, data } = upgradeFixture();
  expect(deployed(executable)).toBeInstanceOf(PublicKey); expect(authority(data).equals(provider)).toBe(true);
  for (const value of [null, { ...executable, owner: program }, { ...executable, executable: false }, { ...executable, data: Buffer.alloc(LOADER_PROGRAM_CODEC.fixedSize - 1) }]) expect(() => deployed(value)).toThrow();
  expect(() => authority({ ...data, data: programData(Buffer.alloc(0), provider, 0, 0) })).toThrow("immutable");
  expect(() => authority({ ...data, data: Buffer.alloc(LOADER_PROGRAM_DATA_CODEC.fixedSize - 1) })).toThrow();
  expect(() => authority({ ...data, executable: true })).toThrow();
});

test.each([[100, 145, 0], [100, 144, 10_240], [100, 200, 0], [20_000, 100, 19_945]])("upgrade growth rounds minimum extension correctly %#", (artifact, size, expected) => expect(extension(artifact, size)).toBe(expected));

test("upgrade CLI preserves paths containing spaces without shell or Linux-specific helpers", () => {
  const args = deployArgs(program, "a path/program.so", "keys/authority.json", "keys/buffer.json", "http://rpc.invalid");
  expect(args.slice(0, 4)).toEqual(["solana", "program", "deploy", "a path/program.so"]);
  expect(args[args.indexOf("--upgrade-authority") + 1]).toBe("keys/authority.json");
  expect(args[args.indexOf("--buffer") + 1]).toBe("keys/buffer.json");
  expect(args).not.toContain("python3");
});

test("upgrade read-only plan neither loads keys nor needs Redis", async () => {
  const { config, path, rpc } = upgradeFixture();
  const plan = await upgrade(config, false, rpc, path);
  expect(plan).toMatchObject({ authority: provider.toBase58(), extensionBytes: 0, bufferFundingEstimated: true, shortfallLamports: 0 });
  config.operator!.payer = "missing-payer.json";
  expect(await upgrade(config, false, rpc, path)).toMatchObject({ balanceLamports: null, shortfallLamports: null });
});

test("matching finalized program avoids duplicate upgrade and signer access", async () => {
  const { config, path, rpc, data, artifact } = upgradeFixture(); data.data = programData(artifact, provider, 10_240);
  expect(await upgrade(config, true, rpc, path)).toMatchObject({ alreadyDeployed: true, finalized: true });
});

test("matching prefix with nonzero deployed suffix is not treated as the inspected artifact", async () => {
  const { config, path, rpc, data, artifact } = upgradeFixture(); data.data = programData(artifact, provider, 1); data.data[data.data.length - 1] = 1;
  expect(await upgrade(config, false, rpc, path)).not.toHaveProperty("alreadyDeployed");
});

test("matching bytes awaiting finality cannot trigger a second upgrade", async () => {
  const { config, path, rpc, data, artifact } = upgradeFixture(); data.data = programData(artifact);
  const staleArtifact = Buffer.from(artifact); staleArtifact[0] = staleArtifact[0]! ^ 1;
  const get = rpc.getAccountInfo.bind(rpc), stale = { ...data, data: programData(staleArtifact) };
  rpc.getAccountInfo = async (address, commitment) => commitment === "finalized" ? stale : get(address, commitment);
  await expect(upgrade(config, true, rpc, path)).rejects.toThrow("awaits finality");
});

test("upgrade execution uses portable CLI arguments and verifies finalized bytes without real subprocesses", async () => {
  const { config, path, rpc, data, artifact } = executionFixture();
  data.data = programData();
  const commands: string[][] = [];
  const run = async (args: string[]) => { commands.push(args); if (args[2] === "deploy") data.data = programData(artifact); };
  expect(await upgrade(config, true, rpc, path, run)).toMatchObject({ finalized: true, extensionBytes: 10_240 });
  expect(commands.map(args => args[2])).toEqual(["extend", "deploy"]);
  expect(commands[1]).toContain(join(config.base, "authority.json"));
  expect(commands[1]).toContain(join(config.base, "buffer.json"));
  expect(commands[1]![3]).not.toBe(path);
  expect(existsSync(commands[1]![3]!)).toBe(false);
});

test("CLI deploy consumes an inspected private snapshot despite original artifact replacement", async () => {
  const { config, path, rpc, data, artifact } = executionFixture();
  data.data = programData();
  let staged = "";
  const run = async (args: string[]) => {
    if (args[2] === "extend") writeFileSync(path, "replaced after inspection");
    else {
      staged = args[3]!;
      expect(readFileSync(staged)).toEqual(artifact);
      expect(statSync(staged).mode & 0o777).toBe(0o400);
      expect(statSync(dirname(staged)).mode & 0o777).toBe(0o700);
      data.data = programData(readFileSync(staged));
    }
  };
  expect(await upgrade(config, true, rpc, path, run)).toHaveProperty("finalized", true);
  expect(existsSync(dirname(staged))).toBe(false);
});

test("failed CLI deployment removes the private artifact snapshot and retains recovery keypair", async () => {
  const { config, path, rpc } = executionFixture();
  let staged = "";
  await expect(upgrade(config, true, rpc, path, async args => { staged = args[3]!; throw Error("synthetic CLI failure"); })).rejects.toThrow("synthetic CLI failure");
  expect(existsSync(dirname(staged))).toBe(false);
  expect(existsSync(join(config.base, "buffer.json"))).toBe(true);
});

test.each([0, 1234, 5000])("retained valid buffer retry funds only missing rent, existing lamports %i", async lamports => {
  const { config, path, rpc, data, artifact, buffer, accounts } = executionFixture();
  const retained = account(bufferData(artifact.length), LOADER); retained.lamports = lamports;
  accounts.set(buffer.publicKey.toBase58(), retained);
  const missing = Math.max(0, (artifact.length + LOADER_BUFFER_CODEC.fixedSize) * 10 - lamports);
  const feeReserve = 2_000_000 + Math.ceil(artifact.length / 512) * 5_000;
  rpc.getBalance = async () => feeReserve + missing;
  const result = await upgrade(config, true, rpc, path, async () => { data.data = programData(artifact); });
  expect(result).toMatchObject({ temporaryBufferLamports: missing, bufferFundingEstimated: false, feeReserveLamports: feeReserve, requiredLamports: feeReserve + missing, finalized: true, shortfallLamports: 0 });
});

test("retained buffers with wrong owner, executable flag, state, authority, or size fail before CLI", async () => {
  const { config, path, rpc, artifact, buffer, accounts } = executionFixture();
  const invalid = [
    { ...account(bufferData(artifact.length), LOADER), owner: SystemProgram.programId },
    { ...account(bufferData(artifact.length), LOADER), executable: true },
    account(bufferData(artifact.length, provider, 3), LOADER),
    account(bufferData(artifact.length, provider, 1, 0), LOADER),
    account(bufferData(artifact.length, market), LOADER),
    account(bufferData(artifact.length - 1), LOADER),
    account(bufferData(artifact.length + 1), LOADER),
  ];
  for (const retained of invalid) {
    accounts.set(buffer.publicKey.toBase58(), retained);
    let called = false;
    await expect(upgrade(config, true, rpc, path, async () => { called = true; })).rejects.toThrow("Retained upgrade buffer");
    expect(called).toBe(false);
  }
});

test("upgrade refuses missing signer and non-ELF artifact before execution", async () => {
  const { config, path, rpc } = upgradeFixture();
  await expect(upgrade(config, true, rpc, path)).rejects.toThrow();
  writeFileSync(path, "invalid"); await expect(upgrade(config, false, rpc, path)).rejects.toThrow("ELF");
});
