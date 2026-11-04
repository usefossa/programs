use super::*;

#[derive(Clone, Copy)]
enum LaunchParameters {
    Pool(PoolInit),
    Curve(CurveInit),
    Model,
}

fn public_curve(target_b: u64) -> Result<Curve> {
    let denominator = PUBLIC_CURVE_SALE_ATOMS - PUBLIC_LIQUIDITY_ATOMS;
    let curve = Curve {
        initial: PUBLIC_SUPPLY_ATOMS,
        // rounding retains at least the intended liquidity inventory and a solvent graduation.
        virtual_a: mul_div(
            PUBLIC_LIQUIDITY_ATOMS,
            PUBLIC_LIQUIDITY_ATOMS,
            denominator,
            false,
        )?,
        virtual_b: mul_div(target_b, PUBLIC_LIQUIDITY_ATOMS, denominator, true)?,
    };
    let sold = curve.out(curve.initial, 0, false, target_b)?;
    // reject quote precision that shifts the intended sale by more than one basis point of supply.
    check(
        sub(PUBLIC_CURVE_SALE_ATOMS, sold)? <= PUBLIC_SUPPLY_ATOMS / 10_000,
        crate::DexError::InvalidArgument,
    )?;
    Ok(curve)
}

pub(crate) fn initialize_pool(ctx: &mut Context<InitializePool>, input: PoolInit) -> Result<()> {
    let launch = &mut ctx.accounts.launch;
    let council = ctx
        .accounts
        .council
        .as_ref()
        .is_some_and(|council| council.address() == &launch.global.council);
    check(
        council
            || (launch.mint_a.address() != &launch.global.usdc_mint
                && launch.mint_b.address() != &launch.global.usdc_mint
                && launch.config.reference_pool != Address::default()),
        crate::DexError::Unauthorized,
    )?;
    initialize(
        launch,
        ctx.bumps.launch.authority,
        LaunchParameters::Pool(input),
    )
}

pub(crate) fn initialize_curve(
    accounts: &mut Launch,
    authority_bump: u8,
    input: CurveInit,
) -> Result<()> {
    initialize(accounts, authority_bump, LaunchParameters::Curve(input))
}

pub(crate) fn initialize_model_curve(accounts: &mut Launch, authority_bump: u8) -> Result<()> {
    initialize(accounts, authority_bump, LaunchParameters::Model)
}

