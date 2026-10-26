use crate::receipt;
use crate::{
    abi::{
        BOOKING_LEN, BOOK_MAGIC, CAPACITY_LEN, FEE_BOOKING_LEN, FEE_MARKET_LEN, HEADER_LEN,
        LIST_FEE_LEN, RESERVE_MODEL_LEN, TERMS_LEN,
    },
    helpers::{
        add, addr, bps_ceil, dec, err, fee, fee_budget, inc, rd16, rd32, rd64, require, ri64, sub,
        wi64, wr16, wr32, wr64,
    },
    matching::{select, Request},
    state::{book, booked_user, booking, fresh, market, model, slot_offset, user, writable},
    token::{token_owned_by, user_payments, user_transfer, user_vault, vault_owned_by},
    CLAIM_SECONDS,
};
use pinocchio::{
    sysvars::{clock::Clock, Sysvar},
    AccountView, Address, ProgramResult,
};

#[inline(always)]
pub(crate) fn reserve(program: &Address, accounts: &mut [AccountView], p: &[u8]) -> ProgramResult {
    let [m, mo, t, c, u, owner, b, rent_payer, list] = accounts else {
        return Err(err(3));
    };
    require(owner.is_signer(), 4)?;
    writable(&[c, u])?;
    let md = market(m, program)?;
    let fees = md.len() == FEE_MARKET_LEN;
    let booking_len = if fees { FEE_BOOKING_LEN } else { BOOKING_LEN };
    require(b.data_len() == booking_len, 2)?;
    let generation = if p.len() == 107 {
        require(rent_payer.is_signer(), 4)?;
        fresh(b, program, booking_len)?;
        0
    } else {
        receipt::prepare_booking(
            b,
            program,
            owner.address(),
            m.address(),
            u.address(),
            rent_payer.address(),
            p,
        )?
    };
    let p = &p[..107];
    let modl = model(mo, program, m.address())?;
    let ud = user(u, program, owner.address(), m.address())?;
    {
        let mut counter = [0u8; 8];
        counter[..7].copy_from_slice(&ud[113..120]);
        require(
            generation == 0
                || (generation < (1u64 << 56)
                    && generation == add(u64::from_le_bytes(counter), 1)?),
            7,
        )?;
    }
    let mut r = Request::parse(p, rd64(&ud, 72))?;
    require(modl[40..72] == p[..32], 2)?;
    let td = book(t, program, mo, &modl, true)?;
    let cd = book(c, program, mo, &modl, false)?;
    let ld = crate::instructions::list::state(list, program)?;
    require(&ld[8..40] == m.address().as_ref(), 2)?;
    let (fee_bps, protocol_bps) = if fees {
        let at = crate::abi::provider_list_len(rd32(&ld, 72) as usize);
        require(ld.len() == at + LIST_FEE_LEN, 7)?;
        let terms = (rd16(&ld, at), rd16(&ld, at + 2));
        super::fees::validate_terms(terms.0, terms.1)?;
        r.spend = fee_budget(r.spend, terms.0);
        terms
    } else {
        (0, 0)
    };
    let providers = &*ld;
    let mut chosen = select(&td, &cd, providers, &r)?;
    if fees {
        chosen.reserve = add(chosen.reserve, bps_ceil(chosen.reserve, fee_bps))?;
        chosen.score = add(chosen.score, bps_ceil(chosen.score, fee_bps))?;
    }
    let ti = HEADER_LEN + chosen.slot as usize * TERMS_LEN;
    let ci = HEADER_LEN + chosen.slot as usize * CAPACITY_LEN;
    let active = inc(rd32(&cd, ci + 16))?;
    let free = sub(rd64(&ud, 72), chosen.reserve)?;
    // allow generation at the booked speed in addition to startup and settlement time.
    let claim_seconds =
        CLAIM_SECONDS + i64::from(r.max_output.div_ceil(u32::from(rd16(&td, ti + 108).max(1))));
    require(claim_seconds <= 86_400, 3)?;
    let claim_end = Clock::get()?
        .unix_timestamp
        .checked_add(claim_seconds)
        .ok_or(err(1))?;
    let mut bd = b.try_borrow_mut()?;
    bd.fill(0);
    wr64(&mut bd, 152, generation);
    bd[..8].copy_from_slice(BOOK_MAGIC);
    bd[8..40].copy_from_slice(owner.address().as_ref());
    bd[40..72].copy_from_slice(&td[ti..ti + 32]);
    bd[72..104].copy_from_slice(m.address().as_ref());
    wr64(&mut bd, 104, chosen.reserve);
    bd[112..124].copy_from_slice(&td[ti + 96..ti + 108]);
    wr32(&mut bd, 124, r.max_input);
    wr32(&mut bd, 128, r.max_output);
    wi64(&mut bd, 132, claim_end);
    bd[180..182].copy_from_slice(&p[44..46]);
    bd[192..224].copy_from_slice(u.address().as_ref());
    bd[224..256].copy_from_slice(mo.address().as_ref());
    bd[256..288].copy_from_slice(rent_payer.address().as_ref());
    bd[288..320].copy_from_slice(&td[ti + 32..ti + 64]);
    bd[320..352].copy_from_slice(list.address().as_ref());
    wr32(&mut bd, 352, chosen.slot);
    bd[357] = p[54];
    bd[358] = u8::from(chosen.cache);
    wr64(&mut bd, 360, chosen.score);
    if fees {
        bd[368..400].copy_from_slice(&ld[40..72]);
        let reserve = modl.len() == RESERVE_MODEL_LEN && modl[141] == 1;
        bd[400..432].copy_from_slice(if reserve {
            &modl[144..176]
        } else {
            &md[72..104]
        });
        wr16(&mut bd, 432, fee_bps);
        wr16(&mut bd, 434, protocol_bps);
        bd[436] = u8::from(reserve);
    }
    drop(bd);
    drop(ud);
    drop(cd);
    wr64(&mut u.try_borrow_mut()?, 72, free);
    if generation > 0 {
        u.try_borrow_mut()?[113..120].copy_from_slice(&generation.to_le_bytes()[..7]);
    }
    let mut d = c.try_borrow_mut()?;
    wr32(&mut d, ci + 16, active);
    Ok(())
}

