import { ComputeBudgetProgram, Connection, Keypair, PublicKey, SystemProgram, Transaction, TransactionInstruction, type AccountMeta } from '@solana/web3.js'
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { resolve } from 'node:path'
import assert from 'node:assert/strict'

type Offer = { provider: string; providerKey: number[]; profile: string; payout: string; bond: string; authority: string }
type Model = { market: string; mint: string; model: string; modelID: number[]; terms: string; capacity: string; offers: Offer[]; creator: string; creatorKey: number[]; profile: string; metadata: string }
type User = { model: string; owner: string; ownerKey: number[]; user: string; vault: string; authority: string }
type Payer = { pubkey: string; key: number[] }
type Metadata = { program: string; fixture: string; models: Model[]; users: User[]; payers: Payer[]; admin: string; adminKey: number[] }
type Prepared = { tx: Transaction; payer: Keypair; signers: Keypair[]; model: Model; user: User; booking: Keypair; stage: string }
type Landed = { signature: string; slot: number; elapsedMs: number; prepared: Prepared }
type Phase = { label: string; submitted: number; landed: number; failed: number; elapsedMs: number; landedPerSlot: Record<string, number>; slots: number; maxPerSlot: number; observedTps: number; p50Ms: number; p95Ms: number }

const base = resolve(import.meta.dir, '../..'), output = resolve(base, process.env.FOSSA_V2_RESULT_DIR ?? 'target/stress')
const narrow = process.env.FOSSA_V2_CASE === 'one-model'
const checkMetadata = process.env.FOSSA_V2_METADATA === '1'
const metadata: Metadata = JSON.parse(readFileSync(resolve(base, '.local/stress/metadata.json'), 'utf8'))
const url = process.env.FOSSA_V2_RPC_URL ?? 'http://127.0.0.1:19099'
assert(['127.0.0.1', 'localhost', '[::1]'].includes(new URL(url).hostname), 'local rpc only')
const rpc = new Connection(url, 'confirmed'), program = new PublicKey(metadata.program)
const token = new PublicKey('TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA'), bookingLen = 368
const pk = (s: string) => new PublicKey(s), key = (s: number[]) => Keypair.fromSecretKey(Uint8Array.from(s))
const meta = (s: string | PublicKey, isWritable = false, isSigner = false): AccountMeta => ({ pubkey: typeof s === 'string' ? pk(s) : s, isWritable, isSigner })
const ix = (op: number, data: Buffer, keys: AccountMeta[]) => new TransactionInstruction({ programId: program, data: Buffer.concat([Buffer.from([op]), data]), keys })
const pause = (ms: number) => Bun.sleep(ms)
const loadedBudget = (bytes: number) => {
  const data = Buffer.alloc(5); data[0] = 4; data.writeUInt32LE(bytes, 1)
  return new TransactionInstruction({ programId: ComputeBudgetProgram.programId, keys: [], data })
}
const phases: Phase[] = [], samples: object[] = []
const metadataCases: object[] = []
mkdirSync(output, { recursive: true })
let rent = 0

