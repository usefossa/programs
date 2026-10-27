use super::*;
pub(crate) struct LaunchAccounts {
    pub payer: AccountView,
    pub config: AccountView,
    pub pool: AccountView,
    pub authority: AccountView,
    pub ma: AccountView,
    pub mb: AccountView,
    pub sa: AccountView,
    pub sb: AccountView,
    pub va: AccountView,
    pub vb: AccountView,
    pub lm: AccountView,
    pub ulp: AccountView,
    pub ta: AccountView,
    pub tb: AccountView,
    pub tlp: AccountView,
}
pub(crate) struct SwapAccounts {
    pub user: AccountView,
    pub pool: AccountView,
    pub authority: AccountView,
    pub ma: AccountView,
    pub mb: AccountView,
    pub ua: AccountView,
    pub ub: AccountView,
    pub va: AccountView,
    pub vb: AccountView,
    pub ta: AccountView,
    pub tb: AccountView,
}
pub(crate) struct LiquidityAccounts {
    pub user: AccountView,
    pub pool: AccountView,
    pub authority: AccountView,
    pub ma: AccountView,
    pub mb: AccountView,
    pub ua: AccountView,
    pub ub: AccountView,
    pub va: AccountView,
    pub vb: AccountView,
    pub lm: AccountView,
    pub ulp: AccountView,
    pub ta: AccountView,
    pub tb: AccountView,
    pub tlp: AccountView,
}

pub(crate) fn initialize_config(ctx: &mut Context<InitializeConfig>, p: &[u8]) -> Result<()> {
    let program = ctx.program_id;
    check(ctx.remaining_accounts()?.is_empty(), 3)?;
    let mut admin = *ctx.accounts.admin.account();
    let admin = &mut admin;
    let mut config = *ctx.accounts.config.account();
    let config = &mut config;
    let mut mint = *ctx.accounts.mint.account();
    let mint = &mut mint;
    let token = ctx.accounts.token.account();
    let mut global = *ctx.accounts.global.account();
    let global = &mut global;

    check(matches!(p.len(), 38 | 70) && admin.is_signer(), 3)?;
    fresh(config, program, CONFIG_LEN)?;
    token::mint(mint, token, Clock::get()?.epoch, true)?;
    rates(read_u16(p, 32), read_u16(p, 34), read_u16(p, 36))?;
    check(
        read_u16(p, 32) == 125 && read_u16(p, 34) == 10_000 && read_u16(p, 36) == 0,
        3,
    )?;
    check(p[..32] != [0; 32], 3)?;
    let gd = global_admin(program, global, admin)?;
    check(p[..32] == gd[40..72], 3)?;
    let mut d = config.try_borrow_mut()?;
    d[..8].copy_from_slice(CONFIG_MAGIC);
    key(&mut d, CONFIG_ADMIN, admin.address());
    d[CONFIG_TREASURY..CONFIG_TREASURY + 32].copy_from_slice(&p[..32]);
    key(&mut d, CONFIG_QUOTE, mint.address());
    d[CONFIG_TRADE_BPS..CONFIG_CREATOR_SHARE + 2].copy_from_slice(&p[32..38]);
    let distributor = if p.len() == 70 { &p[38..70] } else { &p[..32] };
    check(distributor == &gd[72..104], 3)?;
    d[CONFIG_DISTRIBUTOR..CONFIG_DISTRIBUTOR + 32].copy_from_slice(distributor);
    d[CONFIG_ENABLED] = 1;
    Ok(())
}