fn initialize(accounts: &mut Launch, bump: u8, parameters: LaunchParameters) -> Result<()> {
    let curve = !matches!(parameters, LaunchParameters::Pool(_));
    let existing = matches!(parameters, LaunchParameters::Model);
    let Launch {
        payer,
        global,
        treasury,
        config,
        token_a,
        token_b,
        token_program: _,
        mint_a,
        mint_b,
        pool,
        authority,
        source_a,
        source_b,
        vault_a,
        vault_b,
        lp_mint,
        user_lp,
        system_program,
    } = accounts;
    check(
        mint_a.address() != mint_b.address(),
        crate::DexError::InvalidArgument,
    )?;
    check(
        !curve
            || if existing {
                mint_b.address() == &global.usdc_mint
            } else {
                mint_b.address() != &global.usdc_mint && config.reference_pool != Address::default()
            },
        crate::DexError::Unauthorized,
    )?;
    let mode = match parameters {
        LaunchParameters::Model => MODEL,
        LaunchParameters::Curve(input) => input.mode,
        LaunchParameters::Pool(_) => STANDARD,
    };
    let tax_bps = match parameters {
        LaunchParameters::Curve(input) => input.tax_bps,
        _ => 0,
    };
    check(
        mode <= MODEL && (existing || mode < MODEL),
        crate::DexError::InvalidArgument,
    )?;
    check(
        if mode == REWARD || mode == COMMUNITY {
            tax_bps == 100 || tax_bps == 300
        } else {
            tax_bps == 0
        },
        crate::DexError::InvalidArgument,
    )?;
    let trade_bps = config.trade_bps.get();
    let protocol_share = config.protocol_share.get();
    let creator_share = config.creator_share.get();
    rates(trade_bps, protocol_share, creator_share)?;
    let clock = Clock::get()?;
    let now = oracle::timestamp(&clock)?;
    let epoch = clock.epoch;
    let mint_a_state = token::mint(mint_a, token_a.account(), epoch, curve && !existing)?;
    let mint_b_state = token::mint(mint_b, token_b.account(), epoch, true)?;
    let preset = match parameters {
        LaunchParameters::Model => {
            check(
                payer.address() == &global.council,
                crate::DexError::Unauthorized,
            )?;
            check(
                source_a.amount() == MODEL_TREASURY_ATOMS,
                crate::DexError::InvalidAccount,
            )?;
            check(
                mint_a_state.decimals == MODEL_TOKEN_DECIMALS
                    && mint_a_state.supply == MODEL_SUPPLY_ATOMS,
                crate::DexError::InvalidArgument,
            )?;
            Some((
                Curve {
                    initial: MODEL_CURVE_SUPPLY_ATOMS,
                    virtual_a: MODEL_CURVE_SUPPLY_ATOMS,
                    virtual_b: MODEL_VIRTUAL_USDC_ATOMS,
                },
                MODEL_RAISE_USDC_ATOMS,
                MODEL_MINIMUM_BASE_ATOMS,
            ))
        }
        LaunchParameters::Curve(_) => {
            check(
                mint_a_state.decimals == PUBLIC_TOKEN_DECIMALS,
                crate::DexError::InvalidArgument,
            )?;
            let target = oracle::target(config, PUBLIC_RAISE_USDC_ATOMS, now)?;
            Some((public_curve(target)?, target, PUBLIC_LIQUIDITY_ATOMS))
        }
        LaunchParameters::Pool(_) => None,
    };
    // both model allocations are minted directly, avoiding taxable bootstrap transfers.
    let initial_vault_a = if existing {
        MODEL_CURVE_SUPPLY_ATOMS
    } else {
        0
    };
    check(
        token::account(vault_a, true)? == initial_vault_a && token::account(vault_b, true)? == 0,
        crate::DexError::InvalidAccount,
    )?;
    token::account(source_a, false)?;
    token::account(source_b, false)?;
    let bump_ref = [bump];
    let signer_seeds: &[&[u8]] = &[b"authority", pool.address().as_ref(), &bump_ref];
    let signers = [signer_seeds];
    let (reserve_a, reserve_b, virtual_a, virtual_b, target_b, minimum_a, locked_lp) = if curve {
        check(
            u32::from(protocol_share) + u32::from(creator_share) == 10_000,
            crate::DexError::InvalidArgument,
        )?;
        let (preset, target, minimum) = preset.ok_or(crate::DexError::InvalidArgument)?;
        validate_curve(preset, target, minimum)?;
        if !existing {
            check(mint_a_state.supply == 0, crate::DexError::InvalidArgument)?;
            token::curve_mint(
                mint_a,
                authority.address(),
                tax_bps,
                mode == REWARD || mode == COMMUNITY,
            )?;
            let supply = PUBLIC_SUPPLY_ATOMS;
            anchor_spl::token_interface::mint_to(
                CpiContext::new_with_signer(
                    token_a.address(),
                    anchor_spl::token_interface::MintTo {
                        mint: mint_a.try_to_cpi_handle_mut()?,
                        to: vault_a.try_to_cpi_handle_mut()?,
                        authority: authority.to_cpi_handle(),
                    },
                    &signers,
                ),
                supply,
            )?;
            anchor_spl::token_interface::set_authority(
                CpiContext::new_with_signer(
                    token_a.address(),
                    anchor_spl::token_interface::SetAuthority {
                        account_or_mint: mint_a.try_to_cpi_handle_mut()?,
                        current_authority: authority.to_cpi_handle(),
                    },
                    &signers,
                ),
                anchor_spl::token_interface::spl_token_2022::instruction::AuthorityType::MintTokens,
                None,
            )?;
            check(vault_a.amount() == supply, crate::DexError::InvalidAccount)?;
        }
        (
            preset.initial,
            0,
            preset.virtual_a,
            preset.virtual_b,
            target,
            minimum,
            0,
        )
    } else {
        let LaunchParameters::Pool(input) = parameters else {
            unreachable!()
        };
        let x = token::transfer(
            source_a,
            mint_a,
            vault_a,
            payer.account(),
            input.amount_a,
            mint_a_state.decimals,
            &[],
        )?;
        let y = token::transfer(
            source_b,
            mint_b,
            vault_b,
            payer.account(),
            input.amount_b,
            mint_b_state.decimals,
            &[],
        )?;
        check(x > 0 && y > 0, crate::DexError::InvalidArgument)?;
        let total = sqrt(u128::from(x) * u128::from(y));
        check(total > 0, crate::DexError::InvalidArgument)?;
        let minted = total;
        check(minted >= input.minimum_lp, crate::DexError::Slippage)?;
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
            minted,
        )?;
        (x, y, 0, 0, 0, 0, 0)
    };
    pool.config = *config.address();
    pool.creator = *payer.address();
    pool.treasury = config.treasury;
    pool.mint_a = *mint_a.address();
    pool.mint_b = *mint_b.address();
    pool.vault_a = *vault_a.address();
    pool.vault_b = *vault_b.address();
    pool.lp_mint = *lp_mint.address();
    pool.reserve_a = reserve_a.into();
    pool.reserve_b = reserve_b.into();
    pool.virtual_a = virtual_a.into();
    pool.virtual_b = virtual_b.into();
    pool.target_b = target_b.into();
    pool.minimum_a = minimum_a.into();
    pool.locked_lp = locked_lp.into();
    pool.trade_bps = trade_bps.into();
    pool.protocol_share = protocol_share.into();
    pool.creator_share = creator_share.into();
    pool.bump = bump;
    pool.status = if curve { CURVE } else { AMM };
    pool.mode = mode;
    pool.tax_bps = tax_bps.into();
    pool.distributor = config.distributor;
    pool.initial_base = reserve_a.into();
    pool.price_timestamp = now.into();
    action_fee(payer, treasury, system_program)
}

