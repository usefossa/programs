#![allow(unexpected_cfgs)]
extern crate alloc;
pub mod args;
pub mod contexts;
mod curve;
mod instructions;
pub mod math;
mod state;
pub mod state_types;
mod token;
use anchor_lang::prelude::*;
use args::*;
pub use contexts::*;
use curve::Curve;
use math::{add, check, fee, gross, mul_div, sqrt, sub};
use state::{lp, rates};
pub const ID: Address = Address::new_from_array([9; 32]);

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
        instructions::initialize_pool(&mut ctx.accounts.launch, ctx.bumps.launch.authority, config)
    }
    pub fn initialize_curve(ctx: &mut Context<InitializeCurve>, config: CurveInit) -> Result<()> {
        check(
            ctx.remaining_accounts()?.is_empty(),
            crate::DexError::InvalidArgument,
        )?;
        instructions::initialize_curve(&mut ctx.accounts.launch, ctx.bumps.launch.authority, config)
    }
    pub fn initialize_model_curve(
        ctx: &mut Context<InitializeModelCurve>,
        curve: ModelCurve,
    ) -> Result<()> {
        check(
            ctx.remaining_accounts()?.is_empty(),
            crate::DexError::InvalidArgument,
        )?;
        instructions::initialize_model_curve(
            &mut ctx.accounts.launch,
            ctx.bumps.launch.authority,
            curve,
        )
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
    pub fn create_vesting(ctx: &mut Context<CreateVesting>, args: VestingArgs) -> Result<()> {
        instructions::create_vesting(ctx, args)
    }
    pub fn claim_vesting(ctx: &mut Context<ClaimVesting>, minimum_output: u64) -> Result<()> {
        instructions::claim_vesting(ctx, minimum_output)
    }
    pub fn update_config(ctx: &mut Context<UpdateConfig>, enabled: bool) -> Result<()> {
        instructions::update_config(ctx, enabled)
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
                kind: 0,
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
                kind: 0,
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
                kind: 2,
                initial: 1_000_000_000,
                virtual_a: 1_000_000_000_000_000_000,
                virtual_b: 1_000_000_000_000_000
            },
            1_000_000_000,
            100_000_000
        )
        .is_err());
        assert!(validate_curve(
            Curve {
                kind: 0,
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