pub(crate) fn initialize(
    program: &Address,
    mut accounts: LaunchAccounts,
    p: &[u8],
    op: u8,
) -> Result<()> {
    let curve = op != INITIALIZE_POOL;
    let existing = op == INITIALIZE_MODEL_CURVE;
    let LaunchAccounts {
        payer,
        config,
        pool,
        authority,
        ma,
        mb,
        sa,
        sb,
        va,
        vb,
        lm,
        ulp,
        ta,
        tb,
        tlp,
    } = &mut accounts;
    check(
        if existing {
            p.len() == 40
        } else if curve {
            p.len() == 43 || p.len() == 44
        } else {
            p.len() == 24
        },
        3,
    )?;
    let kind = if p.len() == 44 { p[43] } else { 0 };
    check(kind <= 2, 3)?;
    let mode = if existing {
        MODEL
    } else if curve {
        p[40]
    } else {
        STANDARD
    };
    let tax_bps = if curve && !existing {
        read_u16(p, 41)
    } else {
        0
    };
    check(mode <= MODEL && (existing || mode < MODEL), 3)?;
    check(
        if mode == REWARD || mode == COMMUNITY {
            tax_bps == 100 || tax_bps == 300
        } else {
            tax_bps == 0
        },
        3,
    )?;
    check(payer.is_signer(), 4)?;
    fresh(pool, program, POOL_LEN)?;
    writable(&[payer, va, vb, lm, ulp])?;
    distinct(&[config, pool, authority, ma, mb, sa, sb, va, vb, lm, ulp])?;
    check(
        tlp.address() == &anchor_spl::token::ID && tlp.executable(),
        2,
    )?;
    let (expected, bump) =
        Address::derive_program_address(&[b"authority", pool.address().as_ref()], program)
            .ok_or(anchor_lang::prelude::ProgramError::InvalidSeeds)?;
    check(authority.address() == &expected, 2)?;
    let cd = state(config, program, CONFIG_LEN, CONFIG_MAGIC)?;
    check(cd[CONFIG_ENABLED] == 1, 4)?;
    check(address(&cd, CONFIG_QUOTE) == *mb.address(), 2)?;
    let trade_bps = read_u16(&cd, CONFIG_TRADE_BPS);
    let protocol_share = read_u16(&cd, CONFIG_PROTOCOL_SHARE);
    let creator_share = read_u16(&cd, CONFIG_CREATOR_SHARE);
    rates(trade_bps, protocol_share, creator_share)?;
    let epoch = Clock::get()?.epoch;
    let am = token::mint(ma, ta, epoch, curve && !existing)?;
    let bm = token::mint(mb, tb, epoch, true)?;
    check(
        token::account(va, ma, authority.address(), true)? == 0
            && token::account(vb, mb, authority.address(), true)? == 0,
        2,
    )?;
    token::account(sa, ma, payer.address(), false)?;
    token::account(sb, mb, payer.address(), false)?;
    check(lp(lm, authority.address())? == 0, 2)?;
    token::account(ulp, lm, payer.address(), false)?;
    let bump_ref = [bump];
    let signer_seeds: &[&[u8]] = &[b"authority", pool.address().as_ref(), &bump_ref];
    let signers = [signer_seeds];
    let (reserve_a, reserve_b, virtual_a, virtual_b, target_b, minimum_a, locked_lp) = if curve {
        check(
            u32::from(protocol_share) + u32::from(creator_share) == 10_000,
            3,
        )?;
        let supply = if existing {
            token::transfer(sa, ma, va, payer, read_u64(p, 0), am.decimals, &[])?
        } else {
            check(ma.is_writable() && am.supply == 0, 3)?;
            let d = ma.try_borrow()?;
            check(
                d[..4] == [1, 0, 0, 0]
                    && d[4..36] == *authority.address().as_ref()
                    && d[46..50] == [0; 4],
                2,
            )?;
            let mut has_tax = false;
            if d.len() > 82 {
                token::extensions(&d[166..], |kind, e| {
                    if kind == 1 {
                        has_tax = true;
                        check(mode == REWARD || mode == COMMUNITY, 2)?;
                        check(
                            e[..32] == [0; 32]
                                && e[32..64] == *authority.address().as_ref()
                                && e[80..90] == e[98..108]
                                && read_u64(e, 80) == u64::MAX
                                && read_u16(e, 88) == tax_bps,
                            2,
                        )?;
                    }
                    Ok(())
                })?;
            }
            check(has_tax == (mode == REWARD || mode == COMMUNITY), 2)?;
            drop(d);
            let supply = read_u64(p, 0);
            crate::cross_program::mint_to(ma, va, authority, ta.address(), supply, &signers)?;
            crate::cross_program::revoke_mint(ma, authority, ta.address(), &signers)?;
            check(token::balance(va)? == supply, 2)?;
            supply
        };
        let virtual_a = read_u64(p, 8);
        let virtual_b = read_u64(p, 16);
        let target = read_u64(p, 24);
        let minimum = read_u64(p, 32);
        check(
            supply > minimum && minimum > 0 && virtual_b > 0 && target > 0,
            3,
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
        let x = token::transfer(sa, ma, va, payer, read_u64(p, 0), am.decimals, &[])?;
        let y = token::transfer(sb, mb, vb, payer, read_u64(p, 8), bm.decimals, &[])?;
        check(x > 0 && y > 0, 3)?;
        let total = sqrt(u128::from(x) * u128::from(y));
        check(total > MINIMUM_LIQUIDITY, 3)?;
        let minted = sub(total, MINIMUM_LIQUIDITY)?;
        check(minted >= read_u64(p, 16), 5)?;
        crate::cross_program::mint_to(
            lm,
            ulp,
            authority,
            &anchor_spl::token::ID,
            minted,
            &signers,
        )?;
        (x, y, 0, 0, 0, 0, MINIMUM_LIQUIDITY)
    };
    {
        let mut d = pool.try_borrow_mut()?;
        d[..8].copy_from_slice(POOL_MAGIC);
        for (offset, account) in [
            (CONFIG, &*config),
            (CREATOR, &*payer),
            (MINT_A, &*ma),
            (MINT_B, &*mb),
            (VAULT_A, &*va),
            (VAULT_B, &*vb),
            (LP_MINT, &*lm),
        ] {
            key(&mut d, offset, account.address());
        }
        d[TREASURY..TREASURY + 32].copy_from_slice(&cd[CONFIG_TREASURY..CONFIG_TREASURY + 32]);
        d[TRADE_BPS..CREATOR_SHARE + 2]
            .copy_from_slice(&cd[CONFIG_TRADE_BPS..CONFIG_CREATOR_SHARE + 2]);
        d[BUMP] = bump;
        d[MODE] = mode;
        d[CURVE_KIND] = kind;
        state::write_u64(&mut d, INITIAL_BASE, reserve_a);
        d[TAX_BPS..TAX_BPS + 2].copy_from_slice(&tax_bps.to_le_bytes());
        d[DISTRIBUTOR..DISTRIBUTOR + 32]
            .copy_from_slice(&cd[CONFIG_DISTRIBUTOR..CONFIG_DISTRIBUTOR + 32]);
    }
    Pool {
        a: reserve_a,
        b: reserve_b,
        protocol_a: 0,
        protocol_b: 0,
        creator_a: 0,
        creator_b: 0,
        virtual_a,
        virtual_b,
        target_b,
        minimum_a,
        locked_lp,
        bump,
        status: if curve { CURVE } else { AMM },
    }
    .save(pool)
}

pub(crate) fn validate_curve(c: Curve, target: u64, minimum: u64) -> Result<()> {
    check(
        c.initial > minimum
            && minimum > 0
            && c.virtual_b > 0
            && target > 0
            && (c.kind == 0 || c.virtual_a > 0),
        3,
    )?;
    check(
        target <= c.input(c.initial, 0, false, sub(c.initial, minimum)?)?,
        3,
    )?;
    validate_terminal(c, c.initial, 0, target, minimum)
}

fn validate_terminal(c: Curve, a: u64, b: u64, target: u64, minimum: u64) -> Result<()> {
    let budget = sub(target, b)?;
    let out = if budget == 0 {
        0
    } else {
        c.out(a, b, false, budget)?
    };
    let remaining = sub(a, out)?;
    check(remaining >= minimum, 3)?;
    let (numerator, denominator) = c.price(remaining, target)?;
    let matched = mul_div(target, denominator, numerator, true)?;
    check(
        matched <= remaining
            && u128::from(matched) * u128::from(target) >= u128::from(MINIMUM_LIQUIDITY + 1).pow(2),
        3,
    )
}

fn pool_curve(pool: &AccountView, s: &Pool) -> Result<Curve> {
    let d = pool.try_borrow()?;
    Ok(Curve {
        kind: if s.status == AMM { 0 } else { d[CURVE_KIND] },
        initial: read_u64(&d, INITIAL_BASE),
        virtual_a: s.virtual_a,
        virtual_b: s.virtual_b,
    })
}

pub(crate) fn swap(
    program: &Address,
    mut accounts: SwapAccounts,
    p: &[u8],
    exact_out: bool,
) -> Result<()> {
    let SwapAccounts {
        user,
        pool,
        authority,
        ma,
        mb,
        ua,
        ub,
        va,
        vb,
        ta,
        tb,
    } = &mut accounts;
    check(p.len() == 17 && p[0] <= 1 && user.is_signer(), 3)?;
    let mut s = Pool::load(pool, program)?;
    let [am, bm] = s.validate(
        pool,
        program,
        authority,
        [ma, mb],
        [va, vb],
        [ta, tb],
        Clock::get()?.epoch,
    )?;
    distinct(&[ua, ub, va, vb, pool])?;
    token::account(ua, ma, user.address(), false)?;
    token::account(ub, mb, user.address(), false)?;
    check(
        s.status == AMM || (s.status == CURVE && s.b < s.target_b),
        6,
    )?;
    let a_to_b = p[0] == 0;
    let c = pool_curve(pool, &s)?;
    let (source, destination, iv, ov, im, om, mi, mo, real_out) = if a_to_b {
        (ua, ub, va, vb, ma, mb, am, bm, s.b)
    } else {
        (ub, ua, vb, va, mb, ma, bm, am, s.a)
    };
    let specified = read_u64(p, 1);
    let limit = read_u64(p, 9);
    check(specified > 0, 3)?;
    let creator_bps = if s.status == CURVE && pool.try_borrow()?[MODE] == STANDARD {
        50
    } else {
        0
    };
    let total_bps = 125 + creator_bps;
    let (gross_in, wanted_out) = if exact_out {
        let out = mo.gross(specified)?;
        let effective = c.input(s.a, s.b, a_to_b, out)?;
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
        check(amount <= limit, 5)?;
        (amount, out)
    } else {
        (specified, 0)
    };
    let received = token::transfer(source, im, iv, user, gross_in, mi.decimals, &[])?;
    check(received == mi.net(gross_in)?, 2)?;
    let protocol_fee = fee(received, 125, u64::MAX)?;
    let creator_fee = fee(received, creator_bps, u64::MAX)?;
    let trade_fee = add(protocol_fee, creator_fee)?;
    let effective = sub(received, trade_fee)?;
    let out = if exact_out {
        check(c.out(s.a, s.b, a_to_b, effective)? >= wanted_out, 2)?;
        wanted_out
    } else {
        c.out(s.a, s.b, a_to_b, effective)?
    };
    let terminal_dust = !exact_out
        && s.status == CURVE
        && !a_to_b
        && limit == 0
        && add(s.b, effective)? == s.target_b;
    check((out > 0 || terminal_dust) && out < real_out, 5)?;
    if s.status == CURVE && !a_to_b {
        check(
            sub(s.a, out)? >= s.minimum_a && add(s.b, effective)? <= s.target_b,
            5,
        )?;
    }
    let retained = sub(received, add(protocol_fee, creator_fee)?)?;
    let bump = [s.bump];
    let seed: &[&[u8]] = &[b"authority", pool.address().as_ref(), &bump];
    let actual = token::transfer(ov, om, destination, authority, out, mo.decimals, &[seed])?;
    check(actual >= if exact_out { specified } else { limit }, 5)?;
    check(actual == mo.net(out)?, 2)?;
    if a_to_b {
        s.a = add(s.a, retained)?;
        s.b = sub(s.b, out)?;
        s.protocol_a = add(s.protocol_a, protocol_fee)?;
        s.creator_a = add(s.creator_a, creator_fee)?;
    } else {
        s.b = add(s.b, retained)?;
        s.a = sub(s.a, out)?;
        s.protocol_b = add(s.protocol_b, protocol_fee)?;
        s.creator_b = add(s.creator_b, creator_fee)?;
    }
    if s.status == CURVE {
        // rounded cycles must not leave a curve unable to reach a solvent, price-matched graduation.
        validate_terminal(c, s.a, s.b, s.target_b, s.minimum_a)?;
    }
    s.save(pool)?;
    pool.try_borrow_mut()?[STARTED] = 1;
    Ok(())
}

pub(crate) fn graduate(ctx: &mut Context<Graduate>, p: &[u8]) -> Result<()> {
    let program = ctx.program_id;
    check(ctx.remaining_accounts()?.is_empty(), 3)?;
    let mut pool = *ctx.accounts.pool.account();
    let pool = &mut pool;
    let authority = ctx.accounts.authority.account();
    let ma = ctx.accounts.ma.account();
    let mb = ctx.accounts.mb.account();
    let mut va = *ctx.accounts.va.account();
    let va = &mut va;
    let mut vb = *ctx.accounts.vb.account();
    let vb = &mut vb;
    let lm = ctx.accounts.lm.account();
    let ta = ctx.accounts.ta.account();
    let tb = ctx.accounts.tb.account();
    let tlp = ctx.accounts.tlp.account();
    let operator = ctx.accounts.operator.account();

    check(p.is_empty(), 3)?;
    let mut s = Pool::load(pool, program)?;
    check(
        operator.is_signer() && address(&pool.try_borrow()?, OPERATOR) == *operator.address(),
        4,
    )?;
    s.validate(
        pool,
        program,
        authority,
        [ma, mb],
        [va, vb],
        [ta, tb],
        Clock::get()?.epoch,
    )?;
    check(
        s.status == CURVE && s.b >= s.target_b && s.a >= s.minimum_a,
        6,
    )?;
    check(
        tlp.address() == &anchor_spl::token::ID
            && tlp.executable()
            && address(&pool.try_borrow()?, LP_MINT) == *lm.address()
            && lp(lm, authority.address())? == 0,
        2,
    )?;
    let (numerator, denominator) = pool_curve(pool, &s)?.price(s.a, s.b)?;
    let matched = mul_div(s.b, denominator, numerator, true)?;
    check(matched > 0 && matched <= s.a, 6)?;
    let total = sqrt(u128::from(matched) * u128::from(s.b));
    check(total > MINIMUM_LIQUIDITY, 3)?;
    let excess = sub(s.a, matched)?;
    if excess > 0 {
        writable(&[ma])?;
        let bump = [s.bump];
        let seed: &[&[u8]] = &[b"authority", pool.address().as_ref(), &bump];
        crate::cross_program::burn(va, ma, authority, ta.address(), excess, &[seed])?;
    }
    // surplus is burned, all real quote migrates, and native-unit rounding cannot increase spot price.
    s.a = matched;
    s.locked_lp = total;
    s.virtual_a = 0;
    s.virtual_b = 0;
    s.status = AMM;
    s.save(pool)?;
    let timestamp = Clock::get()?.unix_timestamp;
    check(timestamp >= 0, 3)?;
    state::write_u64(&mut pool.try_borrow_mut()?, GRADUATED_AT, timestamp as u64);
    Ok(())
}

pub(crate) fn liquidity(
    program: &Address,
    mut accounts: LiquidityAccounts,
    p: &[u8],
    remove: bool,
) -> Result<()> {
    let LiquidityAccounts {
        user,
        pool,
        authority,
        ma,
        mb,
        ua,
        ub,
        va,
        vb,
        lm,
        ulp,
        ta,
        tb,
        tlp,
    } = &mut accounts;
    check(p.len() == 24 && user.is_signer(), 3)?;
    let mut s = Pool::load(pool, program)?;
    let [am, bm] = s.validate(
        pool,
        program,
        authority,
        [ma, mb],
        [va, vb],
        [ta, tb],
        Clock::get()?.epoch,
    )?;
    check(s.status == AMM, 6)?;
    writable(&[lm, ulp])?;
    distinct(&[pool, ua, ub, va, vb, lm, ulp])?;
    check(
        tlp.address() == &anchor_spl::token::ID
            && tlp.executable()
            && address(&pool.try_borrow()?, LP_MINT) == *lm.address(),
        2,
    )?;
    token::account(ua, ma, user.address(), false)?;
    token::account(ub, mb, user.address(), false)?;
    token::account(ulp, lm, user.address(), false)?;
    let total = add(lp(lm, authority.address())?, s.locked_lp)?;
    let amount = read_u64(p, 0);
    let bound_a = read_u64(p, 8);
    let bound_b = read_u64(p, 16);
    check(amount > 0, 3)?;
    let bump = [s.bump];
    let seed: &[&[u8]] = &[b"authority", pool.address().as_ref(), &bump];
    let signers = [seed];
    if remove {
        let x = mul_div(s.a, amount, total, false)?;
        let y = mul_div(s.b, amount, total, false)?;
        check(x > 0 && y > 0 && x < s.a && y < s.b, 5)?;
        crate::cross_program::burn(ulp, lm, user, &anchor_spl::token::ID, amount, &[])?;
        let nx = token::transfer(va, ma, ua, authority, x, am.decimals, &signers)?;
        let ny = token::transfer(vb, mb, ub, authority, y, bm.decimals, &signers)?;
        check(nx >= bound_a && ny >= bound_b, 5)?;
        s.a = sub(s.a, x)?;
        s.b = sub(s.b, y)?;
    } else {
        // requested LP is exact; both transfer fees are grossed up before max-input checks.
        let x = mul_div(s.a, amount, total, true)?;
        let y = mul_div(s.b, amount, total, true)?;
        let gx = am.gross(x)?;
        let gy = bm.gross(y)?;
        check(gx <= bound_a && gy <= bound_b, 5)?;
        let nx = token::transfer(ua, ma, va, user, gx, am.decimals, &[])?;
        let ny = token::transfer(ub, mb, vb, user, gy, bm.decimals, &[])?;
        check(nx >= x && ny >= y && x > 0 && y > 0, 5)?;
        crate::cross_program::mint_to(
            lm,
            ulp,
            authority,
            &anchor_spl::token::ID,
            amount,
            &signers,
        )?;
        s.a = add(s.a, nx)?;
        s.b = add(s.b, ny)?;
    }
    s.save(pool)
}

pub(crate) fn collect(ctx: &mut Context<CollectFees>, p: &[u8]) -> Result<()> {
    let program = ctx.program_id;
    check(ctx.remaining_accounts()?.is_empty(), 3)?;
    let owner = ctx.accounts.owner.account();
    let mut pool = *ctx.accounts.pool.account();
    let pool = &mut pool;
    let authority = ctx.accounts.authority.account();
    let ma = ctx.accounts.ma.account();
    let mb = ctx.accounts.mb.account();
    let mut ua = *ctx.accounts.ua.account();
    let ua = &mut ua;
    let mut ub = *ctx.accounts.ub.account();
    let ub = &mut ub;
    let mut va = *ctx.accounts.va.account();
    let va = &mut va;
    let mut vb = *ctx.accounts.vb.account();
    let vb = &mut vb;
    let ta = ctx.accounts.ta.account();
    let tb = ctx.accounts.tb.account();

    check(p.len() == 17 && p[0] <= 1 && owner.is_signer(), 3)?;
    let mut s = Pool::load(pool, program)?;
    let [am, bm] = s.validate(
        pool,
        program,
        authority,
        [ma, mb],
        [va, vb],
        [ta, tb],
        Clock::get()?.epoch,
    )?;
    let protocol = p[0] == 0;
    check(
        address(
            &pool.try_borrow()?,
            if protocol { TREASURY } else { CREATOR },
        ) == *owner.address(),
        4,
    )?;
    distinct(&[pool, ua, ub, va, vb])?;
    token::account(ua, ma, owner.address(), false)?;
    token::account(ub, mb, owner.address(), false)?;
    let (x, y) = if protocol {
        (s.protocol_a, s.protocol_b)
    } else {
        (s.creator_a, s.creator_b)
    };
    check(x > 0 || y > 0, 3)?;
    let bump = [s.bump];
    let seed: &[&[u8]] = &[b"authority", pool.address().as_ref(), &bump];
    let signers = [seed];
    let nx = if x > 0 {
        token::transfer(va, ma, ua, authority, x, am.decimals, &signers)?
    } else {
        0
    };
    let ny = if y > 0 {
        token::transfer(vb, mb, ub, authority, y, bm.decimals, &signers)?
    } else {
        0
    };
    check(nx >= read_u64(p, 1) && ny >= read_u64(p, 9), 5)?;
    if protocol {
        s.protocol_a = 0;
        s.protocol_b = 0;
    } else {
        s.creator_a = 0;
        s.creator_b = 0;
    }
    s.save(pool)
}

pub(crate) fn collect_tax(ctx: &mut Context<CollectTransferTax>, p: &[u8]) -> Result<()> {
    let program = ctx.program_id;
    let mut extra = ctx.remaining_accounts()?;
    let mut pool = *ctx.accounts.pool.account();
    let pool = &mut pool;
    let authority = ctx.accounts.authority.account();
    let mut mint = *ctx.accounts.mint.account();
    let mint = &mut mint;
    let mut destination = *ctx.accounts.destination.account();
    let destination = &mut destination;
    let token = ctx.accounts.token.account();

    use anchor_lang::prelude::Clock;
    use anchor_lang::solana_program::sysvar::Sysvar as _;
    check(p.is_empty() && extra.len() <= 16, 3)?;
    let sources = extra.as_mut_slice();
    let (mut mint_view, mut destination_view) = (*mint, *destination);
    let (mint, destination) = (&mut mint_view, &mut destination_view);
    let s = Pool::load(pool, program)?;
    state::authority(pool, authority, program, s.bump)?;
    writable(&[pool, mint, destination])?;
    let d = pool.try_borrow()?;
    check(
        matches!(d[MODE], REWARD | COMMUNITY) && address(&d, MINT_A) == *mint.address(),
        2,
    )?;
    check(
        token.address() == &token::TOKEN_2022 && token.executable(),
        2,
    )?;
    let m = token::mint(mint, token, Clock::get()?.epoch, false)?;
    check(
        m.fee_bps == read_u16(&d, TAX_BPS) && m.fee_cap == u64::MAX,
        2,
    )?;
    token::account(destination, mint, &address(&d, DISTRIBUTOR), false)?;
    drop(d);
    check(
        destination.address() != mint.address() && destination.address() != pool.address(),
        2,
    )?;
    let before = token::balance(destination)?;
    let bump = [s.bump];
    let seed: &[&[u8]] = &[b"authority", pool.address().as_ref(), &bump];
    let signers = [seed];
    use anchor_spl::{
        token_2022::spl_token_2022::extension::transfer_fee::instruction::TransferFeeInstruction,
        token_2022_extensions as extensions,
    };
    if !sources.is_empty() {
        for (i, source) in sources.iter().enumerate() {
            check(
                source.address() != destination.address()
                    && source.owner() == &token::TOKEN_2022
                    && source.is_writable()
                    && source.data_len() >= 165,
                2,
            )?;
            check(source.try_borrow()?[..32] == *mint.address().as_ref(), 2)?;
            check(
                sources[..i]
                    .iter()
                    .all(|previous| previous.address() != source.address()),
                2,
            )?;
        }
        let source_handles = sources.iter_mut().map(CpiHandleMut::writable).collect();
        extensions::withdraw_withheld_tokens_from_accounts(
            CpiContext::new_with_signer(
                token.address(),
                extensions::WithdrawWithheldTokensFromAccounts {
                    mint: CpiHandle::readonly(mint),
                    destination: CpiHandleMut::writable(destination),
                    authority: CpiHandle::readonly(authority),
                },
                &signers,
            ),
            source_handles,
        )?;
    }
    crate::cross_program::invoke_transfer_fee(
        CpiContext::new_with_signer(
            token.address(),
            extensions::WithdrawWithheldTokensFromMint {
                mint: CpiHandleMut::writable(mint),
                destination: CpiHandleMut::writable(destination),
                authority: CpiHandle::readonly(authority),
            },
            &signers,
        ),
        TransferFeeInstruction::WithdrawWithheldTokensFromMint,
    )?;
    let collected = sub(token::balance(destination)?, before)?;
    check(collected > 0, 3)?;
    let mut d = pool.try_borrow_mut()?;
    let total = u128::from_le_bytes(d[TAX_COLLECTED..TAX_COLLECTED + 16].try_into().unwrap());
    let total = total
        .checked_add(u128::from(collected))
        .ok_or(anchor_lang::prelude::ProgramError::ArithmeticOverflow)?;
    d[TAX_COLLECTED..TAX_COLLECTED + 16].copy_from_slice(&total.to_le_bytes());
    Ok(())
}

pub(crate) fn create_vesting(ctx: &mut Context<CreateVesting>, p: &[u8]) -> Result<()> {
    let program = ctx.program_id;
    let extra = ctx.remaining_accounts()?;
    let creator = ctx.accounts.creator.account();
    let mut pool = *ctx.accounts.pool.account();
    let pool = &mut pool;
    let mut vesting = *ctx.accounts.vesting.account();
    let vesting = &mut vesting;
    let beneficiary = ctx.accounts.beneficiary.account();

    check(extra.len() <= 1, 3)?;
    check(p.len() == 24 && creator.is_signer(), 3)?;
    let mut s = Pool::load(pool, program)?;
    writable(&[pool])?;
    fresh(vesting, program, VESTING_LEN)?;
    let d = pool.try_borrow()?;
    let owns_launch = address(&d, CREATOR) == *creator.address();
    if !owns_launch {
        check(extra.len() == 1, 4)?;
        global_admin(program, &extra[0], creator)?;
    }
    check(s.status == CURVE && d[STARTED] == 0 && d[MODE] != MODEL, 4)?;
    let amount = read_u64(p, 0);
    let cliff = read_u64(p, 8);
    let duration = read_u64(p, 16);
    check(
        amount > 0
            && cliff <= i64::MAX as u64
            && duration <= i64::MAX as u64
            && add(cliff, duration)? <= i64::MAX as u64,
        3,
    )?;
    let remaining = sub(s.a, amount)?;
    validate_curve(
        Curve {
            kind: d[CURVE_KIND],
            initial: remaining,
            virtual_a: s.virtual_a,
            virtual_b: s.virtual_b,
        },
        s.target_b,
        s.minimum_a,
    )?;
    let locked = add(read_u64(&d, VESTED_BASE), amount)?;
    drop(d);
    let mut d = vesting.try_borrow_mut()?;
    d[..8].copy_from_slice(VESTING_MAGIC);
    key(&mut d, 8, pool.address());
    key(&mut d, 40, beneficiary.address());
    for (offset, value) in [(72, amount), (80, 0), (88, cliff), (96, duration)] {
        state::write_u64(&mut d, offset, value);
    }
    drop(d);
    s.a = remaining;
    s.save(pool)?;
    let mut d = pool.try_borrow_mut()?;
    state::write_u64(&mut d, INITIAL_BASE, remaining);
    state::write_u64(&mut d, VESTED_BASE, locked);
    Ok(())
}

pub(crate) fn claim_vesting(ctx: &mut Context<ClaimVesting>, p: &[u8]) -> Result<()> {
    let program = ctx.program_id;
    check(ctx.remaining_accounts()?.is_empty(), 3)?;
    let beneficiary = ctx.accounts.beneficiary.account();
    let mut pool = *ctx.accounts.pool.account();
    let pool = &mut pool;
    let mut vesting = *ctx.accounts.vesting.account();
    let vesting = &mut vesting;
    let authority = ctx.accounts.authority.account();
    let mut mint = *ctx.accounts.mint.account();
    let mint = &mut mint;
    let mut vault = *ctx.accounts.vault.account();
    let vault = &mut vault;
    let mut destination = *ctx.accounts.destination.account();
    let destination = &mut destination;
    let token = ctx.accounts.token.account();

    check(p.len() == 8 && beneficiary.is_signer(), 3)?;
    writable(&[pool, vesting, vault, destination])?;
    let s = Pool::load(pool, program)?;
    check(s.status == AMM, 6)?;
    state::authority(pool, authority, program, s.bump)?;
    let d = pool.try_borrow()?;
    check(
        address(&d, MINT_A) == *mint.address() && address(&d, VAULT_A) == *vault.address(),
        2,
    )?;
    let vd = state(vesting, program, VESTING_LEN, VESTING_MAGIC)?;
    check(
        address(&vd, 8) == *pool.address() && address(&vd, 40) == *beneficiary.address(),
        4,
    )?;
    let clock = Clock::get()?;
    check(clock.unix_timestamp >= 0, 3)?;
    let start = add(read_u64(&d, GRADUATED_AT), read_u64(&vd, 88))?;
    let now = clock.unix_timestamp as u64;
    check(now >= start, 6)?;
    let total = read_u64(&vd, 72);
    let duration = read_u64(&vd, 96);
    let vested = if duration == 0 {
        total
    } else {
        mul_div(total, (now - start).min(duration), duration, false)?
    };
    let amount = sub(vested, read_u64(&vd, 80))?;
    check(amount > 0, 3)?;
    let remaining = sub(read_u64(&d, VESTED_BASE), amount)?;
    drop(d);
    drop(vd);
    let m = token::mint(mint, token, clock.epoch, false)?;
    token::account(vault, mint, authority.address(), true)?;
    token::account(destination, mint, beneficiary.address(), false)?;
    let bump = [s.bump];
    let seed: &[&[u8]] = &[b"authority", pool.address().as_ref(), &bump];
    let net = token::transfer(
        vault,
        mint,
        destination,
        authority,
        amount,
        m.decimals,
        &[seed],
    )?;
    check(net >= read_u64(p, 0), 5)?;
    state::write_u64(&mut vesting.try_borrow_mut()?, 80, vested);
    state::write_u64(&mut pool.try_borrow_mut()?, VESTED_BASE, remaining);
    Ok(())
}

pub(crate) fn global_admin<'a>(
    program: &Address,
    global: &'a AccountView,
    admin: &AccountView,
) -> Result<impl core::ops::Deref<Target = [u8]> + 'a> {
    let (expected, _) = Address::derive_program_address(&[b"protocol"], program)
        .ok_or(anchor_lang::prelude::ProgramError::InvalidSeeds)?;
    check(global.address() == &expected, 2)?;
    let d = state(global, program, GLOBAL_LEN, GLOBAL_MAGIC)?;
    check(admin.is_signer() && address(&d, 8) == *admin.address(), 4)?;
    Ok(d)
}