pub(crate) fn graduate(ctx: &mut Context<Graduate>) -> Result<()> {
    check(
        ctx.remaining_accounts()?.is_empty(),
        crate::DexError::InvalidArgument,
    )?;
    let pool = &mut ctx.accounts.pool;
    let authority = ctx.accounts.authority.account();
    let mint_a = &mut ctx.accounts.mint_a;
    let mint_b = &ctx.accounts.mint_b;
    let vault_a = &mut ctx.accounts.vault_a;
    let vault_b = &mut ctx.accounts.vault_b;
    let lp_mint = &ctx.accounts.lp_mint;
    let token_a = ctx.accounts.token_a.account();
    let token_b = ctx.accounts.token_b.account();

    let mut pool_state = **pool;
    let clock = Clock::get()?;
    pool_state.validate(
        [mint_a, mint_b],
        [vault_a, vault_b],
        [token_a, token_b],
        clock.epoch,
    )?;
    check(
        pool_state.status == CURVE
            && pool_state.reserve_b.get() >= pool_state.target_b.get()
            && pool_state.reserve_a.get() >= pool_state.minimum_a.get(),
        crate::DexError::Unavailable,
    )?;
    check(
        lp(lp_mint, authority.address())? == 0,
        crate::DexError::InvalidAccount,
    )?;
    let (numerator, denominator) =
        pool_curve(&pool_state).price(pool_state.reserve_a.get(), pool_state.reserve_b.get())?;
    let matched = mul_div(pool_state.reserve_b.get(), denominator, numerator, true)?;
    check(
        matched > 0 && matched <= pool_state.reserve_a.get(),
        crate::DexError::Unavailable,
    )?;
    let total = sqrt(u128::from(matched) * u128::from(pool_state.reserve_b.get()));
    check(total > MINIMUM_LIQUIDITY, crate::DexError::InvalidArgument)?;
    let excess = sub(pool_state.reserve_a.get(), matched)?;
    if excess > 0 {
        let bump = [pool_state.bump];
        let seed: &[&[u8]] = &[b"authority", pool.address().as_ref(), &bump];
        anchor_spl::token_interface::burn(
            CpiContext::new_with_signer(
                token_a.address(),
                anchor_spl::token_interface::Burn {
                    from: vault_a.try_to_cpi_handle_mut()?,
                    mint: mint_a.try_to_cpi_handle_mut()?,
                    authority: authority.to_cpi_handle(),
                },
                &[seed],
            ),
            excess,
        )?;
    }
    // surplus is burned, all real quote migrates, and native-unit rounding cannot increase spot price.
    pool_state.reserve_a = matched.into();
    pool_state.locked_lp = total.into();
    pool_state.virtual_a = 0.into();
    pool_state.virtual_b = 0.into();
    pool_state.status = AMM;
    pool_state.price_cumulative = 0.into();
    pool_state.price_timestamp = oracle::timestamp(&clock)?.into();
    **pool = pool_state;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_preset_retains_liquidity_and_price_matched_graduation() {
        for target in [1, 3] {
            assert!(public_curve(target).is_err());
        }
        for target in [
            1_000,
            10_000,
            1_000_000,
            PUBLIC_RAISE_USDC_ATOMS,
            1_000_000_000_000,
        ] {
            let curve = public_curve(target).unwrap();
            validate_curve(curve, target, PUBLIC_LIQUIDITY_ATOMS).unwrap();
            let sold = curve.out(curve.initial, 0, false, target).unwrap();
            assert!(sold <= PUBLIC_CURVE_SALE_ATOMS);
            let remaining = curve.initial - sold;
            assert!(remaining >= PUBLIC_LIQUIDITY_ATOMS);
            let (price_numerator, price_denominator) = curve.price(remaining, target).unwrap();
            let matched = mul_div(target, price_denominator, price_numerator, true).unwrap();
            assert!(matched <= remaining);
        }
    }

    #[test]
    fn model_preset_doubles_spot_price_with_only_graduation_dust() {
        let curve = Curve {
            initial: MODEL_CURVE_SUPPLY_ATOMS,
            virtual_a: MODEL_CURVE_SUPPLY_ATOMS,
            virtual_b: MODEL_VIRTUAL_USDC_ATOMS,
        };
        validate_curve(curve, MODEL_RAISE_USDC_ATOMS, MODEL_MINIMUM_BASE_ATOMS).unwrap();
        let sold = curve
            .out(curve.initial, 0, false, MODEL_RAISE_USDC_ATOMS)
            .unwrap();
        let remaining = curve.initial - sold;
        assert_eq!(sold, 52_720_779_386_314);
        assert_eq!(remaining, 37_279_220_613_686);
        let (end_numerator, end_denominator) =
            curve.price(remaining, MODEL_RAISE_USDC_ATOMS).unwrap();
        let matched =
            mul_div(MODEL_RAISE_USDC_ATOMS, end_denominator, end_numerator, true).unwrap();
        assert_eq!(remaining - matched, 151);
        let (start_numerator, start_denominator) = curve.price(curve.initial, 0).unwrap();
        let twice_opening = 2 * u128::from(start_numerator) * u128::from(end_denominator);
        let terminal = u128::from(end_numerator) * u128::from(start_denominator);
        assert!(terminal <= twice_opening);
        assert!(twice_opening - terminal < twice_opening / 1_000_000_000);
    }
}