#[inline(always)]
pub(crate) fn claim(program: &Address, accounts: &mut [AccountView], p: &[u8]) -> ProgramResult {
    require(accounts.len() == 11 || accounts.len() == 13, 3)?;
    let (core, destinations) = accounts.split_at_mut(11);
    let [m, mo, c, u, b, provider, vault, payout, mint, authority, token] = core else {
        return Err(err(3));
    };
    require(p.len() == 12 || p.len() == 140, 3)?;
    require(token.address() == &pinocchio_token::ID, 3)?;
    require(provider.is_signer(), 4)?;
    writable(&[c, u, b, vault, payout])?;
    let md = market(m, program)?;
    let modl = model(mo, program, m.address())?;
    let cd = book(c, program, mo, &modl, false)?;
    let bd = booking(b, program, m.address(), mo.address())?;
    let fees = bd.len() == FEE_BOOKING_LEN;
    require(destinations.len() == if fees { 2 } else { 0 }, 3)?;
    require(payout.address() != vault.address(), 2)?;
    if fees {
        super::fees::validate_terms(rd16(&bd, 432), rd16(&bd, 434))?;
        require(bd[436] <= 1 && bd[437..440] == [0; 3], 2)?;
        writable(&[&destinations[0], &destinations[1]])?;
        require(
            destinations.iter().all(|d| d.address() != vault.address()),
            2,
        )?;
        token_owned_by(&destinations[0], mint.address(), &addr(&bd, 368))?;
        if bd[436] == 1 {
            require(&bd[400..432] == destinations[1].address().as_ref(), 2)?;
            vault_owned_by(&destinations[1], mint.address(), mo.address())?;
        } else {
            token_owned_by(&destinations[1], mint.address(), &addr(&bd, 400))?;
        }
    }
    let generation = rd64(&bd, 152);
    require(
        if generation == 0 {
            p.len() == 12
        } else {
            p.len() == 140 && generation == rd64(p, 12)
        },
        7,
    )?;
    let ci = slot_offset(&cd, rd32(&bd, 352), false)?;
    require(
        bd[148] == 0
            && bd[140..148] == [0; 8]
            && bd[356] == 0
            && bd[359] == 0
            && &bd[40..72] == provider.address().as_ref()
            && Clock::get()?.unix_timestamp <= ri64(&bd, 132),
        7,
    )?;
    require(&bd[288..320] == payout.address().as_ref(), 2)?;
    let ud = booked_user(u, program, &bd, m.address())?;
    user_vault(vault, mint, &md, u, &ud, program)?;
    token_owned_by(payout, mint.address(), provider.address())?;
    let uncached = rd32(p, 0);
    let cached = rd32(p, 4);
    let output = rd32(p, 8);
    require(
        u64::from(uncached) + u64::from(cached) <= u64::from(rd32(&bd, 124))
            && output <= rd32(&bd, 128)
            && (cached == 0 || bd[358] == 1),
        8,
    )?;
    let provider_charge =
        fee(uncached, rd32(&bd, 112)) + fee(cached, rd32(&bd, 116)) + fee(output, rd32(&bd, 120));
    let list_fee = if fees {
        bps_ceil(provider_charge, rd16(&bd, 432))
    } else {
        0
    };
    let protocol_fee = if fees {
        bps_ceil(list_fee, rd16(&bd, 434))
    } else {
        0
    };
    let creator_fee = list_fee - protocol_fee;
    let charge = add(provider_charge, list_fee)?;
    let reserve = rd64(&bd, 104);
    require(charge <= reserve, 8)?;
    let free = add(rd64(&ud, 72), reserve - charge)?;
    let active = dec(rd32(&cd, ci + 16))?;
    let bump = ud[112];
    drop(ud);
    drop(bd);
    drop(cd);
    if fees {
        user_payments(
            [vault, mint, authority],
            [
                (payout, provider_charge),
                (&destinations[0], creator_fee),
                (&destinations[1], protocol_fee),
            ],
            u.address(),
            bump,
            program,
        )?;
    } else if provider_charge > 0 {
        user_transfer(
            [vault, mint, payout, authority],
            u.address(),
            bump,
            program,
            provider_charge,
        )?;
    }
    let mut d = b.try_borrow_mut()?;
    d[148] = if generation == 0 {
        4
    } else {
        receipt::AVAILABLE
    };
    wr64(&mut d, 160, charge);
    d[168..180].copy_from_slice(&p[..12]);
    drop(d);
    if generation > 0 {
        receipt::settled(b, charge, &p[20..])?;
    }
    wr64(&mut u.try_borrow_mut()?, 72, free);
    let mut d = c.try_borrow_mut()?;
    wr32(&mut d, ci + 16, active);
    Ok(())
}