pub(crate) fn initialize_global(ctx: &mut Context<InitializeGlobal>, p: &[u8]) -> Result<()> {
    let program = ctx.program_id;
    check(ctx.remaining_accounts()?.is_empty(), 3)?;
    let mut admin = *ctx.accounts.admin.account();
    let admin = &mut admin;
    let mut global = *ctx.accounts.global.account();
    let global = &mut global;
    let executable = ctx.accounts.executable.account();
    let program_data = ctx.accounts.program_data.account();
    let system = ctx.accounts.system.account();

    use anchor_lang::prelude::{Address, ProgramError};

    check(
        p.len() == 64 && admin.is_signer() && p[..32] != [0; 32] && p[32..] != [0; 32],
        3,
    )?;
    const LOADER: Address = Address::from_str_const("BPFLoaderUpgradeab1e11111111111111111111111");
    check(
        executable.address() == program
            && executable.executable()
            && executable.owner() == &LOADER
            && executable.data_len() == 36
            && program_data.owner() == &LOADER
            && program_data.data_len() >= 45,
        4,
    )?;
    let ed = executable.try_borrow()?;
    let pd = program_data.try_borrow()?;
    check(
        ed[..4] == [2, 0, 0, 0]
            && ed[4..36] == *program_data.address().as_ref()
            && pd[..4] == [3, 0, 0, 0]
            && pd[12] == 1
            && pd[13..45] == *admin.address().as_ref(),
        4,
    )?;
    drop(ed);
    drop(pd);
    let (expected, bump) = Address::derive_program_address(&[b"protocol"], program)
        .ok_or(ProgramError::InvalidSeeds)?;
    check(
        global.address() == &expected
            && global.owner() == &anchor_lang::system_program::ID
            && global.data_len() == 0
            && system.address() == &anchor_lang::system_program::ID
            && system.executable(),
        2,
    )?;
    writable(&[admin, global])?;
    let bump_ref = [bump];
    let seed: &[&[u8]] = &[b"protocol", &bump_ref];
    anchor_lang::create_account_signed(admin, global, GLOBAL_LEN, program, seed)?;
    let mut d = global.try_borrow_mut()?;
    d[..8].copy_from_slice(GLOBAL_MAGIC);
    key(&mut d, 8, admin.address());
    d[40..104].copy_from_slice(p);
    Ok(())
}

