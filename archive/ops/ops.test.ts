import { afterEach, expect, test } from "bun:test";
import { existsSync, mkdtempSync, readFileSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { Connection, Keypair, PublicKey, SystemProgram, Transaction, type AccountInfo } from "@solana/web3.js";
import { loadConfig, loadKeypair, options, type Config } from "./config";
import { authority, deployed, deployArgs, extension, upgrade, validateElf } from "./upgrade";

const directories: string[] = [];
afterEach(() => { for (const path of directories.splice(0)) rmSync(path, { recursive: true, force: true }); });
const key = (seed: number) => Keypair.fromSeed(new Uint8Array(32).fill(seed));
const program = key(1).publicKey, market = key(2).publicKey, provider = key(3).publicKey;
const loader = new PublicKey("BPFLoaderUpgradeab1e11111111111111111111111");
function fixture() {
  const base = mkdtempSync(join(tmpdir(), "fossa-upgrade-")); directories.push(base);
  const config: Config = { base, solana: { rpc_url: "http://rpc.invalid" }, marketplace: { program_id: program.toBase58() }, operator: { keypair: "missing-key.json" } };
  return { base, config };
}
function account(magic: string, size: number, owner = program): AccountInfo<Buffer> {
  const data = Buffer.alloc(size); data.write(magic);
  return { data, owner, lamports: 1_000_000, executable: false, rentEpoch: 0 };
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

function elf() {
  const artifact = Buffer.alloc(256); artifact.set([127, 69, 76, 70, 2, 1, 1]);
  artifact.writeUInt16LE(3, 16); artifact.writeUInt16LE(263, 18); artifact.writeUInt32LE(1, 20);
  artifact.writeBigUInt64LE(120n, 24); artifact.writeBigUInt64LE(64n, 32); artifact.writeBigUInt64LE(128n, 40);
  artifact.writeUInt16LE(64, 52); artifact.writeUInt16LE(56, 54); artifact.writeUInt16LE(1, 56);
  artifact.writeUInt16LE(64, 58); artifact.writeUInt16LE(2, 60);
  artifact.writeUInt32LE(1, 64); artifact.writeUInt32LE(5, 68);
  artifact.writeBigUInt64LE(120n, 72); artifact.writeBigUInt64LE(120n, 80);
  artifact.writeBigUInt64LE(8n, 96); artifact.writeBigUInt64LE(8n, 104); artifact.fill(9, 120, 128);
  artifact.writeUInt32LE(1, 196); artifact.writeBigUInt64LE(120n, 216); artifact.writeBigUInt64LE(8n, 224);
  return artifact;
}
function upgradeFixture() {
  const { config, base } = fixture(), artifact = elf(), bytes = artifact.length;
  const path = join(base, "program.so"); writeFileSync(path, artifact);
  const executable = account("", 36, loader), dataAddress = key(10).publicKey;
  executable.executable = true; executable.data.writeUInt32LE(2); dataAddress.toBuffer().copy(executable.data, 4);
  const data = account("", 45 + bytes, loader); data.data.writeUInt32LE(3); data.data[12] = 1; provider.toBuffer().copy(data.data, 13);
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

test("ELF validation checks complete headers, tables, segments, sections, and SBF entry point", () => {
  validateElf(elf());
  for (const bytes of [4, 63, 119, 127, 255]) expect(() => validateElf(elf().subarray(0, bytes))).toThrow("ELF");
  const mutations = [
    (a: Buffer) => { a[4] = 1; },
    (a: Buffer) => a.writeUInt16LE(62, 18),
    (a: Buffer) => a.writeBigUInt64LE(0xffff_ffff_ffff_ffffn, 32),
    (a: Buffer) => a.writeUInt16LE(0xffff, 56),
    (a: Buffer) => a.writeUInt16LE(2, 62),
    (a: Buffer) => a.writeUInt32LE(4, 68),
    (a: Buffer) => a.writeBigUInt64LE(128n, 24),
    (a: Buffer) => a.writeBigUInt64LE(1000n, 96),
    (a: Buffer) => a.writeBigUInt64LE(7n, 104),
    (a: Buffer) => a.writeBigUInt64LE(1000n, 224),
    (a: Buffer) => a.writeUInt32LE(2, 232),
  ];
  for (const mutate of mutations) { const artifact = elf(); mutate(artifact); expect(() => validateElf(artifact)).toThrow("ELF"); }
  const legacy = elf(); legacy.writeUInt16LE(247, 18); validateElf(legacy);
  const nobits = elf(); nobits.writeUInt32LE(8, 196); nobits.writeBigUInt64LE(1000n, 224); validateElf(nobits);
});

test("upgrade inspection validates loader state and rejects immutable or malformed programs", () => {
  const { executable, data } = upgradeFixture();
  expect(deployed(executable)).toBeInstanceOf(PublicKey); expect(authority(data).equals(provider)).toBe(true);
  for (const value of [null, { ...executable, owner: program }, { ...executable, executable: false }, { ...executable, data: Buffer.alloc(35) }]) expect(() => deployed(value)).toThrow();
  data.data[12] = 0; expect(() => authority(data)).toThrow("immutable");
  expect(() => authority({ ...data, data: Buffer.alloc(44) })).toThrow();
  data.data[12] = 1; expect(() => authority({ ...data, executable: true })).toThrow();
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
  const { config, path, rpc, data, artifact } = upgradeFixture(); artifact.copy(data.data, 45);
  data.data = Buffer.concat([data.data, Buffer.alloc(10_240)]);
  expect(await upgrade(config, true, rpc, path)).toMatchObject({ alreadyDeployed: true, finalized: true });
});

test("matching prefix with nonzero deployed suffix is not treated as the inspected artifact", async () => {
  const { config, path, rpc, data, artifact } = upgradeFixture(); artifact.copy(data.data, 45);
  data.data = Buffer.concat([data.data, Buffer.from([1])]);
  expect(await upgrade(config, false, rpc, path)).not.toHaveProperty("alreadyDeployed");
});

test("matching bytes awaiting finality cannot trigger a second upgrade", async () => {
  const { config, path, rpc, data, artifact } = upgradeFixture(); artifact.copy(data.data, 45);
  const get = rpc.getAccountInfo.bind(rpc), stale = { ...data, data: Buffer.from(data.data) }; stale.data[45] = stale.data[45]! ^ 1;
  rpc.getAccountInfo = async (address, commitment) => commitment === "finalized" ? stale : get(address, commitment);
  await expect(upgrade(config, true, rpc, path)).rejects.toThrow("awaits finality");
});

test("upgrade execution uses portable CLI arguments and verifies finalized bytes without real subprocesses", async () => {
  const { config, path, rpc, data, artifact } = executionFixture();
  data.data = data.data.subarray(0, 45);
  const commands: string[][] = [];
  const run = async (args: string[]) => { commands.push(args); if (args[2] === "deploy") data.data = Buffer.concat([data.data.subarray(0, 45), artifact]); };
  expect(await upgrade(config, true, rpc, path, run)).toMatchObject({ finalized: true, extensionBytes: 10_240 });
  expect(commands.map(args => args[2])).toEqual(["extend", "deploy"]);
  expect(commands[1]).toContain(join(config.base, "authority.json"));
  expect(commands[1]).toContain(join(config.base, "buffer.json"));
  expect(commands[1]![3]).not.toBe(path);
  expect(existsSync(commands[1]![3]!)).toBe(false);
});

test("CLI deploy consumes an inspected private snapshot despite original artifact replacement", async () => {
  const { config, path, rpc, data, artifact } = executionFixture();
  data.data = data.data.subarray(0, 45);
  let staged = "";
  const run = async (args: string[]) => {
    if (args[2] === "extend") writeFileSync(path, "replaced after inspection");
    else {
      staged = args[3]!;
      expect(readFileSync(staged)).toEqual(artifact);
      expect(statSync(staged).mode & 0o777).toBe(0o400);
      expect(statSync(dirname(staged)).mode & 0o777).toBe(0o700);
      data.data = Buffer.concat([data.data.subarray(0, 45), readFileSync(staged)]);
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
  const retained = account("", artifact.length + 37, loader);
  retained.data.writeUInt32LE(1); retained.data[4] = 1; provider.toBuffer().copy(retained.data, 5); retained.lamports = lamports;
  accounts.set(buffer.publicKey.toBase58(), retained);
  const missing = Math.max(0, (artifact.length + 37) * 10 - lamports);
  rpc.getBalance = async () => 2_000_000 + missing;
  const result = await upgrade(config, true, rpc, path, async () => { artifact.copy(data.data, 45); });
  expect(result).toMatchObject({ temporaryBufferLamports: missing, bufferFundingEstimated: false, requiredLamports: 2_000_000 + missing, finalized: true, shortfallLamports: 0 });
});

test("retained buffers with wrong owner, executable flag, state, authority, or size fail before CLI", async () => {
  const { config, path, rpc, artifact, buffer, accounts } = executionFixture();
  const mutations = [
    (a: AccountInfo<Buffer>) => { a.owner = SystemProgram.programId; },
    (a: AccountInfo<Buffer>) => { a.executable = true; },
    (a: AccountInfo<Buffer>) => { a.data.writeUInt32LE(3); },
    (a: AccountInfo<Buffer>) => { a.data[4] = 0; },
    (a: AccountInfo<Buffer>) => { market.toBuffer().copy(a.data, 5); },
    (a: AccountInfo<Buffer>) => { a.data = a.data.subarray(0, 36); },
    (a: AccountInfo<Buffer>) => { a.data = Buffer.concat([a.data, Buffer.alloc(1)]); },
  ];
  for (const mutate of mutations) {
    const retained = account("", artifact.length + 37, loader);
    retained.data.writeUInt32LE(1); retained.data[4] = 1; provider.toBuffer().copy(retained.data, 5); mutate(retained);
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
