use anchor_lang::prelude::*;

#[derive(Clone, Copy, IdlType, AnchorDeserialize, AnchorSerialize)]
pub struct Rates {
    pub trade_bps: u16,
    pub protocol_share: u16,
    pub creator_share: u16,
}

#[derive(Clone, Copy, IdlType, AnchorDeserialize, AnchorSerialize)]
pub struct ConfigInit {
    pub treasury: Address,
    pub rates: Rates,
    pub distributor: Option<Address>,
}

#[derive(Clone, Copy, IdlType, AnchorDeserialize, AnchorSerialize)]
pub struct PoolInit {
    pub amount_a: u64,
    pub amount_b: u64,
    pub minimum_lp: u64,
}

#[derive(Clone, Copy, IdlType, AnchorDeserialize, AnchorSerialize)]
pub struct ModelCurve {
    pub supply: u64,
    pub virtual_a: u64,
    pub virtual_b: u64,
    pub target_b: u64,
    pub minimum_a: u64,
}

#[derive(Clone, Copy, IdlType, AnchorDeserialize, AnchorSerialize)]
pub struct CurveInit {
    pub curve: ModelCurve,
    pub mode: u8,
    pub tax_bps: u16,
    pub curve_kind: u8,
}

#[derive(Clone, Copy, IdlType, AnchorDeserialize, AnchorSerialize)]
pub struct SwapArgs {
    pub a_to_b: bool,
    pub amount: u64,
    pub limit: u64,
}

#[derive(Clone, Copy, IdlType, AnchorDeserialize, AnchorSerialize)]
pub struct LiquidityArgs {
    pub amount: u64,
    pub bound_a: u64,
    pub bound_b: u64,
}

#[derive(Clone, Copy, IdlType, AnchorDeserialize, AnchorSerialize)]
pub struct CollectArgs {
    pub protocol: bool,
    pub minimum_a: u64,
    pub minimum_b: u64,
}

#[derive(Clone, Copy, IdlType, AnchorDeserialize, AnchorSerialize)]
pub struct VestingArgs {
    pub amount: u64,
    pub cliff: u64,
    pub duration: u64,
}

#[derive(Clone, Copy, IdlType, AnchorDeserialize, AnchorSerialize)]
pub struct GlobalConfig {
    pub treasury: Address,
    pub distributor: Address,
}
