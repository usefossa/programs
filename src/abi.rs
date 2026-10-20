// all integers little-endian; token amounts use six decimals; rates per million tokens.
// custom errors: 1 arithmetic, 2 account identity/layout, 3 payload, 4 authorization,
// 6 no eligible offer/full book, 7 lifecycle/deadline, 8 usage/collateral constraint.
pub const MARKET_LEN: usize = 72;
pub const MODEL_LEN: usize = 144;
pub const URI_MAX: usize = 256;
pub const PROVIDER_LEN: usize = 108 + URI_MAX;
pub const MODEL_METADATA_LEN: usize = 140 + URI_MAX;
pub const USER_LEN: usize = 120;
pub const BOOKING_LEN: usize = 368;
pub const UNBOND_LEN: usize = 160;
pub const HEADER_LEN: usize = 48;
pub const TERMS_LEN: usize = 120;
pub const CAPACITY_LEN: usize = 32;
pub const MAX_ENTRIES: usize = 4096;
pub const UNIT: u64 = 1_000_000;
pub const MARKET_MAGIC: &[u8; 8] = b"FOSSAV20";
pub const MODEL_MAGIC: &[u8; 8] = b"MODELV20";
pub const PROVIDER_MAGIC: &[u8; 8] = b"PROV0V20";
pub const MODEL_METADATA_MAGIC: &[u8; 8] = b"MMETAV20";
pub const USER_MAGIC: &[u8; 8] = b"USER0V20";
pub const TERMS_MAGIC: &[u8; 8] = b"TERMSV20";
pub const CAPACITY_MAGIC: &[u8; 8] = b"CAPACV20";
pub const BOOK_MAGIC: &[u8; 8] = b"BOOK0V20";
pub const UNBOND_MAGIC: &[u8; 8] = b"UNBNDV20";

// market: magic0, admin8[32], mint40[32]. immutable after initialization.
// model: magic0, market8[32], model_id40[32], terms72[32], capacity104[32], limit136:u32, bump140:u8.
// model pda: [b"model", market, model_id]. model_id is an opaque model/version identifier.
// approved providers initialize models; canonical ids and book bindings remain immutable.
// provider pda: [b"provider", market, provider]. magic0, market8[32], authority40[32],
// document_sha25672[32], uri_len104:u16, approved106:u8, bump107:u8, uri108[256].
// model metadata pda: [b"model_metadata", model]. magic0, market8[32], model40[32],
// authority72[32], document_sha256104[32], uri_len136:u16, bump138:u8, reserved139:u8, uri140[256].
// metadata authorities are immutable. uri is 1..=256 utf-8 bytes, no ascii controls/whitespace;
// unused uri bytes are zeroed. hashes are exact caller-supplied sha-256 bytes; json is never fetched/parsed.
// metadata payload: document_sha2560[32], uri32[remaining bytes], no length prefix.
// provider revocation blocks new models/offers only. existing offers retain their per-offer approval;
// admin uses opcode 16 to disable them. pinned bookings and owner metadata updates remain valid.
// metadata/profile accounts never participate in matching or booking settlement.
// book header: magic0, model8[32], limit40:u32, count44:u32. entries append forever, never reused.
// terms entry: provider0[32], payout32[32], bond_vault64[32], rates96[3*u32], speed108:u16,
// privacy110:u8, active111:u8, approved112:u8, bond_bump113:u8, reserved114[2], capacity116:u32.
// bond authority pda: [b"bond", model, slot.to_le_bytes()]. each slot owns a separate token vault.
// capacity entry: funded_bond0:u64, exposure8:u64, active16:u32, obligations20:u32, pending_unbond24:u64.
// user: magic0, owner8[32], market40[32], free72:u64, vault80[32], bump112:u8.
// user vault authority pda: [b"vault", user]. no vault address can back two lanes/slots.
// booking: magic0, owner8[32], provider40[32], market72[32], reserve104:u64, rates112[3*u32],
// max_input124:u32, max_output128:u32, claim_end132:i64, dispute_end140:i64, status148:u8,
// charge160:u64, usage168[3*u32], min_speed180:u16, user192[32], model224[32], rent_payer256[32],
// payout288[32], bond_vault320[32], slot352:u32, bond_bump356:u8, requested_privacy357:u8,
// cache_allowed358:u8, reserved359:u8, estimated_score360:u64.
// status: 0 reserved, 1 claimed, 2 disputed, 3 released/expired, 4 finalized.
// unbond: magic0, market8[32], provider40[32], amount72:u64, ready80:i64, executed88:u8,
// slot92:u32, model96[32], rent_payer128[32].