#[inline(always)]
pub(crate) fn release(
    program: &Address,
    accounts: &mut [AccountView],
    op: u8,
    p: &[u8],
) -> ProgramResult {
    require(p.len() == 8 || p.is_empty(), 3)?;
    require(accounts.len() == if op == 8 { 5 } else { 6 }, 3)?;
    let (core, extra) = accounts.split_at_mut(5);
    let [m, mo, c, u, b] = core else {
        return Err(err(3));
    };
    writable(&[c, u, b])?;
    market(m, program)?;
    let modl = model(mo, program, m.address())?;
    let cd = book(c, program, mo, &modl, false)?;
    let bd = booking(b, program, m.address(), mo.address())?;
    let generation = rd64(&bd, 152);
    require(
        if generation == 0 {
            p.is_empty()
        } else {
            p.len() == 8 && generation == rd64(p, 0)
        },
        7,
    )?;
    let ci = slot_offset(&cd, rd32(&bd, 352), false)?;
    require(bd[148] == 0, 7)?;
    if op == 8 {
        require(Clock::get()?.unix_timestamp > ri64(&bd, 132), 7)?;
    } else {
        require(
            extra[0].is_signer() && &bd[40..72] == extra[0].address().as_ref(),
            7,
        )?;
    }
    let ud = booked_user(u, program, &bd, m.address())?;
    let reserve = rd64(&bd, 104);
    let free = add(rd64(&ud, 72), reserve)?;
    let active = dec(rd32(&cd, ci + 16))?;
    drop(cd);
    drop(ud);
    drop(bd);
    b.try_borrow_mut()?[148] = if generation == 0 {
        3
    } else {
        receipt::AVAILABLE
    };
    wr64(&mut u.try_borrow_mut()?, 72, free);
    let mut d = c.try_borrow_mut()?;
    wr32(&mut d, ci + 16, active);
    Ok(())
}

#[inline(always)]
pub(crate) fn close(program: &Address, accounts: &mut [AccountView], p: &[u8]) -> ProgramResult {
    let [a, recipient] = accounts else {
        return Err(err(3));
    };
    require(p.is_empty() && a.address() != recipient.address(), 3)?;
    writable(&[a, recipient])?;
    require(a.owner() == program, 2)?;
    let d = a.try_borrow()?;
    if (d.len() == BOOKING_LEN || d.len() == FEE_BOOKING_LEN) && &d[..8] == BOOK_MAGIC {
        let terminal = d[148] == 3 || d[148] == 4;
        let terminal = {
            if d[148] == receipt::AVAILABLE {
                require(recipient.is_signer(), 4)?;
            }
            terminal || d[148] == receipt::AVAILABLE
        };
        require(terminal && &d[256..288] == recipient.address().as_ref(), 7)?;
    } else {
        return Err(err(2));
    }
    drop(d);
    let lamports = add(recipient.lamports(), a.lamports())?;
    recipient.set_lamports(lamports);
    a.close()
}
