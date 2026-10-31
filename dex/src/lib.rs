#![allow(unexpected_cfgs)]
extern crate alloc;
pub mod args;
pub mod contexts;
mod curve;
mod instructions;
pub mod math;
mod oracle;
mod state;
pub mod state_types;
mod token;
use anchor_lang::prelude::*;
use args::*;
pub use contexts::*;
use curve::Curve;
use math::{add, check, fee, gross, mul_div, sqrt, sub};
use state::{lp, rates};
pub const ID: Address = anchor_lang::address!("dexXxx8t1AZqsAZ3dLdMcfvF3mMeEGBdbEPjAYXy5n9");
pub const ACTION_FEE_LAMPORTS: u64 = 25_000_000;
pub const ORACLE_WINDOW_SECONDS: u64 = 1_800;
pub const PUBLIC_RAISE_USDC_ATOMS: u64 = 8_500_000_000;
pub const PUBLIC_TOKEN_DECIMALS: u8 = 6;
pub const PUBLIC_SUPPLY_ATOMS: u64 = 1_000_000_000_000_000;
pub const PUBLIC_CURVE_SALE_ATOMS: u64 = 793_100_000_000_000;
pub const PUBLIC_LIQUIDITY_ATOMS: u64 = PUBLIC_SUPPLY_ATOMS - PUBLIC_CURVE_SALE_ATOMS;
pub const MODEL_TOKEN_DECIMALS: u8 = 6;
pub const MODEL_SUPPLY_ATOMS: u64 = 100_000_000_000_000;
pub const MODEL_TREASURY_ATOMS: u64 = MODEL_SUPPLY_ATOMS / 10;
pub const MODEL_CURVE_SUPPLY_ATOMS: u64 = MODEL_SUPPLY_ATOMS - MODEL_TREASURY_ATOMS;
pub const MODEL_RAISE_USDC_ATOMS: u64 = 50_000_000_000;
// ceil(target * (1 + sqrt(2))) makes terminal spot price approximately twice opening.
pub const MODEL_VIRTUAL_USDC_ATOMS: u64 = 120_710_678_119;

#[error_code]
pub enum DexError {
    #[msg("invalid arithmetic input")]
    InvalidArithmetic,
    #[msg("invalid account")]
    InvalidAccount,
    #[msg("invalid argument")]
    InvalidArgument,
    #[msg("unauthorized")]
    Unauthorized,
    #[msg("slippage exceeded")]
    Slippage,
    #[msg("liquidity unavailable")]
    Unavailable,
    #[msg("quote price unavailable or stale")]
    StaleQuote,
    #[msg("buy exceeds launch supply limit")]
    BuyLimit,
}

#[program]
pub mod fossa_dex {
    use super::*;
    pub fn initialize_config(
        ctx: &mut Context<InitializeConfig>,
        config: ConfigInit,
    ) -> Result<()> {
        instructions::initialize_config(ctx, config)
    }
    pub fn initialize_pool(ctx: &mut Context<InitializePool>, config: PoolInit) -> Result<()> {
        check(
            ctx.remaining_accounts()?.is_empty(),
            crate::DexError::InvalidArgument,
        )?;
        instructions::initialize_pool(ctx, config)
    }
    pub fn initialize_curve(ctx: &mut Context<InitializeCurve>, config: CurveInit) -> Result<()> {
        check(
            ctx.remaining_accounts()?.is_empty(),
            crate::DexError::InvalidArgument,
        )?;
        instructions::initialize_curve(&mut ctx.accounts.launch, ctx.bumps.launch.authority, config)
    }
    pub fn initialize_model_curve(ctx: &mut Context<InitializeModelCurve>) -> Result<()> {
        check(
            ctx.remaining_accounts()?.is_empty(),
            crate::DexError::InvalidArgument,
        )?;
        instructions::initialize_model_curve(&mut ctx.accounts.launch, ctx.bumps.launch.authority)
    }
    pub fn swap(ctx: &mut Context<Swap>, args: SwapArgs) -> Result<()> {
        check(
            ctx.remaining_accounts()?.is_empty(),
            crate::DexError::InvalidArgument,
        )?;
        instructions::swap(&mut ctx.accounts.trade, args, false)
    }
    pub fn swap_exact_out(ctx: &mut Context<SwapExactOut>, args: SwapArgs) -> Result<()> {
        check(
            ctx.remaining_accounts()?.is_empty(),
            crate::DexError::InvalidArgument,
        )?;
        instructions::swap(&mut ctx.accounts.trade, args, true)
    }
    pub fn graduate(ctx: &mut Context<Graduate>) -> Result<()> {
        instructions::graduate(ctx)
    }
    pub fn add_liquidity(ctx: &mut Context<AddLiquidity>, args: LiquidityArgs) -> Result<()> {
        check(
            ctx.remaining_accounts()?.is_empty(),
            crate::DexError::InvalidArgument,
        )?;
        instructions::liquidity(&mut ctx.accounts.liquidity, args, false)
    }
    pub fn remove_liquidity(ctx: &mut Context<RemoveLiquidity>, args: LiquidityArgs) -> Result<()> {
        check(
            ctx.remaining_accounts()?.is_empty(),
            crate::DexError::InvalidArgument,
        )?;
        instructions::liquidity(&mut ctx.accounts.liquidity, args, true)
    }
    pub fn collect_fees(ctx: &mut Context<CollectFees>, args: CollectArgs) -> Result<()> {
        instructions::collect(ctx, args)
    }
    pub fn collect_transfer_tax(ctx: &mut Context<CollectTransferTax>) -> Result<()> {
        instructions::collect_tax(ctx)
    }
    pub fn update_config(ctx: &mut Context<UpdateConfig>, enabled: bool) -> Result<()> {
        instructions::update_config(ctx, enabled)
    }
    pub fn refresh_quote(ctx: &mut Context<RefreshQuote>) -> Result<()> {
        instructions::refresh_quote(ctx)
    }
    pub fn initialize_global(
        ctx: &mut Context<InitializeGlobal>,
        config: GlobalConfig,
    ) -> Result<()> {
        instructions::initialize_global(ctx, config)
    }
    pub fn update_global(ctx: &mut Context<UpdateGlobal>, config: GlobalConfig) -> Result<()> {
        instructions::update_global(ctx, config)
    }
    pub fn update_recipients(ctx: &mut Context<UpdateRecipients>) -> Result<()> {
        instructions::update_recipients(ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instructions::validate_curve;

    #[test]
    fn reject_unfundable_or_overflowing_graduation() {
        assert!(validate_curve(
            Curve {
                initial: 100_000,
                virtual_a: 0,
                virtual_b: 1
            },
            999,
            100
        )
        .is_err());
        assert!(validate_curve(
            Curve {
                initial: 1_000_000_000,
                virtual_a: 10_000_000_000,
                virtual_b: 1_000_000_000
            },
            80_000_000,
            100_000_000
        )
        .is_err());
        assert!(validate_curve(
            Curve {
                initial: 99_000,
                virtual_a: 0,
                virtual_b: 25_000_000_000
            },
            2_202_970_298,
            10_000
        )
        .is_ok());
    }
}