async function accounts(keys: PublicKey[]) {
  const result = []
  for (let i = 0; i < keys.length; i += 100) result.push(...await rpc.getMultipleAccountsInfo(keys.slice(i, i + 100)))
  return result
}
async function tx(instructions: TransactionInstruction[], payer: Keypair, extra: Keypair[], units: number, loadedBytes: number) {
  const block = await rpc.getLatestBlockhash('confirmed')
  const value = new Transaction({ feePayer: payer.publicKey, recentBlockhash: block.blockhash }).add(
    ComputeBudgetProgram.setComputeUnitLimit({ units }),
    loadedBudget(loadedBytes),
    ...instructions,
  )
  value.sign(...new Map([payer, ...extra].map(k => [k.publicKey.toBase58(), k])).values())
  assert(value.serialize().length <= 1232, 'transaction exceeds packet size')
  return value
}
async function send(value: Transaction) {
  const signature = await rpc.sendRawTransaction(value.serialize(), { skipPreflight: false, maxRetries: 5 })
  const limit = performance.now() + 30_000
  while (performance.now() < limit) {
    const status = (await rpc.getSignatureStatuses([signature])).value[0]
    if (status?.err) throw Error(JSON.stringify(status.err))
    if (status?.confirmationStatus === 'confirmed' || status?.confirmationStatus === 'finalized') return signature
    await pause(100)
  }
  throw Error('confirmation timeout for ' + signature)
}
const metadataPayload = (uri: string, revision: number) => Buffer.concat([
  Buffer.from(new Bun.CryptoHasher('sha256').update(JSON.stringify({ revision })).digest()),
  Buffer.from(uri),
])
async function rejected(value: Transaction, code: number, watched: PublicKey[]) {
  const before = await accounts(watched)
  const signature = await rpc.sendRawTransaction(value.serialize(), { skipPreflight: true, maxRetries: 5 })
  let error: unknown
  for (let i = 0; i < 300; i++) {
    const status = (await rpc.getSignatureStatuses([signature])).value[0]
    if (status?.err) { error = status.err; break }
    assert(!status?.confirmationStatus, 'expected rejection succeeded')
    await pause(100)
  }
  assert(JSON.stringify(error).includes('"Custom":' + code), 'wrong rejection: ' + JSON.stringify(error))
  assert.deepEqual(await accounts(watched), before, 'failed metadata transaction mutated accounts')
  metadataCases.push({ rejectedCustom: code, rollback: true })
}
async function metadataChecks() {
  const m = metadata.models[0]!, offer = m.offers[0]!, provider = key(offer.providerKey), payer = key(metadata.payers[65]!.key), admin = key(metadata.adminKey)
  const profileKey = pk(offer.profile), modelMetadata = pk(m.metadata), uri = 'https://example.test/provider.json'
  const original = (await rpc.getAccountInfo(profileKey))!.data
  assert.equal(original.length, 364); assert.equal(original.subarray(0, 8).toString(), 'PROV0V20')
  const update = (payload: Buffer, signer: Keypair) => ix(21, payload, [meta(m.market), meta(profileKey, true), meta(signer.publicKey, false, true)])
  const payload = metadataPayload(uri, 1)
  await send(await tx([update(payload, provider)], payer, [provider], 8_000, 300_000))
  let stored = (await rpc.getAccountInfo(profileKey))!.data
  assert(stored.subarray(72, 104).equals(payload.subarray(0, 32)))
  assert.equal(stored.subarray(108, 108 + stored.readUInt16LE(104)).toString(), uri)
  assert(stored.subarray(108 + Buffer.byteLength(uri)).every(x => x === 0))
  await rejected(await tx([update(metadataPayload(uri, 2), key(m.creatorKey))], payer, [key(m.creatorKey)], 8_000, 300_000), 4, [profileKey])
  await rejected(await tx([update(Buffer.concat([Buffer.alloc(32), Buffer.alloc(257, 97)]), provider)], payer, [provider], 8_000, 300_000), 3, [profileKey])
  await rejected(await tx([ix(20, Buffer.from([0]), [meta(m.market), meta(profileKey, true), meta(provider.publicKey, false, true)])], payer, [provider], 8_000, 300_000), 4, [profileKey])
  const approve = (enabled: number) => ix(20, Buffer.from([enabled]), [meta(m.market), meta(profileKey, true), meta(admin.publicKey, false, true)])
  await send(await tx([approve(0)], payer, [admin], 8_000, 300_000))
  await send(await tx([update(metadataPayload(uri, 3), provider)], payer, [provider], 8_000, 300_000))
  stored = (await rpc.getAccountInfo(profileKey))!.data; assert.equal(stored[106], 0)
  assert(stored.subarray(72, 104).equals(metadataPayload(uri, 3).subarray(0, 32)))
  await send(await tx([approve(1)], payer, [admin], 8_000, 300_000))
  const modelPayload = metadataPayload('https://example.test/model.json', 4), creator = key(m.creatorKey)
  await send(await tx([ix(22, modelPayload, [meta(m.market), meta(m.model), meta(modelMetadata, true), meta(creator.publicKey, false, true)])], payer, [creator], 8_000, 300_000))
  const md = (await rpc.getAccountInfo(modelMetadata))!.data
  assert.equal(md.length, 396); assert(md.subarray(104, 136).equals(modelPayload.subarray(0, 32)))
  await rejected(await tx([ix(22, modelPayload, [meta(m.market), meta(m.model), meta(modelMetadata, true), meta(provider.publicKey, false, true)])], payer, [provider], 8_000, 300_000), 4, [modelMetadata])
  // a size filter is sufficient because no other v2 account uses this fixed length.
  const profiles = await rpc.getProgramAccounts(program, { filters: [{ dataSize: 364 }] })
  assert(profiles.some(x => x.pubkey.equals(profileKey)))
  metadataCases.push({ profileUpdate: true, sameUriNewHash: true, revokedMetadataUpdate: true, modelCreatorUpdate: true, indexedProviderAccounts: profiles.length })

  // create fresh records through rpc transactions, including both book allocations and maximum uri.
  const newcomer = Keypair.generate(), terms = Keypair.generate(), capacity = Keypair.generate()
  const profile = PublicKey.findProgramAddressSync([Buffer.from('provider'), pk(m.market).toBuffer(), newcomer.publicKey.toBuffer()], program)[0]
  const id = Buffer.from(new Bun.CryptoHasher('sha256').update(newcomer.publicKey.toBuffer()).digest())
  const model = PublicKey.findProgramAddressSync([Buffer.from('model'), pk(m.market).toBuffer(), id], program)[0]
  const modelMetadataKey = PublicKey.findProgramAddressSync([Buffer.from('model_metadata'), model.toBuffer()], program)[0]
  const maximumUri = 'https://example.test/' + 'x'.repeat(256 - 'https://example.test/'.length)
  await send(await tx([ix(19, metadataPayload(maximumUri, 40), [meta(m.market), meta(profile, true), meta(newcomer.publicKey, false, true), meta(payer.publicKey, true, true), meta(SystemProgram.programId)])], payer, [newcomer], 20_000, 300_000))
  assert.equal((await rpc.getAccountInfo(profile))!.data[106], 0)
  const [termsRent, capacityRent] = await Promise.all([rpc.getMinimumBalanceForRentExemption(528), rpc.getMinimumBalanceForRentExemption(176)])
  const limit = Buffer.alloc(4); limit.writeUInt32LE(4)
  const init = [
    SystemProgram.createAccount({ fromPubkey: payer.publicKey, newAccountPubkey: terms.publicKey, lamports: termsRent, space: 528, programId: program }),
    SystemProgram.createAccount({ fromPubkey: payer.publicKey, newAccountPubkey: capacity.publicKey, lamports: capacityRent, space: 176, programId: program }),
    ix(18, Buffer.concat([id, limit, metadataPayload(maximumUri, 41)]), [meta(m.market), meta(model, true), meta(terms.publicKey, true, true), meta(capacity.publicKey, true, true), meta(newcomer.publicKey, false, true), meta(payer.publicKey, true, true), meta(SystemProgram.programId), meta(profile), meta(modelMetadataKey, true)]),
  ]
  const feePayer = key(metadata.payers[64]!.key), signers = [payer, newcomer, terms, capacity]
  await rejected(await tx(init, feePayer, signers, 50_000, 300_000), 4, [model, modelMetadataKey, terms.publicKey, capacity.publicKey])
  await send(await tx([ix(20, Buffer.from([1]), [meta(m.market), meta(profile, true), meta(admin.publicKey, false, true)])], payer, [admin], 8_000, 300_000))
  const value = await tx(init, feePayer, signers, 50_000, 300_000), signature = await send(value)
  const created = await accounts([model, modelMetadataKey])
  assert.equal(created[0]!.data.length, 144); assert.equal(created[1]!.data.length, 396)
  assert(created[1]!.data.subarray(72, 104).equals(newcomer.publicKey.toBuffer()))
  assert.equal(created[1]!.data.readUInt16LE(136), 256)
  const result = await rpc.getTransaction(signature, { commitment: 'confirmed', maxSupportedTransactionVersion: 0 })
  metadataCases.push({ maxUri: 256, modelCreationBytes: value.serialize().length, signers: value.signatures.length, modelCreationCu: result!.meta!.computeUnitsConsumed, unapprovedCreationRollback: true })
}
async function concurrentMetadata(m: Model, firstBook: Prepared) {
  const offer = m.offers.at(-1)!, provider = key(offer.providerKey), creator = key(m.creatorKey), payer = key(metadata.payers[65]!.key), prepared: Prepared[] = []
  for (let i = 0; i < 32; i++) for (const model of [false, true]) {
    const payload = metadataPayload(model ? 'https://example.test/model.json' : 'https://example.test/provider.json', 10 + i)
    const instruction = model
      ? ix(22, payload, [meta(m.market), meta(m.model), meta(m.metadata, true), meta(creator.publicKey, false, true)])
      : ix(21, payload, [meta(m.market), meta(offer.profile, true), meta(provider.publicKey, false, true)])
    prepared.push({ ...firstBook, tx: await tx([instruction], payer, [model ? creator : provider], 8_000, 300_000), payer, stage: 'metadata-update' })
  }
  assert.equal(overlap([firstBook, prepared[0]!]), 0, 'provider metadata shares booking write locks')
  assert.equal(overlap([firstBook, prepared[1]!]), 0, 'model metadata shares booking write locks')
  return prepared
}
function bookInstruction(m: Model, u: User, b: Keypair, payer: Keypair) {
  const payload = Buffer.alloc(107)
  Buffer.from(m.modelID).copy(payload)
  for (const offset of [32, 36, 40]) payload.writeUInt32LE(5000, offset)
  for (const offset of [46, 50, 63, 71]) payload.writeUInt32LE(1_000_000, offset)
  payload.writeBigUInt64LE(100_000n, 55)
  return ix(6, payload, [meta(m.market), meta(m.model), meta(m.terms), meta(m.capacity, true), meta(u.user, true), meta(u.owner, false, true), meta(b.publicKey, true, true), meta(payer.publicKey, false, true)])
}
async function prepareBookings(models: Model[], count: number, label: string) {
  const block = await rpc.getLatestBlockhash('confirmed'), prepared: Prepared[] = []
  for (let i = 0; i < count; i++) {
    const m = models[i % models.length]!, users = metadata.users.filter(u => u.model === m.model)
    const u = users[Math.floor(i / models.length) % users.length]!, payer = key(metadata.payers[i % metadata.payers.length]!.key), b = Keypair.generate(), owner = key(u.ownerKey)
    const instructions = [
      SystemProgram.createAccount({ fromPubkey: payer.publicKey, newAccountPubkey: b.publicKey, lamports: rent, space: bookingLen, programId: program }),
      bookInstruction(m, u, b, payer),
    ]
    const value = new Transaction({ feePayer: payer.publicKey, recentBlockhash: block.blockhash }).add(
      ComputeBudgetProgram.setComputeUnitLimit({ units: m.offers.length > 1000 ? 620_000 : 15_000 }),
      loadedBudget(m.offers.length > 1000 ? 900_000 : 300_000),
      ...instructions,
    )
    value.sign(payer, owner, b)
    assert(value.serialize().length <= 1232)
    prepared.push({ tx: value, payer, signers: [owner, b], model: m, user: u, booking: b, stage: label })
  }
  return prepared
}
function locks(p: Prepared) {
  const msg = p.tx.compileMessage()
  return new Set(msg.accountKeys.filter((_, i) => msg.isAccountWritable(i)).map(k => k.toBase58()))
}
function overlap(prepared: Prepared[]) {
  let pairs = 0
  for (let i = 0; i < prepared.length; i++) for (let j = i + 1; j < prepared.length; j++) if ([...locks(prepared[i]!)].some(k => locks(prepared[j]!).has(k))) pairs++
  return pairs
}
async function run(label: string, prepared: Prepared[]): Promise<Landed[]> {
  console.log('validator phase start ' + label + ' tx=' + prepared.length)
  const started = performance.now(), pending = new Map<string, { prepared: Prepared; started: number }>(), landed: Landed[] = [], errors: object[] = []
  // bounded rpc submission concurrency keeps client overload separate from on-chain admission.
  let cursor = 0, submissionsDone = false
  const submit = Promise.all(Array.from({ length: Math.min(32, prepared.length) }, async () => {
    while (cursor < prepared.length) {
      const p = prepared[cursor++]!, when = performance.now()
      try {
        const signature = await rpc.sendRawTransaction(p.tx.serialize(), { skipPreflight: true, maxRetries: 20 })
        pending.set(signature, { prepared: p, started: when })
      } catch (error) { errors.push({ stage: 'submission', message: String(error) }) }
    }
  })).then(() => { submissionsDone = true })
  const limit = started + 90_000
  while ((!submissionsDone || pending.size) && performance.now() < limit) {
    for (let i = 0, signatures = [...pending.keys()]; i < signatures.length; i += 200) {
      const group = signatures.slice(i, i + 200), statuses = (await rpc.getSignatureStatuses(group)).value
      for (let j = 0; j < group.length; j++) {
        const status = statuses[j], signature = group[j]!, item = pending.get(signature)!
        if (status?.err) { errors.push({ stage: 'execution', signature, error: status.err }); pending.delete(signature) }
        else if (status?.confirmationStatus === 'confirmed' || status?.confirmationStatus === 'finalized') {
          landed.push({ signature, slot: status.slot, elapsedMs: performance.now() - item.started, prepared: item.prepared }); pending.delete(signature)
        }
      }
    }
    await pause(80)
  }
  await submit
  for (const signature of pending.keys()) errors.push({ stage: 'timeout', signature })
  const elapsedMs = performance.now() - started, distribution: Record<string, number> = {}
  for (const item of landed) distribution[item.slot] = (distribution[item.slot] ?? 0) + 1
  const latency = landed.map(x => x.elapsedMs).sort((a, b) => a - b)
  const phase: Phase = { label, submitted: prepared.length, landed: landed.length, failed: errors.length, elapsedMs, landedPerSlot: distribution, slots: Object.keys(distribution).length, maxPerSlot: Math.max(0, ...Object.values(distribution)), observedTps: landed.length / (elapsedMs / 1000), p50Ms: latency[Math.floor(latency.length * .5)] ?? 0, p95Ms: latency[Math.floor(latency.length * .95)] ?? 0 }
  phases.push(phase)
  writeFileSync(resolve(output, label + '.json'), JSON.stringify({ phase, errors }, null, 2))
  console.log(JSON.stringify(phase))
  assert.equal(errors.length, 0, label + ' failed transactions')
  assert.equal(landed.length, prepared.length)
  return landed
}
async function sample(p: Prepared, signature: string) {
  const result = await rpc.getTransaction(signature, { maxSupportedTransactionVersion: 0, commitment: 'confirmed' })
  assert(result?.meta && !result.meta.err)
  const msg = p.tx.compileMessage(), infos = await accounts(msg.accountKeys), w = locks(p)
  const details = { stage: p.stage, entries: p.model.offers.length, executionCu: result.meta.computeUnitsConsumed, bytes: p.tx.serialize().length, accountCount: msg.accountKeys.length, writableCount: w.size, knownLoadedAccountBytes: infos.reduce((n, a) => n + (a?.data.length ?? 0), 0), caveat: 'account byte sum excludes implicitly loaded programdata; does not equal scheduler cost', cuLimit: p.tx.instructions[0]!.data.readUInt32LE(1), loadedLimit: p.tx.instructions[1]!.data.readUInt32LE(1) }
  samples.push(details); console.log(JSON.stringify(details))
}
async function reconcile(booked: Landed[], terminal = false) {
  const bookings = await accounts(booked.map(x => x.prepared.booking.publicKey)), byUser = new Map<string, bigint>(), byModel = new Map<string, Map<number, { exposure: bigint; active: number; obligations: number }>>()
  for (let i = 0; i < bookings.length; i++) {
    const d = bookings[i]?.data
    assert(d && d.length === bookingLen)
    const p = booked[i]!.prepared, slot = d.readUInt32LE(352), state = d[148], reserve = d.readBigUInt64LE(104), charge = d.readBigUInt64LE(160)
    assert.equal(new PublicKey(d.subarray(224, 256)).toBase58(), p.model.model)
    assert.equal(new PublicKey(d.subarray(192, 224)).toBase58(), p.user.user)
    if (terminal) assert([3, 4].includes(state!))
    if (state === 0) byUser.set(p.user.user, (byUser.get(p.user.user) ?? 0n) + reserve)
    const expected = byModel.get(p.model.model) ?? new Map(), current = expected.get(slot) ?? { exposure: 0n, active: 0, obligations: 0 }
    if (state === 0) { current.exposure += reserve; current.active++; current.obligations++ }
    if (state === 1 || state === 2) { current.exposure += charge; current.obligations++ }
    expected.set(slot, current); byModel.set(p.model.model, expected)
  }
  const usedUsers = [...new Map(booked.map(x => [x.prepared.user.user, x.prepared.user])).values()]
  const userAccounts = await accounts(usedUsers.flatMap(u => [pk(u.user), pk(u.vault)]))
  usedUsers.forEach((u, i) => assert(userAccounts[i * 2 + 1]!.data.readBigUInt64LE(64) >= userAccounts[i * 2]!.data.readBigUInt64LE(72) + (byUser.get(u.user) ?? 0n)))
  const models = [...new Map(booked.map(x => [x.prepared.model.model, x.prepared.model])).values()], capacities = await accounts(models.map(m => pk(m.capacity)))
  for (let j = 0; j < models.length; j++) {
    const m = models[j]!, d = capacities[j]!.data, expected = byModel.get(m.model)!
    for (let slot = 0; slot < m.offers.length; slot++) {
      const at = 48 + slot * 32, e = expected.get(slot) ?? { exposure: 0n, active: 0, obligations: 0 }, funded = d.readBigUInt64LE(at), exposure = d.readBigUInt64LE(at + 8)
      assert.equal(exposure, e.exposure); assert.equal(d.readUInt32LE(at + 16), e.active); assert.equal(d.readUInt32LE(at + 20), e.obligations)
      assert(funded >= exposure + (exposure + 99n) / 100n)
    }
  }
}
async function settle(booked: Landed[], label: string) {
  const infos = await accounts(booked.map(x => x.prepared.booking.publicKey)), prepared: Prepared[] = []
  for (let i = 0; i < booked.length; i++) {
    const p = booked[i]!.prepared, d = infos[i]!.data, slot = d.readUInt32LE(352), offer = p.model.offers[slot]!, provider = key(offer.providerKey)
    const usage = Buffer.alloc(12); usage.writeUInt32LE(500_000); usage.writeUInt32LE(500_000, 8)
    const instruction = i % 4 === 0
      ? ix(7, usage, [meta(p.model.market), meta(p.model.model), meta(p.model.capacity, true), meta(p.user.user, true), meta(p.booking.publicKey, true), meta(offer.provider, false, true), meta(p.user.vault, true), meta(offer.payout, true), meta(p.model.mint), meta(p.user.authority), meta(token)])
      : ix(15, Buffer.alloc(0), [meta(p.model.market), meta(p.model.model), meta(p.model.capacity, true), meta(p.user.user, true), meta(p.booking.publicKey, true), meta(offer.provider, false, true)])
    prepared.push({ ...p, tx: await tx([instruction], p.payer, [provider], 18_000, 500_000), stage: label })
  }
  const landed = await run(label, prepared)
  await sample(landed.find(x => x.prepared.tx.instructions[2]!.data[0] === 7)!.prepared, landed.find(x => x.prepared.tx.instructions[2]!.data[0] === 7)!.signature)
  return landed
}
async function finalizeAndClose(booked: Landed[], label: string) {
  let infos = await accounts(booked.map(x => x.prepared.booking.publicKey))
  const waitUntil = Math.max(...infos.filter(x => x?.data[148] === 1).map(x => Number(x!.data.readBigInt64LE(140)))) + 1
  while (((await rpc.getBlockTime(await rpc.getSlot('confirmed'))) ?? 0) < waitUntil) await pause(500)
  const prepared: Prepared[] = []
  for (let i = 0; i < booked.length; i++) {
    const p = booked[i]!.prepared, instructions = []
    if (infos[i]!.data[148] === 1) instructions.push(ix(9, Buffer.alloc(0), [meta(p.model.market), meta(p.model.model), meta(p.model.capacity, true), meta(p.booking.publicKey, true)]))
    instructions.push(ix(17, Buffer.alloc(0), [meta(p.booking.publicKey, true), meta(p.payer.publicKey, true)]))
    prepared.push({ ...p, tx: await tx(instructions, p.payer, [], 8_000, 400_000), stage: label })
  }
  await run(label, prepared)
  infos = await accounts(booked.map(x => x.prepared.booking.publicKey)); assert(infos.every(x => !x))
  const capacities = await accounts(metadata.models.map(m => pk(m.capacity)))
  for (let j = 0; j < metadata.models.length; j++) for (let i = 0; i < metadata.models[j]!.offers.length; i++) {
    const d = capacities[j]!.data, at = 48 + i * 32
    assert.equal(d.readBigUInt64LE(at + 8), 0n); assert.equal(d.readUInt32LE(at + 16), 0); assert.equal(d.readUInt32LE(at + 20), 0)
  }
}

