use anchor_lang::prelude::*;

pub const URI_MAX: usize = 256;
pub const MAX_ENTRIES: usize = 4096;
pub const MAX_BOOK_GROWTH: usize = 10_240 / core::mem::size_of::<LegacyTermsEntry>();
pub const MODALITY_TEXT: u8 = 1;
pub const MODALITY_IMAGE: u8 = 2;
pub const MODALITY_AUDIO: u8 = 4;
pub const MODALITY_VIDEO: u8 = 8;
pub const MODALITIES: u8 = MODALITY_TEXT | MODALITY_IMAGE | MODALITY_AUDIO | MODALITY_VIDEO;
pub const OFFER_VERSION: u8 = 1;
pub const LIST_OFFER_VERSION: u8 = 2;

#[account]
pub struct Market {
    pub council: Address,
    pub mint: Address,
    pub bank: Address,
    pub creation_mint: Address,
    pub creation_token_program: Address,
    pub creation_net_price: PodU64,
    pub creation_enabled: u8,
    pub reserved: [u8; 7],
    pub dex_program: Address,
    pub dex_config: Address,
}

#[account]
pub struct Model {
    pub market: Address,
    pub model_id: Address,
    pub terms: Address,
    pub capacity: Address,
    pub limit: PodU32,
    pub bump: u8,
    pub reserved: [u8; 3],
    pub group: Address,
}

#[account]
pub struct ModelGroup {
    pub market: Address,
    pub group_id: Address,
    pub backing_vault: Address,
    pub fixed_token_mint: Address,
    pub route_to_reserve: u8,
    pub bump: u8,
    pub reserved: [u8; 6],
}

#[account]
pub struct Provider {
    pub market: Address,
    pub authority: Address,
    pub document_hash: [u8; 32],
    pub uri_len: PodU16,
    pub policy_registered: u8,
    pub bump: u8,
    pub uri: [u8; URI_MAX],
}

#[account]
pub struct ModelMetadata {
    pub market: Address,
    pub target: Address,
    pub authority: Address,
    pub document_hash: [u8; 32],
    pub uri_len: PodU16,
    pub bump: u8,
    pub reserved: u8,
    pub uri: [u8; URI_MAX],
}

#[account]
pub struct ListMetadata {
    pub market: Address,
    pub target: Address,
    pub authority: Address,
    pub document_hash: [u8; 32],
    pub uri_len: PodU16,
    pub bump: u8,
    pub reserved: u8,
    pub uri: [u8; URI_MAX],
}

#[account]
pub struct User {
    pub owner: Address,
    pub market: Address,
    pub free: PodU64,
    pub vault: Address,
    pub generation: PodU64,
    pub bump: u8,
    pub reserved: [u8; 7],
}

#[account]
pub struct BookingTerms {
    pub model: Address,
    pub limit: PodU32,
}

// new discriminator prevents old slab entries from being read with a different stride.
pub type Terms = BookingTerms;

#[account]
pub struct Capacity {
    pub model: Address,
    pub limit: PodU32,
}

#[repr(C)]
#[derive(Clone, Copy, anchor_lang::bytemuck::Pod, anchor_lang::bytemuck::Zeroable)]
pub struct TermsEntry {
    pub provider: Address,
    pub payout: Address,
    pub checkpoint: [u8; 32],
    pub input_rate: PodU32,
    pub cached_rate: PodU32,
    pub output_rate: PodU32,
    pub speed: PodU16,
    pub privacy: u8,
    pub active: u8,
    pub input_modalities: u8,
    pub output_modalities: u8,
    pub version: u8,
    pub reserved: u8,
    pub capacity: PodU32,
    pub context_window: PodU32,
    pub max_input: PodU32,
    pub max_output: PodU32,
    pub provider_policy: PodU32,
}

#[repr(C)]
#[derive(Clone, Copy, anchor_lang::bytemuck::Pod, anchor_lang::bytemuck::Zeroable)]
pub struct CapacityEntry {
    pub reserved: [u8; 16],
    pub active: PodU32,
    pub reserved2: [u8; 12],
}

