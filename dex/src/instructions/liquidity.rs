use super::*;

pub(crate) fn liquidity(accounts: &mut Liquidity, args: LiquidityArgs, remove: bool) -> Result<()> {
    let Liquidity {
        user,
        global: _,
        treasury,
        system_program,
        pool,
        authority,
        token_a,
        token_b,
        token_program: _,
        mint_a,
        mint_b,
        user_a,
        user_b,
        vault_a,
        vault_b,
        lp_mint,
        user_lp,
    } = accounts;
    let mut pool_state = **pool;
    let clock = Clock::get()?;
    let [mint_a_state, mint_b_state] = pool_state.validate(
        [&*mint_a, &*mint_b],
        [&*vault_a, &*vault_b],
        [token_a.account(), token_b.account()],
        clock.epoch,
    )?;
    check(pool_state.status == AMM, crate::DexError::Unavailable)?;
    oracle::accumulate(&mut pool_state, oracle::timestamp(&clock)?)?;
    token::account(user_a, false)?;
    token::account(user_b, false)?;
    let mut total = add(
        lp(lp_mint, authority.address())?,
        pool_state.locked_lp.get(),
    )?;
    let amount = args.amount;
    let bound_a = args.bound_a;
    let bound_b = args.bound_b;
    check(amount > 0, crate::DexError::InvalidArgument)?;
    let bump = [pool_state.bump];
    let seed: &[&[u8]] = &[b"authority", pool.address().as_ref(), &bump];
    let signers = [seed];
    if remove {
        let x = mul_div(pool_state.reserve_a.get(), amount, total, false)?;
        let y = mul_div(pool_state.reserve_b.get(), amount, total, false)?;
        check(x > 0 && y > 0 && amount <= total, crate::DexError::Slippage)?;
        anchor_spl::token::burn(
            CpiContext::new(
                &anchor_spl::token::ID,
                anchor_spl::token::Burn {
                    from: user_lp.try_to_cpi_handle_mut()?,
                    mint: lp_mint.try_to_cpi_handle_mut()?,
                    authority: user.to_cpi_handle(),
                },
            ),
            amount,
        )?;
        let nx = token::transfer(
            vault_a,
            mint_a,
            user_a,
            authority.account(),
            x,
            mint_a_state.decimals,
            &signers,
        )?;
        let ny = token::transfer(
            vault_b,
            mint_b,
            user_b,
            authority.account(),
            y,
            mint_b_state.decimals,
            &signers,
        )?;
        check(nx >= bound_a && ny >= bound_b, crate::DexError::Slippage)?;
        pool_state.reserve_a = sub(pool_state.reserve_a.get(), x)?.into();
        pool_state.reserve_b = sub(pool_state.reserve_b.get(), y)?.into();
    } else {
        if total == 0 && pool_state.reserve_a.get() > 0 && pool_state.reserve_b.get() > 0 {
            // externally burned receipts leave reserves owned by permanent, unminted shares.
            total = sqrt(
                u128::from(pool_state.reserve_a.get()) * u128::from(pool_state.reserve_b.get()),
            );
            pool_state.locked_lp = total.into();
        }
        add(total, amount)?;
        // requested lp stays exact, including an empty direct pool's initial deposits.
        let (x, y, gx, gy) = if total == 0 {
            check(
                pool_state.reserve_a.get() == 0 && pool_state.reserve_b.get() == 0,
                crate::DexError::Unavailable,
            )?;
            let x = mint_a_state.net(bound_a)?;
            let y = mint_b_state.net(bound_b)?;
            check(
                sqrt(u128::from(x) * u128::from(y)) == amount,
                crate::DexError::Slippage,
            )?;
            (x, y, bound_a, bound_b)
        } else {
            let x = mul_div(pool_state.reserve_a.get(), amount, total, true)?;
            let y = mul_div(pool_state.reserve_b.get(), amount, total, true)?;
            (x, y, mint_a_state.gross(x)?, mint_b_state.gross(y)?)
        };
        check(gx <= bound_a && gy <= bound_b, crate::DexError::Slippage)?;
        let nx = token::transfer(
            user_a,
            mint_a,
            vault_a,
            user.account(),
            gx,
            mint_a_state.decimals,
            &[],
        )?;
        let ny = token::transfer(
            user_b,
            mint_b,
            vault_b,
            user.account(),
            gy,
            mint_b_state.decimals,
            &[],
        )?;
        check(
            nx >= x && ny >= y && x > 0 && y > 0,
            crate::DexError::Slippage,
        )?;
        anchor_spl::token::mint_to(
            CpiContext::new_with_signer(
                &anchor_spl::token::ID,
                anchor_spl::token::MintTo {
                    mint: lp_mint.try_to_cpi_handle_mut()?,
                    to: user_lp.try_to_cpi_handle_mut()?,
                    authority: authority.to_cpi_handle(),
                },
                &signers,
            ),
            amount,
        )?;
        pool_state.reserve_a = add(pool_state.reserve_a.get(), nx)?.into();
        pool_state.reserve_b = add(pool_state.reserve_b.get(), ny)?.into();
    }
    **pool = pool_state;
    action_fee(user, treasury, system_program)
}
