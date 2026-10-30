use anchor_lang::prelude::*;

pub const URI_MAX: usize = 256;
pub const MAX_ENTRIES: usize = 4096;

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
    pub reserved: u8,
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
pub struct Terms {
    pub model: Address,
    pub limit: PodU32,
}

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
    pub reserved: [u8; 32],
    pub input_rate: PodU32,
    pub cached_rate: PodU32,
    pub output_rate: PodU32,
    pub speed: PodU16,
    pub privacy: u8,
    pub active: u8,
    pub reserved2: [u8; 4],
    pub capacity: PodU32,
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
pub struct Booking {
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
    pub reserved: [u8; 8],
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
    pub reserved4: u8,
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
}

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
