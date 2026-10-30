use crate::{
    math::{add, check, narrow, sub, Result},
    state_types::{Config, Pool},
    DexError, ORACLE_WINDOW_SECONDS,
};
use anchor_lang::prelude::Clock;

pub fn timestamp(clock: &Clock) -> Result<u64> {
    clock
        .unix_timestamp
        .try_into()
        .map_err(|_| DexError::InvalidArgument.into())
}

pub fn cumulative(pool: &Pool, now: u64) -> Result<u128> {
    check(
        pool.mode == crate::instructions::MODEL
            && pool.status == crate::instructions::AMM
            && pool.locked_lp.get() > 0,
        DexError::StaleQuote,
    )?;
    let elapsed = sub(now, pool.price_timestamp.get())?;
    check(elapsed <= u64::from(u32::MAX), DexError::StaleQuote)?;
    let model_reserve = add(pool.reserve_a.get(), pool.virtual_a.get())?;
    let usdc_reserve = add(pool.reserve_b.get(), pool.virtual_b.get())?;
    check(model_reserve > 0 && usdc_reserve > 0, DexError::StaleQuote)?;
    // model token atoms per usdc atom limits how much short upward spikes can shrink the raise.
    let scaled_model_reserve = u128::from(model_reserve) << 32;
    let denominator = u128::from(usdc_reserve);
    let model_per_usdc_x32 =
        scaled_model_reserve / denominator + u128::from(scaled_model_reserve % denominator != 0);
    Ok(pool
        .price_cumulative
        .get()
        .wrapping_add(model_per_usdc_x32 * u128::from(elapsed)))
}

pub fn accumulate(pool: &mut Pool, now: u64) -> Result<()> {
    if pool.mode == crate::instructions::MODEL && pool.status == crate::instructions::AMM {
        pool.price_cumulative = cumulative(pool, now)?.into();
        pool.price_timestamp = now.into();
    }
    Ok(())
}

pub fn checkpoint(config: &mut Config, reference: &Pool, now: u64) -> Result<()> {
    config.oracle_cumulative = cumulative(reference, now)?.into();
    config.oracle_timestamp = now.into();
    config.quote_per_usdc_x32 = 0.into();
    config.quote_price_timestamp = 0.into();
    Ok(())
}

pub fn refresh(config: &mut Config, reference: &Pool, now: u64) -> Result<()> {
    if config.oracle_timestamp.get() == 0 {
        return checkpoint(config, reference, now);
    }
    let elapsed = sub(now, config.oracle_timestamp.get())?;
    check(elapsed >= ORACLE_WINDOW_SECONDS, DexError::StaleQuote)?;
    if elapsed > ORACLE_WINDOW_SECONDS * 2 {
        return checkpoint(config, reference, now);
    }
    let cumulative = cumulative(reference, now)?;
    let observed = cumulative.wrapping_sub(config.oracle_cumulative.get());
    let elapsed = u128::from(elapsed);
    let average = observed / elapsed + u128::from(observed % elapsed != 0);
    check(average > 0, DexError::StaleQuote)?;
    config.oracle_cumulative = cumulative.into();
    config.oracle_timestamp = now.into();
    config.quote_per_usdc_x32 = average.into();
    config.quote_price_timestamp = now.into();
    Ok(())
}