#[account]
pub struct List {
    pub market: Address,
    pub authority: Address,
    pub limit: PodU32,
    pub fee_bps: PodU16,
    pub protocol_share: PodU16,
    pub reserved: [u8; 4],
}

#[account]
pub struct FilteredBooking {
    pub owner: Address,
    pub provider: Address,
    pub market: Address,
    pub reserve: PodU64,
    pub input_rate: PodU32,
    pub cached_rate: PodU32,
    pub output_rate: PodU32,
    pub max_input: PodU32,
    pub max_output: PodU32,
    pub claim_end: PodI64,
    pub provider_policy: PodU32,
    pub reserved: [u8; 4],
    pub status: u8,
    pub reserved2: [u8; 3],
    pub generation: PodU64,
    pub charge: PodU64,
    pub input: PodU32,
    pub cached: PodU32,
    pub output: PodU32,
    pub min_speed: PodU16,
    pub reserved3: [u8; 10],
    pub user: Address,
    pub model: Address,
    pub rent_payer: Address,
    pub payout: Address,
    pub provider_list: Address,
    pub slot: PodU32,
    pub wallet_booking_counted: u8,
    pub requested_privacy: u8,
    pub cache_allowed: u8,
    pub reserved5: u8,
    pub estimated_score: PodU64,
    pub creator: Address,
    pub protocol_destination: Address,
    pub reserve_authority: Address,
    pub fee_bps: PodU16,
    pub protocol_share: PodU16,
    pub reserve_destination: u8,
    pub reserved6: [u8; 3],
    pub checkpoint: [u8; 32],
    pub offer_context_window: PodU32,
    pub offer_max_input: PodU32,
    pub offer_max_output: PodU32,
    pub offer_input_modalities: u8,
    pub offer_output_modalities: u8,
    pub requested_input_modalities: u8,
    pub requested_output_modalities: u8,
}

// legacy zero markers remain settleable; new reservations always record wallet usage.
pub type Booking = FilteredBooking;

#[repr(C)]
#[derive(Clone, Copy, anchor_lang::bytemuck::Pod, anchor_lang::bytemuck::Zeroable)]
pub struct Receipt {
    pub owner: Address,
    pub user: Address,
    pub booking: Address,
    pub generation: PodU64,
    pub charge: PodU64,
    pub claim_end: PodI64,
}

#[account]
pub struct BookingPolicies {
    pub market: Address,
    pub limited_providers: PodU32,
    pub bump: u8,
    pub reserved: [u8; 3],
}

#[repr(C)]
#[derive(Clone, Copy, anchor_lang::bytemuck::Pod, anchor_lang::bytemuck::Zeroable)]
pub struct BookingPolicy {
    pub provider: Address,
    pub wallet_quota_window_seconds: PodU32,
    pub wallet_quota: PodU32,
    pub wallet_concurrency: PodU32,
}

#[account]
pub struct WalletBookings {
    pub market: Address,
    pub owner: Address,
    pub rent_payer: Address,
    pub bump: u8,
    pub reserved: [u8; 3],
}

#[repr(C)]
#[derive(Clone, Copy, anchor_lang::bytemuck::Pod, anchor_lang::bytemuck::Zeroable)]
pub struct WalletBookingEntry {
    pub provider_policy: PodU32,
    pub quota_window_end: PodI64,
    pub quota_used: PodU32,
    pub active_bookings: PodU32,
}

#[account]
pub struct ProviderPolicy {
    pub market: Address,
    pub provider: Address,
    pub wallet_quota_window_seconds: PodU32,
    pub wallet_quota: PodU32,
    pub wallet_concurrency: PodU32,
    pub epoch: PodU64,
    pub active_mirrors: PodU32,
    pub updating: u8,
    pub bump: u8,
    pub reserved: [u8; 2],
}

#[account]
pub struct ListRouting {
    pub market: Address,
    pub list: Address,
    pub book_count: PodU32,
    pub bump: u8,
    pub reserved: [u8; 3],
}