pub(crate) fn update_global(ctx: &mut Context<UpdateGlobal>, p: &[u8]) -> Result<()> {
    check(ctx.remaining_accounts()?.is_empty(), 3)?;
    check(p.len() == 64 && p[..32] != [0; 32] && p[32..] != [0; 32], 3)?;
    let accounts = &mut ctx.accounts;
    let expected = Address::derive_program_address(&[b"protocol"], ctx.program_id)
        .ok_or(ProgramError::InvalidSeeds)?
        .0;
    check(
        accounts.global.address() == &expected
            && accounts.global.account().data_len() == GLOBAL_LEN,
        2,
    )?;
    check(accounts.global.admin == *accounts.admin.address(), 4)?;
    accounts.global.admin = *accounts.new_admin.address();
    accounts.global.treasury = Address::new_from_array(p[..32].try_into().unwrap());
    accounts.global.distributor = Address::new_from_array(p[32..].try_into().unwrap());
    Ok(())
}

pub(crate) fn update_config(ctx: &mut Context<UpdateConfig>, p: &[u8]) -> Result<()> {
    check(ctx.remaining_accounts()?.is_empty(), 3)?;
    check(p.len() == 1 && p[0] <= 1, 3)?;
    let accounts = &mut ctx.accounts;
    global_admin(
        ctx.program_id,
        accounts.global.account(),
        accounts.admin.account(),
    )?;
    check(accounts.config.account().data_len() == CONFIG_LEN, 2)?;
    token::mint(
        accounts.mint.account(),
        accounts.token.account(),
        Clock::get()?.epoch,
        true,
    )?;
    accounts.config.admin = *accounts.admin.address();
    accounts.config.quote_mint = *accounts.mint.address();
    accounts.config.treasury = accounts.global.treasury;
    accounts.config.distributor = accounts.global.distributor;
    accounts.config.enabled = p[0];
    Ok(())
}

pub(crate) fn update_recipients(ctx: &mut Context<UpdateRecipients>, p: &[u8]) -> Result<()> {
    let program = ctx.program_id;
    check(ctx.remaining_accounts()?.is_empty(), 3)?;
    let mut admin = *ctx.accounts.admin.account();
    let admin = &mut admin;
    let mut pool = *ctx.accounts.pool.account();
    let pool = &mut pool;
    let mut global = *ctx.accounts.global.account();
    let global = &mut global;

    check(p.is_empty(), 3)?;
    let gd = global_admin(program, global, admin)?;
    state(pool, program, POOL_LEN, POOL_MAGIC)?;
    writable(&[pool])?;
    let mut d = pool.try_borrow_mut()?;
    d[TREASURY..TREASURY + 32].copy_from_slice(&gd[40..72]);
    d[DISTRIBUTOR..DISTRIBUTOR + 32].copy_from_slice(&gd[72..104]);
    Ok(())
}
