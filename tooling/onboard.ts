import { createHash } from "node:crypto";
import { createAssociatedTokenAccountIdempotentInstruction, getAssociatedTokenAddressSync, TOKEN_PROGRAM_ID } from "./token";
import { Connection, Keypair, PublicKey, SystemProgram, Transaction, TransactionInstruction, sendAndConfirmTransaction } from "@solana/web3.js";
import { createClient } from "redis";
import { config, modelDocument, providerKey } from "./config";

const redis = createClient({ url: config.redis.url });
redis.on("error", () => {});
await redis.connect();
const payer = await providerKey(redis);

const rpc = new Connection(config.solana.rpc_url, "confirmed");
const program = new PublicKey(config.marketplace.program_id);
const market = new PublicKey(config.marketplace.market);
const mint = new PublicKey(config.solana.usdc_mint);
const provider = payer.publicKey;
const [profile] = PublicKey.findProgramAddressSync([Buffer.from("provider"), market.toBuffer(), provider.toBuffer()], program);
async function document(value: unknown) {
  const body = JSON.stringify(value), hash = createHash("sha256").update(body).digest();
  const uri = Buffer.from(`${config.server.api_url.replace(/\/$/, "")}/v1/market/metadata/${hash.toString("hex")}`);
  if (uri.length > 256) throw Error("Metadata URI exceeds program limit");
  await redis.set(`${config.redis.namespace}:metadata:${hash.toString("hex")}`, body, { NX: true });
  return Buffer.concat([hash, uri]);
}

function instruction(op: number, data: Buffer, keys: { pubkey: PublicKey; isSigner: boolean; isWritable: boolean }[]) {
  return new TransactionInstruction({ programId: program, data: Buffer.concat([Buffer.from([op]), data]), keys });
}

async function send(label: string, tx: Transaction, signers: Keypair[] = []) {
  const signature = await sendAndConfirmTransaction(rpc, tx, [payer, ...signers], { commitment: "confirmed" });
  console.log(label, signature);
}

const profileDoc = await document({ name: config.provider.name, endpoint: config.provider.public_url, description: config.provider.description ?? "Inference provider" });

