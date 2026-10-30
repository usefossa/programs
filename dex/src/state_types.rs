use anchor_lang::prelude::*;

#[account]
pub struct Global {
    pub council: Address,
    pub treasury: Address,
    pub distributor: Address,
    pub usdc_mint: Address,
    pub reserved: [u8; 8],
}

#[account]
pub struct Config {
    pub council: Address,
    pub treasury: Address,
    pub quote_mint: Address,
    pub trade_bps: PodU16,
    pub protocol_share: PodU16,
    pub creator_share: PodU16,
    pub reserved: [u8; 2],
    pub distributor: Address,
    pub enabled: u8,
    pub reference_pool: Address,
    pub oracle_cumulative: PodU128,
    pub oracle_timestamp: PodU64,
    pub quote_per_usdc_x32: PodU128,
    pub quote_price_timestamp: PodU64,
    pub padding: [u8; 32],
    pub padding2: [u8; 15],
}

#[account]
pub struct Pool {
    pub config: Address,
    pub creator: Address,
    pub treasury: Address,
    pub mint_a: Address,
    pub mint_b: Address,
    pub vault_a: Address,
    pub vault_b: Address,
    pub lp_mint: Address,
    pub reserve_a: PodU64,
    pub reserve_b: PodU64,
    pub protocol_a: PodU64,
    pub protocol_b: PodU64,
    pub creator_a: PodU64,
    pub creator_b: PodU64,
    pub virtual_a: PodU64,
    pub virtual_b: PodU64,
    pub target_b: PodU64,
    pub minimum_a: PodU64,
    pub locked_lp: PodU64,
    pub reserved: [u8; 8],
    pub trade_bps: PodU16,
    pub protocol_share: PodU16,
    pub creator_share: PodU16,
    pub bump: u8,
    pub status: u8,
    pub mode: u8,
    pub tax_bps: PodU16,
    pub reserved2: [u8; 3],
    pub distributor: Address,
    pub tax_collected: PodU128,
    pub initial_base: PodU64,
    pub price_cumulative: PodU128,
    pub price_timestamp: PodU64,
    pub padding: [u8; 32],
    pub padding2: [u8; 24],
}