#[repr(C)]
#[derive(Clone, Copy, anchor_lang::bytemuck::Pod, anchor_lang::bytemuck::Zeroable)]
pub struct PolicyMirror {
    pub provider: Address,
    pub wallet_quota_window_seconds: PodU32,
    pub wallet_quota: PodU32,
    pub wallet_concurrency: PodU32,
    pub epoch: PodU64,
    pub active: u8,
    pub reserved: [u8; 3],
}

#[account]
pub struct ProviderModelCapacity {
    pub model: Address,
    pub provider: Address,
    pub total: PodU32,
    pub allocated: PodU32,
    pub legacy_active: PodU32,
    pub legacy_slot: PodU32,
    pub bump: u8,
    pub reserved: [u8; 3],
}

#[account]
pub struct ListTerms {
    pub model: Address,
    pub list: Address,
    pub capacity: Address,
    pub limit: PodU32,
    pub next_order: PodU32,
    pub bump: u8,
    pub limits_version: u8,
    pub reserved: [u8; 2],
}

#[repr(C)]
#[derive(Clone, Copy, anchor_lang::bytemuck::Pod, anchor_lang::bytemuck::Zeroable)]
pub struct ListOffer {
    pub terms: TermsEntry,
    pub order: PodU32,
}

#[account]
pub struct ListCapacity {
    pub model: Address,
    pub list: Address,
    pub terms: Address,
    pub limit: PodU32,
    pub bump: u8,
    pub reserved: [u8; 3],
}

#[repr(C)]
#[derive(Clone, Copy, anchor_lang::bytemuck::Pod, anchor_lang::bytemuck::Zeroable)]
pub struct ListCapacityEntry {
    pub provider: Address,
    pub active: PodU32,
}

#[account]
pub struct ProviderWallet {
    pub market: Address,
    pub owner: Address,
    pub rent_payer: Address,
    pub bump: u8,
    pub reserved: [u8; 3],
}

#[repr(C)]
#[derive(Clone, Copy, anchor_lang::bytemuck::Pod, anchor_lang::bytemuck::Zeroable)]
pub struct ProviderUsage {
    pub provider: Address,
    pub quota_window_end: PodI64,
    pub quota_used: PodU32,
    pub active_bookings: PodU32,
}

// prior offer layout remains readable while old reservations drain.
#[account]
pub struct FilteredTerms {
    pub model: Address,
    pub limit: PodU32,
}

#[repr(C)]
#[derive(Clone, Copy, anchor_lang::bytemuck::Pod, anchor_lang::bytemuck::Zeroable)]
pub struct LegacyTermsEntry {
    pub provider: Address,
    pub payout: Address,
    pub checkpoint: [u8; 32],
    pub input_rate: PodU32,
    pub cached_rate: PodU32,
    pub output_rate: PodU32,
    pub speed: PodU16,
    pub privacy: u8,
    pub active: u8,
    pub input_modalities: u8,
    pub output_modalities: u8,
    pub version: u8,
    pub reserved: u8,
    pub capacity: PodU32,
    pub context_window: PodU32,
    pub max_input: PodU32,
    pub max_output: PodU32,
}

#[account]
pub struct ProviderLimitPage {
    pub market: Address,
    pub bump: u8,
    pub page: u8,
    pub reserved: [u8; 2],
}

#[repr(C)]
#[derive(Clone, Copy, anchor_lang::bytemuck::Pod, anchor_lang::bytemuck::Zeroable)]
pub struct ProviderLimitEntry {
    pub concurrency_limit: PodU32,
    pub quota_limit: PodU32,
    pub quota_window_seconds: PodU32,
    pub quota_window_end: PodI64,
    pub quota_used: PodU32,
    pub active_bookings: PodU32,
    pub wallet_concurrency_limit: PodU32,
    pub wallet_quota_limit: PodU32,
    pub wallet_quota_window_seconds: PodU32,
}

#[account]
pub struct ProviderLimitIndex {
    pub market: Address,
    pub provider: Address,
    pub index: PodU32,
    pub bump: u8,
    pub reserved: [u8; 3],
}
