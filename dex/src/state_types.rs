use anchor_lang::prelude::*;

#[account]
pub struct Global {
    pub admin: Address,
    pub treasury: Address,
    pub distributor: Address,
    pub reserved: [u8; 8],
}

#[account]
pub struct Config {
    pub admin: Address,
    pub treasury: Address,
    pub quote_mint: Address,
    pub trade_bps: PodU16,
    pub protocol_share: PodU16,
    pub creator_share: PodU16,
    pub reserved: [u8; 2],
    pub distributor: Address,
    pub enabled: u8,
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
    pub curve_kind: u8,
    pub tax_bps: PodU16,
    pub started: u8,
    pub reserved2: [u8; 3],
    pub distributor: Address,
    pub tax_collected: PodU128,
    pub initial_base: PodU64,
    pub vested_base: PodU64,
    pub graduated_at: PodU64,
    pub locked_base: PodU64,
    pub padding: [u8; 32],
    pub padding2: [u8; 24],
}

#[account]
pub struct Vesting {
    pub pool: Address,
    pub beneficiary: Address,
    pub amount: PodU64,
    pub vested: PodU64,
    pub cliff: PodU64,
    pub duration: PodU64,
    pub padding: [u8; 32],
    pub padding2: [u8; 8],
}
