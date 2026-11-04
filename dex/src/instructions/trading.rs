use super::*;

fn trade_gross(effective: u64, creator_bps: u16) -> math::Result<u64> {
    let mut received = gross(effective, 125 + creator_bps, u64::MAX)?;
    for _ in 0..4 {
        if sub(
            received,
            add(
                fee(received, 125, u64::MAX)?,
                fee(received, creator_bps, u64::MAX)?,
            )?,
        )? >= effective
        {
            return Ok(received);
        }
        received = add(received, 1)?;
    }
    Err(crate::DexError::InvalidArithmetic.into())
}

pub(crate) fn swap(accounts: &mut Trade, args: SwapArgs, exact_out: bool) -> Result<()> {
    let Trade {
        user,
        pool,
        authority,
        token_a,
        token_b,
        mint_a,
        mint_b,
        user_a,
        user_b,
        vault_a,
        vault_b,
    } = accounts;
    let mut pool_state = **pool;
    let clock = Clock::get()?;
    let [mint_a_state, mint_b_state] = pool_state.validate(
        [&*mint_a, &*mint_b],
        [&*vault_a, &*vault_b],
        [token_a.account(), token_b.account()],
        clock.epoch,
    )?;
    token::account(user_a, false)?;
    token::account(user_b, false)?;
    check(
        pool_state.status == AMM
            || (pool_state.status == CURVE && pool_state.reserve_b < pool_state.target_b),
        crate::DexError::Unavailable,
    )?;
    oracle::accumulate(&mut pool_state, oracle::timestamp(&clock)?)?;
    let a_to_b = args.a_to_b;
    let curve = pool_curve(&pool_state);
    let (source, destination, iv, ov, im, om, mi, mo, real_out) = if a_to_b {
        (
            user_a,
            user_b,
            vault_a,
            vault_b,
            &*mint_a,
            &*mint_b,
            mint_a_state,
            mint_b_state,
            pool_state.reserve_b.get(),
        )
    } else {
        (
            user_b,
            user_a,
            vault_b,
            vault_a,
            &*mint_b,
            &*mint_a,
            mint_b_state,
            mint_a_state,
            pool_state.reserve_a.get(),
        )
    };
    let specified = args.amount;
    let limit = args.limit;
    check(specified > 0, crate::DexError::InvalidArgument)?;
    let creator_bps = if pool_state.status == CURVE && pool_state.mode == STANDARD {
        50
    } else {
        0
    };
    let (mut gross_in, wanted_out) = if exact_out {
        let out = mo.gross(specified)?;
        let effective = curve.input(
            pool_state.reserve_a.get(),
            pool_state.reserve_b.get(),
            a_to_b,
            out,
        )?;
        let amount = mi.gross(trade_gross(effective, creator_bps)?)?;
        check(amount <= limit, crate::DexError::Slippage)?;
        (amount, out)
    } else {
        (specified, 0)
    };
    let mut received = mi.net(gross_in)?;
    let mut protocol_fee = fee(received, 125, u64::MAX)?;
    let mut creator_fee = fee(received, creator_bps, u64::MAX)?;
    let mut effective = sub(received, add(protocol_fee, creator_fee)?)?;
    if !exact_out && pool_state.status == CURVE && !a_to_b {
        let remaining = sub(pool_state.target_b.get(), pool_state.reserve_b.get())?;
        if effective > remaining {
            received = trade_gross(remaining, creator_bps)?;
            gross_in = mi.gross(received)?;
            protocol_fee = fee(received, 125, u64::MAX)?;
            creator_fee = fee(received, creator_bps, u64::MAX)?;
            effective = sub(received, add(protocol_fee, creator_fee)?)?;
        }
    }
    let out = if exact_out {
        check(
            curve.out(
                pool_state.reserve_a.get(),
                pool_state.reserve_b.get(),
                a_to_b,
                effective,
            )? >= wanted_out,
            crate::DexError::InvalidAccount,
        )?;
        wanted_out
    } else {
        curve.out(
            pool_state.reserve_a.get(),
            pool_state.reserve_b.get(),
            a_to_b,
            effective,
        )?
    };
    let terminal_dust = !exact_out
        && pool_state.status == CURVE
        && !a_to_b
        && limit == 0
        && add(pool_state.reserve_b.get(), effective)? == pool_state.target_b.get();
    check(
        (out > 0 || terminal_dust) && out < real_out,
        crate::DexError::Slippage,
    )?;
    if pool_state.status == CURVE && !a_to_b {
        let launch_supply = if pool_state.mode == MODEL {
            MODEL_SUPPLY_ATOMS
        } else {
            pool_state.initial_base.get()
        };
        check(
            out <= mul_div(launch_supply, 3, 4, false)?,
            crate::DexError::BuyLimit,
        )?;
        check(
            sub(pool_state.reserve_a.get(), out)? >= pool_state.minimum_a.get()
                && add(pool_state.reserve_b.get(), effective)? <= pool_state.target_b.get(),
            crate::DexError::Slippage,
        )?;
    }
    check(
        token::transfer(source, im, iv, user, gross_in, mi.decimals, &[])? == received,
        crate::DexError::InvalidAccount,
    )?;
    let retained = effective;
    let bump = [pool_state.bump];
    let seed: &[&[u8]] = &[b"authority", pool.address().as_ref(), &bump];
    let actual = token::transfer(ov, om, destination, authority, out, mo.decimals, &[seed])?;
    check(
        actual >= if exact_out { specified } else { limit },
        crate::DexError::Slippage,
    )?;
    check(actual == mo.net(out)?, crate::DexError::InvalidAccount)?;
    if a_to_b {
        pool_state.reserve_a = add(pool_state.reserve_a.get(), retained)?.into();
        pool_state.reserve_b = sub(pool_state.reserve_b.get(), out)?.into();
        pool_state.protocol_a = add(pool_state.protocol_a.get(), protocol_fee)?.into();
        pool_state.creator_a = add(pool_state.creator_a.get(), creator_fee)?.into();
    } else {
        pool_state.reserve_b = add(pool_state.reserve_b.get(), retained)?.into();
        pool_state.reserve_a = sub(pool_state.reserve_a.get(), out)?.into();
        pool_state.protocol_b = add(pool_state.protocol_b.get(), protocol_fee)?.into();
        pool_state.creator_b = add(pool_state.creator_b.get(), creator_fee)?.into();
    }
    if pool_state.status == CURVE {
        // rounded cycles must not leave a curve unable to reach a solvent, price-matched graduation.
        validate_terminal(
            curve,
            pool_state.reserve_a.get(),
            pool_state.reserve_b.get(),
            pool_state.target_b.get(),
            pool_state.minimum_a.get(),
        )?;
    }
    **pool = pool_state;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_input_preserves_exact_net_with_both_trade_fees_and_transfer_tax() {
        for creator_bps in [0, 50] {
            for effective in [1, 2, 39, 40, 79, 80, 399, 400, 999, 10_000, 1_000_000] {
                let received = trade_gross(effective, creator_bps).unwrap();
                assert_eq!(
                    received
                        - fee(received, 125, u64::MAX).unwrap()
                        - fee(received, creator_bps, u64::MAX).unwrap(),
                    effective
                );
                for fee_bps in [0, 100, 300, 9999] {
                    let mint = token::Mint {
                        fee_bps,
                        fee_cap: 1_000,
                        ..token::Mint::default()
                    };
                    assert_eq!(mint.net(mint.gross(received).unwrap()).unwrap(), received);
                }
            }
        }
    }
}
