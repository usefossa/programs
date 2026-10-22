import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { Keypair, PublicKey, type Signer } from "@solana/web3.js";

export type Config = {
  base: string;
  solana: { rpc_url: string };
  marketplace: { program_id: string };
  operator?: { keypair?: string; payer?: string; buffer?: string };
};

export function loadConfig(path = process.env.FOSSA_CONFIG): Config {
  if (!path) throw Error("Pass --config <path> or set FOSSA_CONFIG.");
  const config = Bun.TOML.parse(readFileSync(resolve(path), "utf8")) as unknown as Config;
  if (typeof config.solana?.rpc_url !== "string") throw Error("Configure solana.rpc_url.");
  const url = new URL(config.solana?.rpc_url);
  if (!["http:", "https:"].includes(url.protocol)) throw Error("RPC URL must use HTTP or HTTPS.");
  if (typeof config.marketplace?.program_id !== "string") throw Error("Configure marketplace.program_id.");
  new PublicKey(config.marketplace?.program_id);
  return { ...config, base: dirname(resolve(path)) };
}

export function options(args: string[]) {
  let path: string | undefined, execute = false;
  for (let i = 0; i < args.length; i++) {
    if (args[i] === "--execute") execute = true;
    else if (args[i] === "--config" && args[i + 1] && !args[i + 1]!.startsWith("--")) path = args[++i];
    else throw Error(`Unknown or incomplete argument: ${args[i]}`);
  }
  return { path, execute };
}

export function loadKeypair(base: string, path?: string, expected?: PublicKey): Signer {
  if (!path) throw Error("Configure a Solana JSON keypair file under [operator].");
  const raw = readFileSync(resolve(base, path));
  let values: unknown;
  try {
    try { values = JSON.parse(raw.toString("utf8")); }
    catch { throw Error("Invalid Solana JSON keypair."); }
    if (!Array.isArray(values) || values.length !== 64 || !values.every(v => Number.isInteger(v) && v >= 0 && v <= 255)) throw Error("Expected a 64-byte Solana JSON keypair.");
    const bytes = Uint8Array.from(values);
    try {
      const publicKey = Keypair.fromSecretKey(bytes).publicKey;
      if (expected && !publicKey.equals(expected)) throw Error("Signer does not match the required authority.");
      return { publicKey, secretKey: bytes };
    } catch (error) { bytes.fill(0); throw error; }
  } finally {
    raw.fill(0);
    if (Array.isArray(values)) values.fill(0);
  }
}
