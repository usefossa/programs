import { afterEach, expect, test } from "bun:test";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Connection, Keypair, PublicKey, SystemProgram, Transaction, type AccountInfo } from "@solana/web3.js";
import { loadConfig, loadKeypair, options, type Config } from "./config";
import { authority, deployed, deployArgs, extension, upgrade } from "./upgrade";

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

function upgradeFixture(bytes = 64) {
  const { config, base } = fixture(), artifact = Buffer.alloc(bytes, 9); artifact.set([127, 69, 76, 70]);
  const path = join(base, "program.so"); writeFileSync(path, artifact);
  const executable = account("", 36, loader), dataAddress = key(10).publicKey;
  executable.executable = true; executable.data.writeUInt32LE(2); dataAddress.toBuffer().copy(executable.data, 4);
  const data = account("", 45 + bytes, loader); data.data.writeUInt32LE(3); data.data[12] = 1; provider.toBuffer().copy(data.data, 13);
  const { rpc } = rpcFixture(new Map([[program.toBase58(), executable], [dataAddress.toBase58(), data]]));
  return { config, path, rpc, executable, data, artifact };
}

test("upgrade inspection validates loader state and rejects immutable or malformed programs", () => {
  const { executable, data } = upgradeFixture();
  expect(deployed(executable)).toBeInstanceOf(PublicKey); expect(authority(data).equals(provider)).toBe(true);
  for (const value of [null, { ...executable, owner: program }, { ...executable, executable: false }, { ...executable, data: Buffer.alloc(35) }]) expect(() => deployed(value)).toThrow();
  data.data[12] = 0; expect(() => authority(data)).toThrow("immutable");
  expect(() => authority({ ...data, data: Buffer.alloc(44) })).toThrow();
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
  expect(plan).toMatchObject({ authority: provider.toBase58(), extensionBytes: 0, shortfallLamports: 0 });
  config.operator!.payer = "missing-payer.json";
  expect(await upgrade(config, false, rpc, path)).toMatchObject({ balanceLamports: null, shortfallLamports: null });
});

test("matching finalized program avoids duplicate upgrade and signer access", async () => {
  const { config, path, rpc, data, artifact } = upgradeFixture(); artifact.copy(data.data, 45);
  expect(await upgrade(config, true, rpc, path)).toMatchObject({ alreadyDeployed: true, finalized: true });
});

test("matching bytes awaiting finality cannot trigger a second upgrade", async () => {
  const { config, path, rpc, data, artifact } = upgradeFixture(); artifact.copy(data.data, 45);
  const get = rpc.getAccountInfo.bind(rpc), stale = { ...data, data: Buffer.from(data.data) }; stale.data[45] = stale.data[45]! ^ 1;
  rpc.getAccountInfo = async (address, commitment) => commitment === "finalized" ? stale : get(address, commitment);
  await expect(upgrade(config, true, rpc, path)).rejects.toThrow("awaits finality");
});

test("upgrade execution uses portable CLI arguments and verifies finalized bytes without real subprocesses", async () => {
  const { config, path, rpc, data, artifact } = upgradeFixture();
  config.operator = { keypair: "authority.json", buffer: "buffer.json" };
  writeFileSync(join(config.base, "authority.json"), JSON.stringify([...key(3).secretKey]));
  writeFileSync(join(config.base, "buffer.json"), JSON.stringify([...key(11).secretKey]));
  data.data = data.data.subarray(0, 45);
  const commands: string[][] = [];
  const run = async (args: string[]) => { commands.push(args); if (args[2] === "deploy") data.data = Buffer.concat([data.data.subarray(0, 45), artifact]); };
  expect(await upgrade(config, true, rpc, path, run)).toMatchObject({ finalized: true, extensionBytes: 10_240 });
  expect(commands.map(args => args[2])).toEqual(["extend", "deploy"]);
  expect(commands[1]).toContain(join(config.base, "authority.json"));
  expect(commands[1]).toContain(join(config.base, "buffer.json"));
});

test("upgrade refuses missing signer and non-ELF artifact before execution", async () => {
  const { config, path, rpc } = upgradeFixture();
  await expect(upgrade(config, true, rpc, path)).rejects.toThrow();
  writeFileSync(path, "invalid"); await expect(upgrade(config, false, rpc, path)).rejects.toThrow("ELF");
});