pub fn target(config: &Config, raise_usdc_atoms: u64, now: u64) -> Result<u64> {
    let model_per_usdc_x32 = config.quote_per_usdc_x32.get();
    check(
        model_per_usdc_x32 > 0
            && sub(now, config.quote_price_timestamp.get())? <= ORACLE_WINDOW_SECONDS,
        DexError::StaleQuote,
    )?;
    let numerator = u128::from(raise_usdc_atoms)
        .checked_mul(model_per_usdc_x32)
        .ok_or(anchor_lang::prelude::ProgramError::ArithmeticOverflow)?;
    let scale = 1u128 << 32;
    narrow(numerator / scale + u128::from(numerator % scale != 0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytemuck::Zeroable;

    fn reference() -> Pool {
        let mut pool = Pool::zeroed();
        pool.mode = crate::instructions::MODEL;
        pool.status = crate::instructions::AMM;
        pool.locked_lp = 1.into();
        pool.reserve_a = 100.into();
        pool.reserve_b = 200.into();
        pool.price_timestamp = 100.into();
        pool
    }

    #[test]
    fn same_timestamp_price_change_cannot_change_completed_observation() {
        let mut pool = reference();
        let mut config = Config::zeroed();
        checkpoint(&mut config, &pool, 100).unwrap();
        assert!(refresh(&mut config, &pool, 1_899).is_err());
        assert_eq!(config.oracle_timestamp.get(), 100);
        accumulate(&mut pool, 1_900).unwrap();
        pool.reserve_b = 1_000_000.into();
        refresh(&mut config, &pool, 1_900).unwrap();
        assert_eq!(config.quote_per_usdc_x32.get(), 1u128 << 31);
        assert_eq!(target(&config, 101, 3_700).unwrap(), 51);
        assert!(target(&config, 101, 3_701).is_err());
    }

    #[test]
    fn brief_upward_spike_has_only_its_time_weight_in_the_raise() {
        let mut pool = reference();
        pool.reserve_b = 100.into();
        let mut config = Config::zeroed();
        checkpoint(&mut config, &pool, 100).unwrap();
        accumulate(&mut pool, 1_899).unwrap();
        pool.reserve_b = 1_000_000.into();
        refresh(&mut config, &pool, 1_900).unwrap();
        let raise = 1_800_000;
        let target = target(&config, raise, 1_900).unwrap();
        assert!(target < raise);
        assert!(raise - target < raise / ORACLE_WINDOW_SECONDS);
    }

    #[test]
    fn expired_observation_restarts_warmup_and_invalidates_cached_quote() {
        let pool = reference();
        let mut config = Config::zeroed();
        checkpoint(&mut config, &pool, 100).unwrap();
        refresh(&mut config, &pool, 1_900).unwrap();
        refresh(&mut config, &pool, 5_501).unwrap();
        assert_eq!(config.oracle_timestamp.get(), 5_501);
        assert_eq!(config.quote_per_usdc_x32.get(), 0);
        assert!(refresh(&mut config, &pool, 7_300).is_err());
        refresh(&mut config, &pool, 7_301).unwrap();
        assert_eq!(config.quote_per_usdc_x32.get(), 1u128 << 31);
    }

    #[test]
    fn accumulator_wrap_preserves_average_and_curve_reference_is_rejected() {
        let mut pool = reference();
        pool.price_cumulative = (u128::MAX - (1u128 << 31)).into();
        let mut config = Config::zeroed();
        checkpoint(&mut config, &pool, 100).unwrap();
        refresh(&mut config, &pool, 1_900).unwrap();
        assert_eq!(config.quote_per_usdc_x32.get(), 1u128 << 31);
        pool.status = 0;
        assert!(cumulative(&pool, 1_900).is_err());
    }

    #[test]
    fn low_native_ratios_and_fractional_averages_round_up() {
        let mut pool = reference();
        pool.reserve_a = 1.into();
        pool.reserve_b = 3_000_000_000.into();
        let mut config = Config::zeroed();
        checkpoint(&mut config, &pool, 100).unwrap();
        refresh(&mut config, &pool, 1_900).unwrap();
        assert_eq!(config.quote_per_usdc_x32.get(), 2);
        assert_eq!(target(&config, 30_000_000_000, 1_900).unwrap(), 14);

        pool.reserve_b = (1u64 << 32).into();
        checkpoint(&mut config, &pool, 100).unwrap();
        accumulate(&mut pool, 1_000).unwrap();
        pool.reserve_a = 2.into();
        refresh(&mut config, &pool, 1_900).unwrap();
        assert_eq!(config.quote_per_usdc_x32.get(), 2);
    }
}
