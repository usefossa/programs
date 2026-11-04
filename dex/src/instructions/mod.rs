use super::*;
use crate::args::{
    CollectArgs, ConfigInit, CurveInit, GlobalConfig, LiquidityArgs, PoolInit, SwapArgs,
};
use crate::state_types::Pool;

const MINIMUM_LIQUIDITY: u64 = 1_000;
const MODEL_MINIMUM_BASE_ATOMS: u64 =
    (MODEL_CURVE_SUPPLY_ATOMS as u128 * (MODEL_VIRTUAL_USDC_ATOMS - MODEL_RAISE_USDC_ATOMS) as u128
        / (MODEL_VIRTUAL_USDC_ATOMS + MODEL_RAISE_USDC_ATOMS) as u128) as u64;
const STANDARD: u8 = 0;
const REWARD: u8 = 1;
const COMMUNITY: u8 = 2;
pub(crate) const MODEL: u8 = 3;
const CURVE: u8 = 0;
pub(crate) const AMM: u8 = 1;

mod config;
mod fees;
mod launch;
mod liquidity;
mod trading;

pub(crate) use config::{
    initialize_config, initialize_global, refresh_quote, update_config, update_global,
    update_recipients,
};
pub(crate) use fees::{collect, collect_tax};
pub(crate) use launch::{graduate, initialize_curve, initialize_model_curve, initialize_pool};
pub(crate) use liquidity::liquidity;
pub(crate) use trading::swap;

fn action_fee(
    payer: &mut Signer,
    treasury: &mut SystemAccount,
    system: &Program<System>,
) -> Result<()> {
    if payer.address() == treasury.address() {
        return Ok(());
    }
    anchor_lang::system_program::transfer(
        CpiContext::new(
            system.address(),
            anchor_lang::system_program::Transfer {
                from: payer.try_to_cpi_handle_mut()?,
                to: treasury.try_to_cpi_handle_mut()?,
            },
        ),
        ACTION_FEE_LAMPORTS,
    )
}

pub(crate) fn validate_curve(curve: Curve, target: u64, minimum: u64) -> Result<()> {
    check(
        curve.initial > minimum && minimum > 0 && curve.virtual_b > 0 && target > 0,
        crate::DexError::InvalidArgument,
    )?;
    check(
        target <= curve.input(curve.initial, 0, false, sub(curve.initial, minimum)?)?,
        crate::DexError::InvalidArgument,
    )?;
    validate_terminal(curve, curve.initial, 0, target, minimum)
}

fn validate_terminal(curve: Curve, a: u64, b: u64, target: u64, minimum: u64) -> Result<()> {
    let budget = sub(target, b)?;
    let out = if budget == 0 {
        0
    } else {
        curve.out(a, b, false, budget)?
    };
    let remaining = sub(a, out)?;
    check(remaining >= minimum, crate::DexError::InvalidArgument)?;
    let (numerator, denominator) = curve.price(remaining, target)?;
    let matched = mul_div(target, denominator, numerator, true)?;
    check(
        matched <= remaining
            && u128::from(matched) * u128::from(target) >= u128::from(MINIMUM_LIQUIDITY + 1).pow(2),
        crate::DexError::InvalidArgument,
    )
}

fn pool_curve(pool: &Pool) -> Curve {
    Curve {
        initial: pool.initial_base.get(),
        virtual_a: pool.virtual_a.get(),
        virtual_b: pool.virtual_b.get(),
    }
}
