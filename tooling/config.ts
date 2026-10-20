import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { Keypair, PublicKey } from "@solana/web3.js";
import nacl from "tweetnacl";

export type HostedModel = {
  bond_target_atomic?: number; id: string; name: string; context_tokens: number; max_input_bound: number; output_tokens: number;
  input: string[]; effort: string[];
  nonthinking?: { temperature: number; top_p: number; presence_penalty: number };
  temperature: number; top_p: number; input_rate: number; cached_rate: number; output_rate: number;
};
export type Config = {
  redis: { url: string; namespace: string }; security: { encryption_key: string };
  server: { api_url: string }; solana: { rpc_url: string; usdc_mint: string };
  marketplace: { program_id: string; market: string };
  provider: { name: string; description?: string; public_url: string; wallet: string;
    speed_tps: number; capacity: number; model_book_limit: number; bond_target_atomic: number; models: HostedModel[] };
};
const configPath = process.env.FOSSA_CONFIG;
if (!configPath) throw Error("Set FOSSA_CONFIG to an explicit operator configuration path.");
export const config = Bun.TOML.parse(readFileSync(resolve(configPath), "utf8")) as Config;
export const modelAddress = (id: string) => PublicKey.findProgramAddressSync([
  Buffer.from("model"), new PublicKey(config.marketplace.market).toBuffer(), createHash("sha256").update(id).digest(),
], new PublicKey(config.marketplace.program_id))[0];
export function modelDocument(model: HostedModel) {
  return {
    id: model.id, name: model.name, modalities: { input: model.input, output: ["text"] },
    effort: model.effort, variants: model.effort.map(id => ({ id, body: { reasoning_effort: id === "thinking" ? "high" : id, ...(id === "none" ? model.nonthinking : {}) } })),
    capabilities: { tools: true, reasoning: true }, contextTokens: model.context_tokens, inputBoundTokens: model.max_input_bound,
    outputTokens: model.output_tokens,
    settings: { maxTokens: model.output_tokens, temperature: model.temperature, topP: model.top_p },
  };
}
export async function providerKey(redis: { get(key: string): Promise<string | null> }) {
  const { program_id, market } = config.marketplace;
  const stored = await redis.get(`${config.redis.namespace}:market:${program_id}:${market}:provider:${config.provider.wallet}:key`);
  if (!stored) throw Error("Encrypted provider key unavailable");
  const value = JSON.parse(stored), encryption = Buffer.from(config.security.encryption_key, "hex");
  try {
    const bytes = nacl.secretbox.open(Buffer.from(value.ciphertext, "hex"), Buffer.from(value.nonce, "hex"), encryption);
    if (!bytes) throw Error("Provider key decryption failed");
    try {
      const key = Keypair.fromSecretKey(Uint8Array.from(bytes));
      if (key.publicKey.toBase58() !== config.provider.wallet) throw Error("Provider key identity mismatch");
      return key;
    } finally { bytes.fill(0); }
  } finally { encryption.fill(0); }
}
