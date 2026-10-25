// integers little-endian; token amounts use six decimals; rates per million tokens.
// custom errors: 1 arithmetic, 2 account identity/layout, 3 payload, 4 authorization,
// 6 no eligible offer/full book or list, 7 lifecycle/deadline, 8 usage constraint.
pub const MARKET_LEN: usize = 72;
pub const MODEL_LEN: usize = 144;
pub const URI_MAX: usize = 256;
pub const PROVIDER_LEN: usize = 108 + URI_MAX;
pub const MODEL_METADATA_LEN: usize = 140 + URI_MAX;
pub const USER_LEN: usize = 120;
pub const BOOKING_LEN: usize = 368;
pub const HEADER_LEN: usize = 48;
pub const TERMS_LEN: usize = 120;
pub const CAPACITY_LEN: usize = 32;
pub const LIST_HEADER: usize = 80;
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
pub const LIST_MAGIC: &[u8; 8] = b"LIST0V30";

// market: magic0, creator8[32], mint40[32]; immutable.
// model: magic0, market8[32], model_id40[32], terms72[32], capacity104[32], limit136:u32, bump140:u8.
// model pda: [b"model", market, model_id]; first registered provider creates canonical books; limits may grow.
// provider pda: [b"provider", market, provider]. magic0, market8[32], authority40[32],
// document_sha25672[32], uri_len104:u16, reserved106:u8, bump107:u8, uri108[256].
// model metadata pda: [b"model_metadata", model]. magic0, market8[32], model40[32],
// authority72[32], document_sha256104[32], uri_len136:u16, bump138:u8, reserved139:u8, uri140[256].
// metadata authorities are immutable. uri is 1..=256 utf-8 bytes without ascii controls/whitespace;
// unused uri bytes zeroed. metadata payload: document_sha2560[32], uri32[remaining bytes].
// metadata does not participate in matching or settlement. registration requires no admission authority.
// book header: magic0, model8[32], limit40:u32, count44:u32; slots append and never recycle.
// terms: provider0[32], payout32[32], reserved64[32], rates96[3*u32], speed108:u16,
// privacy110:u8, active111:u8, reserved112[4], capacity116:u32. one new offer per provider/model.
// capacity: reserved0[16], active16:u32, reserved20[12]. historical reserved bytes ignored.
// user: magic0, owner8[32], market40[32], free72:u64, vault80[32], bump112:u8, generation113:u56.
// user vault authority pda: [b"vault", user]. no vault backs two user lanes.
// list: magic0, market8[32], authority40[32], limit72:u32, count76:u32, sorted unique providers80[count*32].
// allocated length80+limit*64; unused keys and cache nodes zero. authority receives closing rent.
// derived cache at80+limit*32: count32-byte nodes in dense eytzinger order, children2i+1/2i+2.
// inorder equals sorted identities; each node stores four u64le values decoded from key chunks as big-endian.
// program-maintained cache permits bounded full-key membership without allocation or hashing.
// booking: magic0, owner8[32], provider40[32], market72[32], reserve104:u64, rates112[3*u32],
// max_input124:u32, max_output128:u32, claim_end132:i64, reserved140[8], status148:u8,
// generation152:u64, charge160:u64, usage168[3*u32], min_speed180:u16, user192[32], model224[32],
// rent_payer256[32], payout288[32], provider_list320[32], slot352:u32, reserved356:u8,
// requested_privacy357:u8, cache_allowed358:u8, reserved359:u8, estimated_score360:u64.
// status: 0 reserved, 3 released/expired, 4 finalized, 5 available for reuse. claim finalizes immediately.
// receipt120: owner0[32], user32[32], booking64[32], generation96:u64, charge104:u64, claim_end112:i64.
// receipts authenticated in top-level claim instruction data; no separate receipt accounts.

// accounts: w writable, s signer; all others readonly. exact account counts required.
// 0 market: [market(ws), creator(s), mint]; empty.
// 1 user: [user(ws), owner(s), market, vault]; empty.
// 2 offer: [market, model, terms(w), capacity(w), provider_profile, provider(s), payout];
//   slot:u32, rates[3*u32], speed:u16, privacy:u8, active:u8, capacity:u32 (24 bytes).
// 3 deposit: [market, user(w), owner(s), source(w), vault(w), mint, token_program]; amount:u64.
// 5 update offer: [market, model, terms(w), provider(s)]; same payload as2.
// 6 book: [market, model, terms, capacity(w), user(w), owner(s), booking(ws), rent_payer(s), list];
//   model_id0[32], rate_caps32[3*u32], min_speed44:u16, max_input46:u32, max_output50:u32,
//   privacy54:u8, max_spend55:u64, estimated_input63:u32, estimated_cached67:u32,
//   estimated_output71:u32, cache_provider75[32] (107 bytes); reusable appends generation107:u64.
//   existing available booking and recorded rent payer need not sign reusable bookings; user owner signs.
//   exact lowest estimated charge among eligible listed providers wins; lower-slot ties.
//   all rate caps apply. cached estimate applies only to matching cache_provider. no proven cache hit.
//   reserve = ceil(max_input*input_rate/UNIT)+ceil(max_output*output_rate/UNIT)+1.
//   selected list address pinned; later list edits do not invalidate existing bookings or settlement.
// 7 claim: [market, model, capacity(w), user(w), booking(w), provider(s), user_vault(w),
//   payout(w), mint, user_authority, token_program]; usage[3*u32]. reusable appends generation:u64,
//   receipt[120] (140 bytes). receipt body and generation must match actual settlement; top-level only.
// 8 expire: [market, model, capacity(w), user(w), booking(w)]; empty or reusable generation:u64.
// 10 withdraw: [market, user(w), owner(s), vault(w), destination(w), mint, user_authority, token_program]; amount:u64.
// 15 release: [market, model, capacity(w), user(w), booking(w), provider(s)]; empty or generation:u64.
// 17 close booking: [booking(w), recorded_rent_payer(w)]; empty, terminal only; available requires payer signature.
// 18 model: [market, model_pda(w), terms(ws), capacity(ws), provider(s), payer(ws), system_program,
//   provider_profile, model_metadata_pda(w)]; model_id0[32], limit32:u32, metadata36[remaining bytes].
// 19 provider: [market, provider_profile_pda(w), provider(s), payer(ws), system_program]; metadata.
// 21 provider metadata: [market, provider_profile(w), provider(s)]; metadata.
// 22 model metadata: [market, model, model_metadata(w), creator(s)]; metadata.
// 26 reusable booking: [market, user, owner(s), booking(ws), rent_payer(s)]; empty.
// 28 list: [market, list(ws), authority(s)]; limit:u32; zeroed program-owned account preallocated.
// 29 membership: [market, list(w), authority(s)]; provider[32], included:u8; duplicate add/missing remove reject.
// 30 close list: [list(w), authority(ws)]; empty; list must be empty.
// 32 grow: [market, model(w), terms(w), capacity(w), payer(ws), system_program]; new_limit:u32.
//   increase only, at most85 slots per instruction, maximum4096; payer covers rent, data retained.
// removed opcodes4,9,11,12,13,14,16,20,23,24,25,27 reject; never repurposed.

pub const fn terms_book_len(limit: usize) -> usize {
    HEADER_LEN + limit * TERMS_LEN
}
pub const fn capacity_book_len(limit: usize) -> usize {
    HEADER_LEN + limit * CAPACITY_LEN
}
pub const fn provider_list_len(limit: usize) -> usize {
    LIST_HEADER + limit * 64
}