const version = await rpc.getVersion(); rent = await rpc.getMinimumBalanceForRentExemption(bookingLen)
if (checkMetadata) await metadataChecks()
// increase advertised capacity through provider-signed instructions, retaining registered prices and identities.
for (let start = 0; start < metadata.models.length; start += 4) {
  const batch = metadata.models.slice(start, start + 4), infos = await accounts(batch.map(m => pk(m.terms))), payer = key(metadata.payers[0]!.key), signers: Keypair[] = []
  const updates = batch.map((m, j) => {
    const slot = m.offers.length - 1, offer = m.offers[slot]!, d = infos[j]!.data, p = Buffer.alloc(24)
    p.writeUInt32LE(slot); d.copy(p, 4, 48 + slot * 120 + 96, 48 + slot * 120 + 112); p.writeUInt32LE(10_000, 20)
    signers.push(key(offer.providerKey))
    return ix(5, p, [meta(m.market), meta(m.model), meta(m.terms, true), meta(offer.provider, false, true)])
  })
  await send(await tx(updates, payer, signers, 20_000, 1_000_000))
}
const all: Landed[] = []
for (const [label, models] of (narrow ? [['one-model', [metadata.models[0]!]]] : [['one-model', [metadata.models[0]!]], ['many-models', metadata.models.slice(1)]]) as [string, Model[]][]) {
  const prepared = await prepareBookings([...models], 512, label)
  const check = prepared.slice(0, Math.min(models.length, 32)); const conflicts = overlap(check)
  if (models.length > 1) assert.equal(conflicts, 0, 'independent model transactions share a writable account')
  samples.push({ label, comparedTransactions: check.length, writableOverlapPairs: conflicts })
  if (checkMetadata && label === 'one-model') prepared.push(...await concurrentMetadata(models[0]!, prepared[0]!))
  const landed = await run(label, prepared), booked = landed.filter(x => x.prepared.tx.instructions.at(-1)!.data[0] === 6); all.push(...booked)
  if (checkMetadata && label === 'one-model') {
    const updates = landed.filter(x => x.prepared.stage === 'metadata-update')
    assert.equal(updates.length, 64)
    await sample(updates[0]!.prepared, updates[0]!.signature)
    const m = models[0]!, d = await accounts([pk(m.offers.at(-1)!.profile), pk(m.metadata)])
    const hashes = Array.from({ length: 32 }, (_, i) => metadataPayload('', 10 + i).subarray(0, 32))
    assert(hashes.some(hash => d[0]!.data.subarray(72, 104).equals(hash)))
    assert(hashes.some(hash => d[1]!.data.subarray(104, 136).equals(hash)))
    metadataCases.push({ concurrentUpdates: 64, bookingMetadataWriteOverlap: 0, finalHashesValid: true })
  }
  await sample(booked[0]!.prepared, booked[0]!.signature); await reconcile(all)
  await settle(booked, label + '-settle'); await reconcile(all)
}
// update terms concurrently with fresh bookings, showing serialized price writes remain safe.
if (!narrow) {
  const model = metadata.models[1]!, prepared = await prepareBookings([model], 128, 'price-book-mix'), offer = model.offers.at(-1)!
  for (let i = 0; i < 64; i++) {
    const p = Buffer.alloc(24); p.writeUInt32LE(model.offers.length - 1)
    for (const at of [4, 8, 12]) p.writeUInt32LE(i % 2 + 1, at)
    p[16] = 100; p[18] = 1; p[19] = 1; p.writeUInt32LE(10_000, 20)
    const user = prepared[i % prepared.length]!.user, payer = key(metadata.payers[i % metadata.payers.length]!.key)
    prepared.push({ tx: await tx([ix(5, p, [meta(model.market), meta(model.model), meta(model.terms, true), meta(offer.provider, false, true)])], payer, [key(offer.providerKey)], 8_000, 300_000), payer, signers: [], model, user, booking: Keypair.generate(), stage: 'price-book-mix' })
  }
  const mixed = await run('price-book-mix', prepared), booked = mixed.filter(x => x.prepared.tx.instructions.at(-1)!.data[0] === 6)
  all.push(...booked); await reconcile(all); await settle(booked, 'price-book-mix-settle'); await reconcile(all)
}
await finalizeAndClose(all, 'finalize-close')
writeFileSync(resolve(output, 'validator-results.json'), JSON.stringify({ profile: 'local Agave validator; imported reachable initialized/funded state; mainnet throughput unproven', version, fixture: metadata.fixture, phases, samples, metadataCases, totalBookings: all.length, custodyReconciled: true, terminalAccountsClosed: true }, null, 2))
console.log('validator stress passed bookings=' + all.length)