try {
  let account = await rpc.getAccountInfo(profile);
  if (!account) {
    await send("provider registered", new Transaction().add(instruction(19, profileDoc, [
      { pubkey: market, isSigner: false, isWritable: false }, { pubkey: profile, isSigner: false, isWritable: true },
      { pubkey: provider, isSigner: true, isWritable: false }, { pubkey: provider, isSigner: true, isWritable: true },
      { pubkey: SystemProgram.programId, isSigner: false, isWritable: false },
    ])));
    account = await rpc.getAccountInfo(profile);
  }
  if (!account || account.owner.toBase58() !== program.toBase58() || account.data.subarray(0, 8).toString() !== "PROV0V20") throw Error("Provider profile invalid");
  if (account.data[106] !== 1) await send("provider approved", new Transaction().add(instruction(20, Buffer.from([1]), [
    { pubkey: market, isSigner: false, isWritable: false }, { pubkey: profile, isSigner: false, isWritable: true },
    { pubkey: provider, isSigner: true, isWritable: false },
  ])));
  if (!account.data.subarray(72, 104).equals(profileDoc.subarray(0, 32))) await send("provider metadata updated", new Transaction().add(instruction(21, profileDoc, [
    { pubkey: market, isSigner: false, isWritable: false }, { pubkey: profile, isSigner: false, isWritable: true },
    { pubkey: provider, isSigner: true, isWritable: false },
  ])));

  for (const hosted of config.provider.models) {
    const offer = { ...config.provider, ...hosted };
    const modelId = createHash("sha256").update(hosted.id).digest();
    const [model] = PublicKey.findProgramAddressSync([Buffer.from("model"), market.toBuffer(), modelId], program);
    const [metadata] = PublicKey.findProgramAddressSync([Buffer.from("model_metadata"), model.toBuffer()], program);
    const modelDoc = await document(modelDocument(hosted));
    const terms = Keypair.generate(), capacity = Keypair.generate();
    const limit = config.provider.model_book_limit;
    if (!Number.isInteger(limit) || limit < 1 || limit > 4096) throw Error("Invalid model book limit");
    let modelAccount = await rpc.getAccountInfo(model);
    if (!modelAccount) {
      const termsBytes = 48 + limit * 120, capacityBytes = 48 + limit * 32;
      await send("model initialized", new Transaction().add(
        SystemProgram.createAccount({ fromPubkey: provider, newAccountPubkey: terms.publicKey, lamports: await rpc.getMinimumBalanceForRentExemption(termsBytes), space: termsBytes, programId: program }),
        SystemProgram.createAccount({ fromPubkey: provider, newAccountPubkey: capacity.publicKey, lamports: await rpc.getMinimumBalanceForRentExemption(capacityBytes), space: capacityBytes, programId: program }),
        instruction(18, Buffer.concat([modelId, Buffer.from(Uint32Array.of(limit).buffer), modelDoc]), [
          { pubkey: market, isSigner: false, isWritable: false }, { pubkey: model, isSigner: false, isWritable: true },
          { pubkey: terms.publicKey, isSigner: true, isWritable: true }, { pubkey: capacity.publicKey, isSigner: true, isWritable: true },
          { pubkey: provider, isSigner: true, isWritable: false }, { pubkey: provider, isSigner: true, isWritable: true },
          { pubkey: SystemProgram.programId, isSigner: false, isWritable: false }, { pubkey: profile, isSigner: false, isWritable: false },
          { pubkey: metadata, isSigner: false, isWritable: true },
        ]),
      ), [terms, capacity]);
      modelAccount = await rpc.getAccountInfo(model);
    }
    if (!modelAccount || modelAccount.owner.toBase58() !== program.toBase58() || modelAccount.data.subarray(0, 8).toString() !== "MODELV20") throw Error("Model account invalid");
    const chainTerms = new PublicKey(modelAccount.data.subarray(72, 104));
    const chainCapacity = new PublicKey(modelAccount.data.subarray(104, 136));
    const currentMetadata = await rpc.getAccountInfo(metadata);
    if (currentMetadata && !currentMetadata.data.subarray(104, 136).equals(modelDoc.subarray(0, 32))) await send("model metadata updated", new Transaction().add(instruction(22, modelDoc, [
      { pubkey: market, isSigner: false, isWritable: false }, { pubkey: model, isSigner: false, isWritable: false },
      { pubkey: metadata, isSigner: false, isWritable: true }, { pubkey: provider, isSigner: true, isWritable: false },
    ])));

    const termsAccount = await rpc.getAccountInfo(chainTerms);
    if (!termsAccount || termsAccount.data.subarray(0, 8).toString() !== "TERMSV20") throw Error("Terms book invalid");
    const count = termsAccount.data.readUInt32LE(44);
    const existingSlot = Array.from({ length: count }, (_, slot) => slot).find(slot => termsAccount.data.subarray(48 + slot * 120, 80 + slot * 120).equals(provider.toBuffer()));
    const offerSlot = existingSlot ?? count;
    const slot = Buffer.alloc(4); slot.writeUInt32LE(offerSlot);
    if (existingSlot === undefined) {
      const [bondAuthority] = PublicKey.findProgramAddressSync([Buffer.from("bond"), model.toBuffer(), slot], program);
      const payout = getAssociatedTokenAddressSync(mint, provider);
      const bondVault = getAssociatedTokenAddressSync(mint, bondAuthority, true);
      const termsData = Buffer.alloc(24);
      termsData.writeUInt32LE(offerSlot, 0);
      termsData.writeUInt32LE(offer.input_rate, 4);
      termsData.writeUInt32LE(offer.cached_rate, 8);
      termsData.writeUInt32LE(offer.output_rate, 12);
      termsData.writeUInt16LE(offer.speed_tps, 16);
      termsData[18] = 0;
      termsData[19] = 1;
      termsData.writeUInt32LE(offer.capacity, 20);
      await send("offer registered", new Transaction().add(
        createAssociatedTokenAccountIdempotentInstruction(provider, payout, provider, mint),
        createAssociatedTokenAccountIdempotentInstruction(provider, bondVault, bondAuthority, mint),
        instruction(2, termsData, [
          { pubkey: market, isSigner: false, isWritable: false }, { pubkey: model, isSigner: false, isWritable: false },
          { pubkey: chainTerms, isSigner: false, isWritable: true }, { pubkey: chainCapacity, isSigner: false, isWritable: true },
          { pubkey: profile, isSigner: false, isWritable: false }, { pubkey: provider, isSigner: true, isWritable: false },
          { pubkey: payout, isSigner: false, isWritable: false }, { pubkey: bondVault, isSigner: false, isWritable: false },
        ]),
      ));
    }
    const currentCapacity = await rpc.getAccountInfo(chainCapacity);
    const currentTerms = await rpc.getAccountInfo(chainTerms);
    if (!currentCapacity || !currentTerms || currentCapacity.data.readUInt32LE(44) < 1 || currentTerms.data.readUInt32LE(44) < 1) throw Error("Offer books unavailable");
    const desired = Buffer.alloc(20);
    desired.writeUInt32LE(offer.input_rate, 0);
    desired.writeUInt32LE(offer.cached_rate, 4);
    desired.writeUInt32LE(offer.output_rate, 8);
    desired.writeUInt16LE(offer.speed_tps, 12);
    desired[14] = 0;
    desired[15] = 1;
    desired.writeUInt32LE(offer.capacity, 16);
    const current = currentTerms.data.subarray(48 + offerSlot * 120 + 96, 48 + offerSlot * 120 + 112);
    if (!current.equals(desired.subarray(0, 16)) || currentTerms.data.readUInt32LE(48 + offerSlot * 120 + 116) !== offer.capacity) {
      await send("offer terms updated", new Transaction().add(instruction(5, Buffer.concat([slot, desired]), [
        { pubkey: market, isSigner: false, isWritable: false }, { pubkey: model, isSigner: false, isWritable: false },
        { pubkey: chainTerms, isSigner: false, isWritable: true }, { pubkey: provider, isSigner: true, isWritable: false },
      ])));
    }
    const funded = Number(currentCapacity.data.readBigUInt64LE(48 + offerSlot * 32));
    const target = offer.bond_target_atomic;
    if (!Number.isSafeInteger(target) || target < 0) throw Error("Invalid bond target");
    if (funded < target) {
      const amount = target - funded;
      const source = getAssociatedTokenAddressSync(mint, provider);
      const balance = await rpc.getTokenAccountBalance(source).then(result => Number(result.value.amount));
      if (balance < amount) { console.log("model awaiting bond", hosted.id, "shortfall USDC", (amount - balance) / 1_000_000); continue; }
      const bondVault = new PublicKey(currentTerms.data.subarray(48 + offerSlot * 120 + 64, 48 + offerSlot * 120 + 96));
      const payload = Buffer.alloc(12);
      payload.writeUInt32LE(offerSlot, 0);
      payload.writeBigUInt64LE(BigInt(amount), 4);
      await send("provider bond funded", new Transaction().add(instruction(4, payload, [
        { pubkey: market, isSigner: false, isWritable: false }, { pubkey: model, isSigner: false, isWritable: false },
        { pubkey: chainTerms, isSigner: false, isWritable: false }, { pubkey: chainCapacity, isSigner: false, isWritable: true },
        { pubkey: provider, isSigner: true, isWritable: false }, { pubkey: source, isSigner: false, isWritable: true },
        { pubkey: bondVault, isSigner: false, isWritable: true }, { pubkey: mint, isSigner: false, isWritable: false },
        { pubkey: TOKEN_PROGRAM_ID, isSigner: false, isWritable: false },
      ])));
    }
    console.log("model ready", hosted.id, model.toBase58());
  }
} finally {
  payer.secretKey.fill(0);
  await redis.quit();
}