// account order; w writable, s signer; remaining accounts readonly.
// 0 initialize market: [market(ws), admin(s), mint]; payload empty.
// 1 initialize user: [user(ws), owner(s), market, vault]; payload empty.
// 2 register: [market, model, terms(w), capacity(w), provider_profile, provider(s), payout, bond_vault];
//   append admin(s) for another offer by the same provider in this model, unless provider is admin.
//   payload slot:u32, input_rate:u32, cached_rate:u32, output_rate:u32, speed:u16,
//   privacy:u8, active:u8, capacity:u32 (24 bytes). slot must equal current count. approved provider required.
//   new offer starts approved; admin can change its approval with opcode 16.
// 3 deposit: [market, user(w), owner(s), source(w), vault(w), mint, token_program]; amount:u64.
// 4 fund: [market, model, terms, capacity(w), provider(s), source(w), bond_vault(w), mint, token_program]; slot:u32, amount:u64.
// 5 update: [market, model, terms(w), provider(s)]; slot:u32, rates/speed/privacy/active/capacity (24 bytes).
// 6 book: [market, model, terms, capacity(w), user(w), owner(s), booking(ws), rent_payer(s)];
//   payload model_id0[32], rate_caps32[3*u32], min_speed44:u16, max_input46:u32,
//   max_output50:u32, privacy54:u8, max_spend55:u64, estimated_input63:u32,
//   estimated_cached67:u32, estimated_output71:u32, cache_provider75[32] (107 bytes).
//   estimated_input includes cached tokens; estimates bounded by limits. zero cache_provider requires zero cached estimate.
//   all three rate caps filter every offer, including providers whose cached rate cannot apply.
//   score = ceil(estimated_uncached*input_rate/UNIT) + ceil(estimated_cached*cached_rate/UNIT)
//         + ceil(estimated_output*output_rate/UNIT); cached estimate becomes zero for other providers.
//   minimum score wins; equal score chooses lower permanent slot. estimates do not prove cache availability.
//   reserve = ceil(max_input*input_rate/UNIT) + ceil(max_output*output_rate/UNIT) + 1.
//   claim may report cached usage only for requested cache_provider, still subject to signed provider claim/dispute.
// 7 claim: [market, model, capacity(w), user(w), booking(w), provider(s), user_vault(w),
//   payout(w), mint, user_authority, token_program]; uncached:u32, cached:u32, output:u32.
//   reusable claims append bucket(w) and generation:u64, epoch:i64, index:u64, receipt[120]
//   after usage (156 payload bytes). top-level only; context/body must match on-chain settlement.
// 8 expire: [market, model, capacity(w), user(w), booking(w)]; empty.
// 9 finalize: [market, model, capacity(w), booking(w)]; empty.
// 10 withdraw: [market, user(w), owner(s), vault(w), destination(w), mint, user_authority, token_program]; amount:u64.
// 11 dispute: [market, model, booking(w), owner(s)]; empty.
// 12 resolve: [market, model, capacity(w), user(w), booking(w), admin(s), bond_vault(w),
//   user_vault(w), mint, bond_authority, token_program]; refund:u64.
// 13 request unbond: [market, model, terms, capacity(w), provider(s), request(ws), rent_payer(s)]; slot:u32, amount:u64.
// 14 execute unbond: [market, model, terms, capacity(w), request(w), provider(s), bond_vault(w),
//   payout(w), mint, bond_authority, token_program]; empty.
// 15 release: [market, model, capacity(w), user(w), booking(w), provider(s)]; empty.
// 16 approve: [market, model, terms(w), admin(s)]; slot:u32, approved:u8.
// 17 close: [booking_or_unbond(w), recorded_rent_payer(w)]; empty, terminal states only.
// 18 initialize model: [market, model_pda(w), terms(ws), capacity(ws), provider(s), payer(ws),
//   system_program, provider_profile, model_metadata_pda(w)]; approved provider required.
//   market admin must authorize creation; append admin(s) unless provider is admin.
//   payload model_id0[32], limit32:u32, document_sha25636[32], uri68[remaining bytes].
//   precreate zeroed program-owned books sized HEADER_LEN + limit*entry_len.
//   initializes canonical model and creator-owned metadata atomically; same model id cannot be replaced.
// 19 initialize provider: [market, provider_profile_pda(w), provider(s), payer(ws), system_program];
//   metadata payload. self-registration starts unapproved; repeat initialization rejected.
// 20 approve provider: [market, provider_profile(w), admin(s)]; approved:u8 (0 or 1).
// 21 update provider metadata: [market, provider_profile(w), provider(s)]; metadata payload.
// 22 update model metadata: [market, model, model_metadata(w), creator(s)]; metadata payload.
//   opcode 18/19 allocate and fund system-owned empty pdas, tolerating prior lamport donations.
//   opcode 21/22 change only uri/hash/uri_len; same uri with a new document hash is supported.
// 27 close receipt: [bucket(w), recorded_rent_payer(w), authority(s)]; empty.
//   authority is recorded rent payer or provider. finalized, empty liabilities, expired epoch/deadlines.
//   refunds only recorded rent payer; opened disputes retain independent case accounts.

pub const fn terms_book_len(limit: usize) -> usize {
    HEADER_LEN + limit * TERMS_LEN
}
pub const fn capacity_book_len(limit: usize) -> usize {
    HEADER_LEN + limit * CAPACITY_LEN
}
