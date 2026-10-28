use super::*;
use crate::args::{
    CollectArgs, ConfigInit, CurveInit, GlobalConfig, LiquidityArgs, ModelCurve, PoolInit,
    SwapArgs, VestingArgs,
};
use crate::state_types::Pool;

const MINIMUM_LIQUIDITY: u64 = 1_000;
const STANDARD: u8 = 0;
const REWARD: u8 = 1;
const COMMUNITY: u8 = 2;
const MODEL: u8 = 3;
const CURVE: u8 = 0;
const AMM: u8 = 1;
pub(crate) fn initialize_config(
    ctx: &mut Context<InitializeConfig>,
    input: ConfigInit,
) -> Result<()> {
    check(
        ctx.remaining_accounts()?.is_empty(),
        crate::DexError::InvalidArgument,
    )?;
    let admin = &ctx.accounts.admin;
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
    config.admin = *admin.address();
    config.treasury = input.treasury;
    config.quote_mint = *mint.address();
    config.trade_bps = input.rates.trade_bps.into();
    config.protocol_share = input.rates.protocol_share.into();
    config.creator_share = input.rates.creator_share.into();
    config.distributor = distributor;
    config.enabled = 1;
    Ok(())
}

#[derive(Clone, Copy)]
enum LaunchParameters {
    Pool(PoolInit),
    Curve(CurveInit),
    Model(ModelCurve),
}

pub(crate) fn initialize_pool(
    accounts: &mut Launch,
    authority_bump: u8,
    input: PoolInit,
) -> Result<()> {
    initialize(accounts, authority_bump, LaunchParameters::Pool(input))
}

pub(crate) fn initialize_curve(
    accounts: &mut Launch,
    authority_bump: u8,
    input: CurveInit,
) -> Result<()> {
    initialize(accounts, authority_bump, LaunchParameters::Curve(input))
}

pub(crate) fn initialize_model_curve(
    accounts: &mut Launch,
    authority_bump: u8,
    input: ModelCurve,
) -> Result<()> {
    initialize(accounts, authority_bump, LaunchParameters::Model(input))
}

