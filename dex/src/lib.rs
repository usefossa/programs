#![allow(unexpected_cfgs)]
extern crate alloc;
pub mod abi;
mod contexts;
#[path = "../../src/cross_program.rs"]
mod cross_program;
mod curve;
mod handlers;
pub mod math;
mod schemas;
mod state;
mod token;
#[path = "../../src/wire.rs"]
mod wire;
use abi::*;
use anchor_lang::prelude::*;
use contexts::*;
use curve::Curve;
use math::{add, check, fee, gross, mul_div, sqrt, sub};
use state::{address, distinct, fresh, key, lp, rates, state, writable, Pool};
use token::{read_u16, read_u64};
use wire::{FixedPayload, OptionalTail};
pub const ID: Address = Address::new_from_array([9; 32]);

#[program]
pub mod fossa_dex {
    use super::*;
    #[discrim = 0]
    pub fn initialize_config(
        ctx: &mut Context<InitializeConfig>,
        p: OptionalTail<38, 32>,
    ) -> Result<()> {
        handlers::initialize_config(ctx, &p.0)
    }
    #[discrim = 1]
    pub fn initialize_pool(ctx: &mut Context<InitializePool>, p: FixedPayload<24>) -> Result<()> {
        check(ctx.remaining_accounts()?.is_empty(), 3)?;
        handlers::initialize(
            ctx.program_id,
            handlers::LaunchAccounts {
                payer: *ctx.accounts.payer.account(),
                config: *ctx.accounts.config.account(),
                pool: *ctx.accounts.pool.account(),
                authority: *ctx.accounts.authority.account(),
                ma: *ctx.accounts.ma.account(),
                mb: *ctx.accounts.mb.account(),
                sa: *ctx.accounts.sa.account(),
                sb: *ctx.accounts.sb.account(),
                va: *ctx.accounts.va.account(),
                vb: *ctx.accounts.vb.account(),
                lm: *ctx.accounts.lm.account(),
                ulp: *ctx.accounts.ulp.account(),
                ta: *ctx.accounts.ta.account(),
                tb: *ctx.accounts.tb.account(),
                tlp: *ctx.accounts.tlp.account(),
            },
            &p.0,
            INITIALIZE_POOL,
        )
    }
    #[discrim = 2]
    pub fn initialize_curve(
        ctx: &mut Context<InitializeCurve>,
        p: OptionalTail<43, 1>,
    ) -> Result<()> {
        check(ctx.remaining_accounts()?.is_empty(), 3)?;
        handlers::initialize(
            ctx.program_id,
            handlers::LaunchAccounts {
                payer: *ctx.accounts.payer.account(),
                config: *ctx.accounts.config.account(),
                pool: *ctx.accounts.pool.account(),
                authority: *ctx.accounts.authority.account(),
                ma: *ctx.accounts.ma.account(),
                mb: *ctx.accounts.mb.account(),
                sa: *ctx.accounts.sa.account(),
                sb: *ctx.accounts.sb.account(),
                va: *ctx.accounts.va.account(),
                vb: *ctx.accounts.vb.account(),
                lm: *ctx.accounts.lm.account(),
                ulp: *ctx.accounts.ulp.account(),
                ta: *ctx.accounts.ta.account(),
                tb: *ctx.accounts.tb.account(),
                tlp: *ctx.accounts.tlp.account(),
            },
            &p.0,
            INITIALIZE_CURVE,
        )
    }
    #[discrim = 9]
    pub fn initialize_model_curve(
        ctx: &mut Context<InitializeModelCurve>,
        p: FixedPayload<40>,
    ) -> Result<()> {
        check(ctx.remaining_accounts()?.is_empty(), 3)?;
        handlers::global_admin(
            ctx.program_id,
            ctx.accounts.global.account(),
            ctx.accounts.admin.account(),
        )?;
        handlers::initialize(
            ctx.program_id,
            handlers::LaunchAccounts {
                payer: *ctx.accounts.payer.account(),
                config: *ctx.accounts.config.account(),
                pool: *ctx.accounts.pool.account(),
                authority: *ctx.accounts.authority.account(),
                ma: *ctx.accounts.ma.account(),
                mb: *ctx.accounts.mb.account(),
                sa: *ctx.accounts.sa.account(),
                sb: *ctx.accounts.sb.account(),
                va: *ctx.accounts.va.account(),
                vb: *ctx.accounts.vb.account(),
                lm: *ctx.accounts.lm.account(),
                ulp: *ctx.accounts.ulp.account(),
                ta: *ctx.accounts.ta.account(),
                tb: *ctx.accounts.tb.account(),
                tlp: *ctx.accounts.tlp.account(),
            },
            &p.0,
            INITIALIZE_MODEL_CURVE,
        )
    }
    #[discrim = 3]
    pub fn swap(ctx: &mut Context<Swap>, p: FixedPayload<17>) -> Result<()> {
        check(ctx.remaining_accounts()?.is_empty(), 3)?;
        handlers::swap(
            ctx.program_id,
            handlers::SwapAccounts {
                user: *ctx.accounts.user.account(),
                pool: *ctx.accounts.pool.account(),
                authority: *ctx.accounts.authority.account(),
                ma: *ctx.accounts.ma.account(),
                mb: *ctx.accounts.mb.account(),
                ua: *ctx.accounts.ua.account(),
                ub: *ctx.accounts.ub.account(),
                va: *ctx.accounts.va.account(),
                vb: *ctx.accounts.vb.account(),
                ta: *ctx.accounts.ta.account(),
                tb: *ctx.accounts.tb.account(),
            },
            &p.0,
            false,
        )
    }
    #[discrim = 4]
    pub fn swap_exact_out(ctx: &mut Context<SwapExactOut>, p: FixedPayload<17>) -> Result<()> {
        check(ctx.remaining_accounts()?.is_empty(), 3)?;
        handlers::swap(
            ctx.program_id,
            handlers::SwapAccounts {
                user: *ctx.accounts.user.account(),
                pool: *ctx.accounts.pool.account(),
                authority: *ctx.accounts.authority.account(),
                ma: *ctx.accounts.ma.account(),
                mb: *ctx.accounts.mb.account(),
                ua: *ctx.accounts.ua.account(),
                ub: *ctx.accounts.ub.account(),
                va: *ctx.accounts.va.account(),
                vb: *ctx.accounts.vb.account(),
                ta: *ctx.accounts.ta.account(),
                tb: *ctx.accounts.tb.account(),
            },
            &p.0,
            true,
        )
    }
    #[discrim = 5]
    pub fn graduate(ctx: &mut Context<Graduate>, p: FixedPayload<0>) -> Result<()> {
        handlers::graduate(ctx, &p.0)
    }
    #[discrim = 6]
    pub fn add_liquidity(ctx: &mut Context<AddLiquidity>, p: FixedPayload<24>) -> Result<()> {
        check(ctx.remaining_accounts()?.is_empty(), 3)?;
        handlers::liquidity(
            ctx.program_id,
            handlers::LiquidityAccounts {
                user: *ctx.accounts.user.account(),
                pool: *ctx.accounts.pool.account(),
                authority: *ctx.accounts.authority.account(),
                ma: *ctx.accounts.ma.account(),
                mb: *ctx.accounts.mb.account(),
                ua: *ctx.accounts.ua.account(),
                ub: *ctx.accounts.ub.account(),
                va: *ctx.accounts.va.account(),
                vb: *ctx.accounts.vb.account(),
                lm: *ctx.accounts.lm.account(),
                ulp: *ctx.accounts.ulp.account(),
                ta: *ctx.accounts.ta.account(),
                tb: *ctx.accounts.tb.account(),
                tlp: *ctx.accounts.tlp.account(),
            },
            &p.0,
            false,
        )
    }
    #[discrim = 7]
    pub fn remove_liquidity(ctx: &mut Context<RemoveLiquidity>, p: FixedPayload<24>) -> Result<()> {
        check(ctx.remaining_accounts()?.is_empty(), 3)?;
        handlers::liquidity(
            ctx.program_id,
            handlers::LiquidityAccounts {
                user: *ctx.accounts.user.account(),
                pool: *ctx.accounts.pool.account(),
                authority: *ctx.accounts.authority.account(),
                ma: *ctx.accounts.ma.account(),
                mb: *ctx.accounts.mb.account(),
                ua: *ctx.accounts.ua.account(),
                ub: *ctx.accounts.ub.account(),
                va: *ctx.accounts.va.account(),
                vb: *ctx.accounts.vb.account(),
                lm: *ctx.accounts.lm.account(),
                ulp: *ctx.accounts.ulp.account(),
                ta: *ctx.accounts.ta.account(),
                tb: *ctx.accounts.tb.account(),
                tlp: *ctx.accounts.tlp.account(),
            },
            &p.0,
            true,
        )
    }
    #[discrim = 8]
    pub fn collect_fees(ctx: &mut Context<CollectFees>, p: FixedPayload<17>) -> Result<()> {
        handlers::collect(ctx, &p.0)
    }
    #[discrim = 10]
    pub fn collect_transfer_tax(
        ctx: &mut Context<CollectTransferTax>,
        p: FixedPayload<0>,
    ) -> Result<()> {
        handlers::collect_tax(ctx, &p.0)
    }
    #[discrim = 11]
    pub fn create_vesting(ctx: &mut Context<CreateVesting>, p: FixedPayload<24>) -> Result<()> {
        handlers::create_vesting(ctx, &p.0)
    }
    #[discrim = 12]
    pub fn claim_vesting(ctx: &mut Context<ClaimVesting>, p: FixedPayload<8>) -> Result<()> {
        handlers::claim_vesting(ctx, &p.0)
    }
    #[discrim = 13]
    pub fn update_config(ctx: &mut Context<UpdateConfig>, p: FixedPayload<1>) -> Result<()> {
        handlers::update_config(ctx, &p.0)
    }
    #[discrim = 14]
    pub fn initialize_global(
        ctx: &mut Context<InitializeGlobal>,
        p: FixedPayload<64>,
    ) -> Result<()> {
        handlers::initialize_global(ctx, &p.0)
    }
    #[discrim = 15]
    pub fn update_global(ctx: &mut Context<UpdateGlobal>, p: FixedPayload<64>) -> Result<()> {
        handlers::update_global(ctx, &p.0)
    }
    #[discrim = 16]
    pub fn update_recipients(
        ctx: &mut Context<UpdateRecipients>,
        p: FixedPayload<0>,
    ) -> Result<()> {
        handlers::update_recipients(ctx, &p.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::validate_curve;

    #[test]
    fn malformed_instruction_shapes_fail_without_panics() {
        for op in 0..=u8::MAX {
            for len in [0, 1, 7, 17, 24, 40, 43, 44, 70, 255] {
                let mut input = alloc::vec![0; 16 + len + 32];
                input[8..16].copy_from_slice(&(len as u64).to_le_bytes());
                if len > 0 {
                    input[16] = op;
                }
                input[16 + len..].copy_from_slice(ID.as_ref());
                assert_ne!(
                    unsafe { __anchor_dispatch(input.as_mut_ptr(), input.as_ptr().add(16)) },
                    0
                );
            }
        }
    }
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
