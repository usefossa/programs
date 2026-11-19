import { createHash } from "node:crypto";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { Connection, PublicKey, type AccountInfo } from "@solana/web3.js";
import { LOADER_PROGRAM_DATA_CODEC } from "./formats";
import { deployed, LOADER, validateElf } from "./upgrade";

type Artifact = { filename: string; bytes: number; sha256: string };
type Recipe = {
  programId: string; genesisHash: string;
  source: { repository: string; commit: string; manifest: string; lockfile: string; lockfileSha256: string };
  build: { solanaVersion: string; platformToolsVersion: string; architecture: string };
  artifact: Artifact;
};

const recipe: Recipe = JSON.parse(readFileSync(new URL("../builds/dex-mainnet.json", import.meta.url), "utf8"));
const sha256 = (bytes: Buffer) => createHash("sha256").update(bytes).digest("hex");

export function checkArtifact(artifact: Buffer, expected: Artifact) {
  validateElf(artifact);
  const hash = sha256(artifact);
  if (artifact.length !== expected.bytes || hash !== expected.sha256) throw Error("Rebuilt artifact differs from the recorded deployment.");
  return hash;
}

export function checkProgramData(artifact: Buffer, account: AccountInfo<Buffer> | null) {
  const offset = LOADER_PROGRAM_DATA_CODEC.fixedSize;
  if (!account || account.executable || !account.owner.equals(LOADER) || account.data.length < offset) throw Error("Invalid deployed ProgramData account.");
  const state = LOADER_PROGRAM_DATA_CODEC.decode(account.data);
  if (state.state !== 3 || state.hasAuthority > 1) throw Error("Invalid deployed ProgramData state.");
  if (!account.data.subarray(offset, offset + artifact.length).equals(artifact)
    || !account.data.subarray(offset + artifact.length).every(byte => byte === 0)) throw Error("Deployed executable differs from the source build.");
  return state.slot;
}

async function run(command: string[], cwd: string) {
  const env: NodeJS.ProcessEnv = { ...process.env, CARGO_TARGET_DIR: join(cwd, "target"), CARGO_INCREMENTAL: "0", GIT_TERMINAL_PROMPT: "0" };
  delete env.RPC_URL;
  const child = Bun.spawn(command, {
    cwd, stdout: "pipe", stderr: "pipe", env,
  });
  const [code, stdout, stderr] = await Promise.all([child.exited, new Response(child.stdout).text(), new Response(child.stderr).text()]);
  if (code !== 0) throw Error(`${command[0]} failed: ${stderr.trim().slice(-4000)}`);
  return stdout + stderr;
}

export async function verifyDex(rpcUrl: string) {
  if (!["https:", "http:"].includes(new URL(rpcUrl).protocol)) throw Error("RPC_URL must be an HTTP endpoint.");
  const rpc = new Connection(rpcUrl, "finalized");
  if (await rpc.getGenesisHash() !== recipe.genesisHash) throw Error("RPC_URL is not Solana mainnet.");
  const scratch = mkdtempSync(join(tmpdir(), "fossa-dex-verify-"));
  try {
    const version = await run(["cargo", "build-sbf", "--version"], scratch);
    if (!version.split("\n").includes(`solana-cargo-build-sbf ${recipe.build.solanaVersion}`)) throw Error(`Install Solana CLI ${recipe.build.solanaVersion} to reproduce this deployment.`);
    console.error(`building deployed source ${recipe.source.commit}`);
    await run(["git", "init", "--quiet"], scratch);
    await run(["git", "fetch", "--quiet", "--depth=1", "--no-tags", recipe.source.repository, recipe.source.commit], scratch);
    await run(["git", "checkout", "--quiet", "--detach", recipe.source.commit], scratch);
    if ((await run(["git", "rev-parse", "HEAD"], scratch)).trim() !== recipe.source.commit) throw Error("Source commit mismatch.");
    if (sha256(readFileSync(join(scratch, recipe.source.lockfile))) !== recipe.source.lockfileSha256) throw Error("Source lockfile mismatch.");
    await run([
      "cargo", "build-sbf", "--manifest-path", recipe.source.manifest,
      "--tools-version", recipe.build.platformToolsVersion, "--arch", recipe.build.architecture,
      "--sbf-out-dir", join(scratch, "artifacts"), "--", "--locked",
    ], scratch);
    const artifact = readFileSync(join(scratch, "artifacts", recipe.artifact.filename));
    const artifactSha256 = checkArtifact(artifact, recipe.artifact);
    const program = new PublicKey(recipe.programId), dataAddress = deployed(await rpc.getAccountInfo(program));
    const deploymentSlot = checkProgramData(artifact, await rpc.getAccountInfo(dataAddress));
    return { program: recipe.programId, sourceCommit: recipe.source.commit, artifactSha256, bytes: artifact.length, deploymentSlot: deploymentSlot.toString(), exactMatch: true };
  } finally {
    rmSync(scratch, { recursive: true, force: true });
  }
}

if (import.meta.main) {
  const rpcUrl = process.env.RPC_URL;
  try {
    if (!rpcUrl || process.argv.length !== 2) throw Error("Set RPC_URL and run bun run verify:dex without additional arguments.");
    console.log(JSON.stringify(await verifyDex(rpcUrl), null, 2));
  } catch (error) {
    const message = error instanceof Error ? error.message : "DEX verification failed.";
    console.error(rpcUrl ? message.replaceAll(rpcUrl, "[configured rpc]") : message);
    process.exitCode = 1;
  }
}