fn initialize(accounts: &mut Launch, bump: u8, parameters: LaunchParameters) -> Result<()> {
    let curve = !matches!(parameters, LaunchParameters::Pool(_));
    let existing = matches!(parameters, LaunchParameters::Model(_));
    let Launch {
        payer,
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
        system_program: _,
    } = accounts;
    let kind = match parameters {
        LaunchParameters::Curve(input) => input.curve_kind,
        _ => 0,
    };
    check(kind <= 2, crate::DexError::InvalidArgument)?;
    let mode = match parameters {
        LaunchParameters::Model(_) => MODEL,
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
    let epoch = Clock::get()?.epoch;
    let mint_a_state = token::mint(mint_a, token_a.account(), epoch, curve && !existing)?;
    let mint_b_state = token::mint(mint_b, token_b.account(), epoch, true)?;
    check(
        token::account(vault_a, true)? == 0 && token::account(vault_b, true)? == 0,
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
        let supply = if existing {
            let LaunchParameters::Model(input) = parameters else {
                unreachable!()
            };
            token::transfer(
                source_a,
                mint_a,
                vault_a,
                payer.account(),
                input.supply,
                mint_a_state.decimals,
                &[],
            )?
        } else {
            check(mint_a_state.supply == 0, crate::DexError::InvalidArgument)?;
            token::curve_mint(
                mint_a,
                authority.address(),
                tax_bps,
                mode == REWARD || mode == COMMUNITY,
            )?;
            let LaunchParameters::Curve(input) = parameters else {
                unreachable!()
            };
            let supply = input.curve.supply;
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
            supply
        };
        let curve_input = match parameters {
            LaunchParameters::Model(input) => input,
            LaunchParameters::Curve(input) => input.curve,
            LaunchParameters::Pool(_) => unreachable!(),
        };
        let virtual_a = curve_input.virtual_a;
        let virtual_b = curve_input.virtual_b;
        let target = curve_input.target_b;
        let minimum = curve_input.minimum_a;
        check(
            supply > minimum && minimum > 0 && virtual_b > 0 && target > 0,
            crate::DexError::InvalidArgument,
        )?;
        validate_curve(
            Curve {
                kind,
                initial: supply,
                virtual_a,
                virtual_b,
            },
            target,
            minimum,
        )?;
        (supply, 0, virtual_a, virtual_b, target, minimum, 0)
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
        check(total > MINIMUM_LIQUIDITY, crate::DexError::InvalidArgument)?;
        let minted = sub(total, MINIMUM_LIQUIDITY)?;
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
        (x, y, 0, 0, 0, 0, MINIMUM_LIQUIDITY)
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
    pool.curve_kind = kind;
    pool.tax_bps = tax_bps.into();
    pool.distributor = config.distributor;
    pool.initial_base = reserve_a.into();
    Ok(())
}

pub(crate) fn validate_curve(curve: Curve, target: u64, minimum: u64) -> Result<()> {
    check(
        curve.initial > minimum
            && minimum > 0
            && curve.virtual_b > 0
            && target > 0
            && (curve.kind == 0 || curve.virtual_a > 0),
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
        kind: if pool.status == AMM {
            0
        } else {
            pool.curve_kind
        },
        initial: pool.initial_base.get(),
        virtual_a: pool.virtual_a.get(),
        virtual_b: pool.virtual_b.get(),
    }
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
    let [mint_a_state, mint_b_state] = pool_state.validate(
        [&*mint_a, &*mint_b],
        [&*vault_a, &*vault_b],
        [token_a.account(), token_b.account()],
        Clock::get()?.epoch,
    )?;
    token::account(user_a, false)?;
    token::account(user_b, false)?;
    check(
        pool_state.status == AMM
            || (pool_state.status == CURVE && pool_state.reserve_b < pool_state.target_b),
        crate::DexError::Unavailable,
    )?;
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
    let total_bps = 125 + creator_bps;
    let (gross_in, wanted_out) = if exact_out {
        let out = mo.gross(specified)?;
        let effective = curve.input(
            pool_state.reserve_a.get(),
            pool_state.reserve_b.get(),
            a_to_b,
            out,
        )?;
        let mut net = gross(effective, total_bps, u64::MAX)?;
        // separately rounded protocol and creator fees can require one additional native unit.
        for _ in 0..4 {
            if sub(
                net,
                add(fee(net, 125, u64::MAX)?, fee(net, creator_bps, u64::MAX)?)?,
            )? >= effective
            {
                break;
            }
            net = add(net, 1)?;
        }
        let amount = mi.gross(net)?;
        check(amount <= limit, crate::DexError::Slippage)?;
        (amount, out)
    } else {
        (specified, 0)
    };
    let received = token::transfer(source, im, iv, user, gross_in, mi.decimals, &[])?;
    check(
        received == mi.net(gross_in)?,
        crate::DexError::InvalidAccount,
    )?;
    let protocol_fee = fee(received, 125, u64::MAX)?;
    let creator_fee = fee(received, creator_bps, u64::MAX)?;
    let trade_fee = add(protocol_fee, creator_fee)?;
    let effective = sub(received, trade_fee)?;
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
        check(
            sub(pool_state.reserve_a.get(), out)? >= pool_state.minimum_a.get()
                && add(pool_state.reserve_b.get(), effective)? <= pool_state.target_b.get(),
            crate::DexError::Slippage,
        )?;
    }
    let retained = sub(received, add(protocol_fee, creator_fee)?)?;
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
    pool_state.started = 1;
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
    pool_state.validate(
        [mint_a, mint_b],
        [vault_a, vault_b],
        [token_a, token_b],
        Clock::get()?.epoch,
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
    let timestamp = Clock::get()?.unix_timestamp;
    check(timestamp >= 0, crate::DexError::InvalidArgument)?;
    pool_state.graduated_at = (timestamp as u64).into();
    **pool = pool_state;
    Ok(())
}

pub(crate) fn liquidity(accounts: &mut Liquidity, args: LiquidityArgs, remove: bool) -> Result<()> {
    let Liquidity {
        user,
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
    let [mint_a_state, mint_b_state] = pool_state.validate(
        [&*mint_a, &*mint_b],
        [&*vault_a, &*vault_b],
        [token_a.account(), token_b.account()],
        Clock::get()?.epoch,
    )?;
    check(pool_state.status == AMM, crate::DexError::Unavailable)?;
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
        check(
            x > 0 && y > 0 && x < pool_state.reserve_a.get() && y < pool_state.reserve_b.get(),
            crate::DexError::Slippage,
        )?;
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
        // requested LP is exact; both transfer fees are grossed up before max-input checks.
        let x = mul_div(pool_state.reserve_a.get(), amount, total, true)?;
        let y = mul_div(pool_state.reserve_b.get(), amount, total, true)?;
        let gx = mint_a_state.gross(x)?;
        let gy = mint_b_state.gross(y)?;
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
    Ok(())
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

pub(crate) fn create_vesting(ctx: &mut Context<CreateVesting>, args: VestingArgs) -> Result<()> {
    check(
        ctx.remaining_accounts()?.is_empty(),
        crate::DexError::InvalidArgument,
    )?;
    let beneficiary = ctx.accounts.beneficiary.account();
    let pool = &mut ctx.accounts.pool;
    let vesting = &mut ctx.accounts.vesting;

    let mut pool_state = **pool;
    check(
        pool_state.status == CURVE && pool_state.started == 0 && pool_state.mode != MODEL,
        crate::DexError::Unauthorized,
    )?;
    let amount = args.amount;
    let cliff = args.cliff;
    let duration = args.duration;
    check(
        amount > 0
            && cliff <= i64::MAX as u64
            && duration <= i64::MAX as u64
            && add(cliff, duration)? <= i64::MAX as u64,
        crate::DexError::InvalidArgument,
    )?;
    let remaining = sub(pool_state.reserve_a.get(), amount)?;
    validate_curve(
        Curve {
            kind: pool_state.curve_kind,
            initial: remaining,
            virtual_a: pool_state.virtual_a.get(),
            virtual_b: pool_state.virtual_b.get(),
        },
        pool_state.target_b.get(),
        pool_state.minimum_a.get(),
    )?;
    let locked = add(pool_state.vested_base.get(), amount)?;
    vesting.pool = *pool.address();
    vesting.beneficiary = *beneficiary.address();
    vesting.amount = amount.into();
    vesting.cliff = cliff.into();
    vesting.duration = duration.into();
    pool_state.reserve_a = remaining.into();
    pool_state.initial_base = remaining.into();
    pool_state.vested_base = locked.into();
    **pool = pool_state;
    Ok(())
}

pub(crate) fn claim_vesting(ctx: &mut Context<ClaimVesting>, minimum_output: u64) -> Result<()> {
    check(
        ctx.remaining_accounts()?.is_empty(),
        crate::DexError::InvalidArgument,
    )?;
    let pool = &mut ctx.accounts.pool;
    let vesting = &mut ctx.accounts.vesting;
    let authority = ctx.accounts.authority.account();
    let mint = &ctx.accounts.mint;
    let vault = &mut ctx.accounts.vault;
    let destination = &mut ctx.accounts.destination;
    let token = ctx.accounts.token.account();

    let pool_state = **pool;
    check(pool_state.status == AMM, crate::DexError::Unavailable)?;
    let clock = Clock::get()?;
    check(clock.unix_timestamp >= 0, crate::DexError::InvalidArgument)?;
    let start = add(pool_state.graduated_at.get(), vesting.cliff.get())?;
    let now = clock.unix_timestamp as u64;
    check(now >= start, crate::DexError::Unavailable)?;
    let total = vesting.amount.get();
    let duration = vesting.duration.get();
    let vested = if duration == 0 {
        total
    } else {
        mul_div(total, (now - start).min(duration), duration, false)?
    };
    let amount = sub(vested, vesting.vested.get())?;
    check(amount > 0, crate::DexError::InvalidArgument)?;
    let remaining = sub(pool_state.vested_base.get(), amount)?;
    let mint_state = token::mint(mint, token, clock.epoch, false)?;
    token::account(vault, true)?;
    token::account(destination, false)?;
    let bump = [pool_state.bump];
    let seed: &[&[u8]] = &[b"authority", pool.address().as_ref(), &bump];
    let net = token::transfer(
        vault,
        mint,
        destination,
        authority,
        amount,
        mint_state.decimals,
        &[seed],
    )?;
    check(net >= minimum_output, crate::DexError::Slippage)?;
    vesting.vested = vested.into();
    pool.vested_base = remaining.into();
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
    let mut admin = *ctx.accounts.admin.account();
    let admin = &mut admin;
    let global = &mut ctx.accounts.global;
    use anchor_lang::prelude::Address;

    check(
        config.treasury != Address::default() && config.distributor != Address::default(),
        crate::DexError::InvalidArgument,
    )?;
    global.admin = *admin.address();
    global.treasury = config.treasury;
    global.distributor = config.distributor;
    Ok(())
}

pub(crate) fn update_global(ctx: &mut Context<UpdateGlobal>, config: GlobalConfig) -> Result<()> {
    check(
        ctx.remaining_accounts()?.is_empty(),
        crate::DexError::InvalidArgument,
    )?;
    check(
        config.treasury != Address::default() && config.distributor != Address::default(),
        crate::DexError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    accounts.global.admin = *accounts.new_admin.address();
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
    accounts.config.admin = *accounts.admin.address();
    accounts.config.quote_mint = *accounts.mint.address();
    accounts.config.treasury = accounts.global.treasury;
    accounts.config.distributor = accounts.global.distributor;
    accounts.config.enabled = u8::from(enabled);
    Ok(())
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
