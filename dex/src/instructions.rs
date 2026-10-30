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
pub(crate) fn initialize_config(
    ctx: &mut Context<InitializeConfig>,
    input: ConfigInit,
) -> Result<()> {
    check(
        ctx.remaining_accounts()?.is_empty(),
        crate::DexError::InvalidArgument,
    )?;
    let council = &ctx.accounts.council;
    let config = &mut ctx.accounts.config;
    let mut mint = *ctx.accounts.mint.account();
    let mint = &mut mint;
    let token = ctx.accounts.token.account();
    let global = &ctx.accounts.global;

    token::mint(&ctx.accounts.mint, token, Clock::get()?.epoch, true)?;
    rates(
        input.rates.trade_bps,
        input.rates.protocol_share,
        input.rates.creator_share,
    )?;
    check(
        input.rates.trade_bps == 125
            && input.rates.protocol_share == 10_000
            && input.rates.creator_share == 0,
        crate::DexError::InvalidArgument,
    )?;
    check(
        input.treasury != Address::default(),
        crate::DexError::InvalidArgument,
    )?;
    check(
        input.treasury == global.treasury,
        crate::DexError::InvalidArgument,
    )?;
    let distributor = input.distributor.unwrap_or(input.treasury);
    check(
        distributor == global.distributor,
        crate::DexError::InvalidArgument,
    )?;
    config.council = *council.address();
    config.treasury = input.treasury;
    config.quote_mint = *mint.address();
    config.trade_bps = input.rates.trade_bps.into();
    config.protocol_share = input.rates.protocol_share.into();
    config.creator_share = input.rates.creator_share.into();
    config.distributor = distributor;
    config.enabled = 1;
    if mint.address() == &global.usdc_mint {
        check(
            ctx.accounts.reference_pool.is_none() && ctx.accounts.mint.decimals() == 6,
            crate::DexError::InvalidAccount,
        )?;
    } else if let Some(reference) = ctx.accounts.reference_pool.as_ref() {
        check(
            reference.mode == MODEL
                && reference.status == AMM
                && reference.locked_lp.get() > 0
                && reference.mint_a == *mint.address()
                && reference.mint_b == global.usdc_mint,
            crate::DexError::InvalidAccount,
        )?;
        config.reference_pool = *reference.address();
        oracle::checkpoint(config, reference, oracle::timestamp(&Clock::get()?)?)?;
    }
    Ok(())
}

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
    let total = add(
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

pub(crate) fn collect(ctx: &mut Context<CollectFees>, args: CollectArgs) -> Result<()> {
    check(
        ctx.remaining_accounts()?.is_empty(),
        crate::DexError::InvalidArgument,
    )?;
    let owner = ctx.accounts.owner.account();
    let pool = &mut ctx.accounts.pool;
    let authority = ctx.accounts.authority.account();
    let mint_a = &ctx.accounts.mint_a;
    let mint_b = &ctx.accounts.mint_b;
    let destination_a = &mut ctx.accounts.destination_a;
    let destination_b = &mut ctx.accounts.destination_b;
    let vault_a = &mut ctx.accounts.vault_a;
    let vault_b = &mut ctx.accounts.vault_b;
    let token_a = ctx.accounts.token_a.account();
    let token_b = ctx.accounts.token_b.account();

    let mut pool_state = **pool;
    let [mint_a_state, mint_b_state] = pool_state.validate(
        [mint_a, mint_b],
        [vault_a, vault_b],
        [token_a, token_b],
        Clock::get()?.epoch,
    )?;
    let protocol = args.protocol;
    check(
        if protocol {
            pool_state.treasury
        } else {
            pool_state.creator
        } == *owner.address(),
        crate::DexError::Unauthorized,
    )?;
    token::account(destination_a, false)?;
    token::account(destination_b, false)?;
    let (x, y) = if protocol {
        (pool_state.protocol_a.get(), pool_state.protocol_b.get())
    } else {
        (pool_state.creator_a.get(), pool_state.creator_b.get())
    };
    check(x > 0 || y > 0, crate::DexError::InvalidArgument)?;
    let bump = [pool_state.bump];
    let seed: &[&[u8]] = &[b"authority", pool.address().as_ref(), &bump];
    let signers = [seed];
    let nx = if x > 0 {
        token::transfer(
            vault_a,
            mint_a,
            destination_a,
            authority,
            x,
            mint_a_state.decimals,
            &signers,
        )?
    } else {
        0
    };
    let ny = if y > 0 {
        token::transfer(
            vault_b,
            mint_b,
            destination_b,
            authority,
            y,
            mint_b_state.decimals,
            &signers,
        )?
    } else {
        0
    };
    check(
        nx >= args.minimum_a && ny >= args.minimum_b,
        crate::DexError::Slippage,
    )?;
    if protocol {
        pool_state.protocol_a = 0.into();
        pool_state.protocol_b = 0.into();
    } else {
        pool_state.creator_a = 0.into();
        pool_state.creator_b = 0.into();
    }
    **pool = pool_state;
    Ok(())
}

pub(crate) fn collect_tax(ctx: &mut Context<CollectTransferTax>) -> Result<()> {
    let mut extra = ctx.remaining_accounts()?;
    let pool = &mut ctx.accounts.pool;
    let authority = ctx.accounts.authority.account();
    let mint = &mut ctx.accounts.mint;
    let destination = &mut ctx.accounts.destination;
    let token = ctx.accounts.token.account();

    use anchor_lang::prelude::Clock;
    use anchor_lang::solana_program::sysvar::Sysvar as _;
    check(extra.len() <= 16, crate::DexError::InvalidArgument)?;
    let sources = extra.as_mut_slice();
    let pool_state = **pool;
    check(
        matches!(pool_state.mode, REWARD | COMMUNITY),
        crate::DexError::InvalidAccount,
    )?;
    let mint_state = token::mint(mint, token, Clock::get()?.epoch, false)?;
    check(
        mint_state.fee_bps == pool_state.tax_bps.get() && mint_state.fee_cap == u64::MAX,
        crate::DexError::InvalidAccount,
    )?;
    token::account(destination, false)?;
    let before = destination.amount();
    let bump = [pool_state.bump];
    let seed: &[&[u8]] = &[b"authority", pool.address().as_ref(), &bump];
    let signers = [seed];
    use anchor_spl::token_2022_extensions as extensions;
    if !sources.is_empty() {
        for (i, source) in sources.iter().enumerate() {
            check(
                source.address() != destination.address()
                    && source.owner() == &anchor_spl::token_2022::ID
                    && source.is_writable()
                    && source.data_len() >= 165,
                crate::DexError::InvalidAccount,
            )?;
            check(
                InterfaceAccount::<anchor_spl::token_interface::TokenAccount>::load(*source)?
                    .mint()
                    == mint.address(),
                crate::DexError::InvalidAccount,
            )?;
            check(
                sources[..i]
                    .iter()
                    .all(|previous| previous.address() != source.address()),
                crate::DexError::InvalidAccount,
            )?;
        }
        let source_handles = sources.iter_mut().map(CpiHandleMut::writable).collect();
        extensions::withdraw_withheld_tokens_from_accounts(
            CpiContext::new_with_signer(
                token.address(),
                extensions::WithdrawWithheldTokensFromAccounts {
                    mint: mint.to_cpi_handle(),
                    destination: destination.try_to_cpi_handle_mut()?,
                    authority: CpiHandle::readonly(authority),
                },
                &signers,
            ),
            source_handles,
        )?;
    }
    extensions::withdraw_withheld_tokens_from_mint(CpiContext::new_with_signer(
        token.address(),
        extensions::WithdrawWithheldTokensFromMint {
            mint: mint.try_to_cpi_handle_mut()?,
            destination: destination.try_to_cpi_handle_mut()?,
            authority: CpiHandle::readonly(authority),
        },
        &signers,
    ))?;
    let collected = sub(destination.amount(), before)?;
    check(collected > 0, crate::DexError::InvalidArgument)?;
    pool.tax_collected = pool
        .tax_collected
        .get()
        .checked_add(u128::from(collected))
        .ok_or(anchor_lang::prelude::ProgramError::ArithmeticOverflow)?
        .into();
    Ok(())
}

pub(crate) fn initialize_global(
    ctx: &mut Context<InitializeGlobal>,
    config: GlobalConfig,
) -> Result<()> {
    check(
        ctx.remaining_accounts()?.is_empty(),
        crate::DexError::InvalidArgument,
    )?;
    let council = &ctx.accounts.council;
    let global = &mut ctx.accounts.global;
    use anchor_lang::prelude::Address;

    check(
        config.treasury != Address::default()
            && config.distributor != Address::default()
            && config.usdc_mint != Address::default()
            && config.treasury != *ctx.accounts.council.address(),
        crate::DexError::InvalidArgument,
    )?;
    global.council = *council.address();
    global.treasury = config.treasury;
    global.distributor = config.distributor;
    global.usdc_mint = config.usdc_mint;
    Ok(())
}

pub(crate) fn update_global(ctx: &mut Context<UpdateGlobal>, config: GlobalConfig) -> Result<()> {
    check(
        ctx.remaining_accounts()?.is_empty(),
        crate::DexError::InvalidArgument,
    )?;
    check(
        config.treasury != Address::default()
            && config.distributor != Address::default()
            && config.usdc_mint == ctx.accounts.global.usdc_mint
            && config.treasury != *ctx.accounts.new_council.address(),
        crate::DexError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    accounts.global.council = *accounts.new_council.address();
    accounts.global.treasury = config.treasury;
    accounts.global.distributor = config.distributor;
    Ok(())
}

pub(crate) fn update_config(ctx: &mut Context<UpdateConfig>, enabled: bool) -> Result<()> {
    check(
        ctx.remaining_accounts()?.is_empty(),
        crate::DexError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    token::mint(
        &accounts.mint,
        accounts.token.account(),
        Clock::get()?.epoch,
        true,
    )?;
    accounts.config.council = *accounts.council.address();
    accounts.config.quote_mint = *accounts.mint.address();
    accounts.config.treasury = accounts.global.treasury;
    accounts.config.distributor = accounts.global.distributor;
    if accounts.config.enabled == 0 && enabled {
        accounts.config.oracle_timestamp = 0.into();
        accounts.config.quote_per_usdc_x32 = 0.into();
        accounts.config.quote_price_timestamp = 0.into();
        accounts.config.oracle_cumulative = 0.into();
    }
    accounts.config.enabled = u8::from(enabled);
    Ok(())
}

pub(crate) fn refresh_quote(ctx: &mut Context<RefreshQuote>) -> Result<()> {
    check(
        ctx.remaining_accounts()?.is_empty(),
        crate::DexError::InvalidArgument,
    )?;
    check(
        ctx.accounts.config.enabled == 1,
        crate::DexError::Unauthorized,
    )?;
    let now = oracle::timestamp(&Clock::get()?)?;
    oracle::refresh(&mut ctx.accounts.config, &ctx.accounts.reference_pool, now)
}

pub(crate) fn update_recipients(ctx: &mut Context<UpdateRecipients>) -> Result<()> {
    check(
        ctx.remaining_accounts()?.is_empty(),
        crate::DexError::InvalidArgument,
    )?;
    let pool = &mut ctx.accounts.pool;
    let global = &ctx.accounts.global;
    pool.treasury = global.treasury;
    pool.distributor = global.distributor;
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
