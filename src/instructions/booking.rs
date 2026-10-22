#[cfg(feature = "receipt-prototype")]
use crate::receipt;
use crate::{
    abi::{BOOKING_LEN, BOOK_MAGIC, CAPACITY_LEN, HEADER_LEN, TERMS_LEN, UNBOND_LEN, UNBOND_MAGIC},
    helpers::{add, dec, err, fee, inc, rd16, rd32, rd64, require, ri64, sub, wi64, wr32, wr64},
    matching::{select, Request},
    state::{book, booked_user, booking, fresh, market, model, slot_offset, user, writable},
    token::{token_owned_by, user_transfer, user_vault},
    CLAIM_SECONDS, DISPUTE_SECONDS,
};
use pinocchio::{
    sysvars::{clock::Clock, Sysvar},
    AccountView, Address, ProgramResult,
};

#[inline(always)]
pub(crate) fn reserve(program: &Address, accounts: &mut [AccountView], p: &[u8]) -> ProgramResult {
    let [m, mo, t, c, u, owner, b, rent_payer] = accounts else {
        return Err(err(3));
    };
    require(owner.is_signer(), 4)?;
    writable(&[c, u])?;
    #[cfg(not(feature = "receipt-prototype"))]
    {
        require(rent_payer.is_signer(), 4)?;
        fresh(b, program, BOOKING_LEN)?;
    }
    #[cfg(feature = "receipt-prototype")]
    let generation = if p.len() == 107 {
        require(rent_payer.is_signer(), 4)?;
        fresh(b, program, BOOKING_LEN)?;
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
    #[cfg(feature = "receipt-prototype")]
    let p = &p[..107];
    market(m, program)?;
    let modl = model(mo, program, m.address())?;
    let ud = user(u, program, owner.address(), m.address())?;
    #[cfg(feature = "receipt-prototype")]
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
    let r = Request::parse(p, rd64(&ud, 72))?;
    require(modl[40..72] == p[..32], 2)?;
    let td = book(t, program, mo, &modl, true)?;
    let cd = book(c, program, mo, &modl, false)?;
    let chosen = select(&td, &cd, &r)?;
    let ti = HEADER_LEN + chosen.slot as usize * TERMS_LEN;
    let ci = HEADER_LEN + chosen.slot as usize * CAPACITY_LEN;
    let active = inc(rd32(&cd, ci + 16))?;
    let obligations = inc(rd32(&cd, ci + 20))?;
    let free = sub(rd64(&ud, 72), chosen.reserve)?;
    // allow generation at the booked speed in addition to startup and settlement time.
    let claim_seconds =
        CLAIM_SECONDS + i64::from(r.max_output.div_ceil(u32::from(rd16(&td, ti + 108).max(1))));
    require(claim_seconds <= 86_400, 3)?;
    let claim_end = Clock::get()?
        .unix_timestamp
        .checked_add(claim_seconds)
        .ok_or(err(1))?;
    let dispute_end = claim_end.checked_add(DISPUTE_SECONDS).ok_or(err(1))?;
    let mut bd = b.try_borrow_mut()?;
    #[cfg(feature = "receipt-prototype")]
    {
        bd.fill(0);
        wr64(&mut bd, 152, generation);
    }
    bd[..8].copy_from_slice(BOOK_MAGIC);
    bd[8..40].copy_from_slice(owner.address().as_ref());
    bd[40..72].copy_from_slice(&td[ti..ti + 32]);
    bd[72..104].copy_from_slice(m.address().as_ref());
    wr64(&mut bd, 104, chosen.reserve);
    bd[112..124].copy_from_slice(&td[ti + 96..ti + 108]);
    wr32(&mut bd, 124, r.max_input);
    wr32(&mut bd, 128, r.max_output);
    wi64(&mut bd, 132, claim_end);
    wi64(&mut bd, 140, dispute_end);
    bd[180..182].copy_from_slice(&p[44..46]);
    bd[192..224].copy_from_slice(u.address().as_ref());
    bd[224..256].copy_from_slice(mo.address().as_ref());
    bd[256..288].copy_from_slice(rent_payer.address().as_ref());
    bd[288..320].copy_from_slice(&td[ti + 32..ti + 64]);
    bd[320..352].copy_from_slice(&td[ti + 64..ti + 96]);
    wr32(&mut bd, 352, chosen.slot);
    bd[356] = td[ti + 113];
    bd[357] = p[54];
    bd[358] = u8::from(chosen.cache);
    wr64(&mut bd, 360, chosen.score);
    drop(bd);
    drop(ud);
    drop(cd);
    wr64(&mut u.try_borrow_mut()?, 72, free);
    #[cfg(feature = "receipt-prototype")]
    {
        if generation > 0 {
            u.try_borrow_mut()?[113..120].copy_from_slice(&generation.to_le_bytes()[..7]);
        }
    }
    let mut d = c.try_borrow_mut()?;
    wr64(&mut d, ci + 8, chosen.exposure);
    wr32(&mut d, ci + 16, active);
    wr32(&mut d, ci + 20, obligations);
    Ok(())
}

#[inline(always)]
pub(crate) fn claim(program: &Address, accounts: &mut [AccountView], p: &[u8]) -> ProgramResult {
    #[cfg(feature = "receipt-prototype")]
    require(accounts.len() == 11 || accounts.len() == 12, 3)?;
    #[cfg(feature = "receipt-prototype")]
    let (accounts, receipt_accounts) = accounts.split_at_mut(11);
    let [m, mo, c, u, b, provider, vault, payout, mint, authority, token] = accounts else {
        return Err(err(3));
    };
    #[cfg(not(feature = "receipt-prototype"))]
    require(p.len() == 12, 3)?;
    #[cfg(feature = "receipt-prototype")]
    require(
        (p.len() == 156 && receipt_accounts.len() == 1)
            || (p.len() == 12 && receipt_accounts.is_empty()),
        3,
    )?;
    require(token.address() == &pinocchio_token::ID, 3)?;
    require(provider.is_signer(), 4)?;
    writable(&[c, u, b, vault, payout])?;
    let md = market(m, program)?;
    let modl = model(mo, program, m.address())?;
    let cd = book(c, program, mo, &modl, false)?;
    let bd = booking(b, program, m.address(), mo.address())?;
    #[cfg(feature = "receipt-prototype")]
    let generation = rd64(&bd, 152);
    #[cfg(feature = "receipt-prototype")]
    require(
        if generation == 0 {
            p.len() == 12
        } else {
            p.len() == 156 && generation == rd64(p, 12)
        },
        7,
    )?;
    let ci = slot_offset(&cd, rd32(&bd, 352), false)?;
    require(
        bd[148] == 0
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
    let charge =
        fee(uncached, rd32(&bd, 112)) + fee(cached, rd32(&bd, 116)) + fee(output, rd32(&bd, 120));
    let reserve = rd64(&bd, 104);
    require(charge <= reserve, 8)?;
    let free = add(rd64(&ud, 72), reserve - charge)?;
    let exposure = add(sub(rd64(&cd, ci + 8), reserve)?, charge)?;
    let active = dec(rd32(&cd, ci + 16))?;
    let obligations = if charge == 0 {
        dec(rd32(&cd, ci + 20))?
    } else {
        rd32(&cd, ci + 20)
    };
    let bump = ud[112];
    drop(ud);
    drop(bd);
    drop(cd);
    if charge > 0 {
        user_transfer(
            [vault, mint, payout, authority],
            u.address(),
            bump,
            program,
            charge,
        )?;
    }
    let mut d = b.try_borrow_mut()?;
    d[148] = if charge == 0 { 4 } else { 1 };
    wr64(&mut d, 160, charge);
    d[168..180].copy_from_slice(&p[..12]);
    drop(d);
    #[cfg(feature = "receipt-prototype")]
    if generation > 0 {
        receipt::settled(program, b, &mut receipt_accounts[0], charge, &p[20..])?;
    }
    wr64(&mut u.try_borrow_mut()?, 72, free);
    let mut d = c.try_borrow_mut()?;
    wr64(&mut d, ci + 8, exposure);
    wr32(&mut d, ci + 16, active);
    wr32(&mut d, ci + 20, obligations);
    Ok(())
}

#[inline(always)]
pub(crate) fn release(
    program: &Address,
    accounts: &mut [AccountView],
    op: u8,
    p: &[u8],
) -> ProgramResult {
    #[cfg(feature = "receipt-prototype")]
    require(p.len() == 8 || p.is_empty(), 3)?;
    #[cfg(not(feature = "receipt-prototype"))]
    require(p.is_empty(), 3)?;
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
    #[cfg(feature = "receipt-prototype")]
    let generation = rd64(&bd, 152);
    #[cfg(feature = "receipt-prototype")]
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
    let exposure = sub(rd64(&cd, ci + 8), reserve)?;
    let active = dec(rd32(&cd, ci + 16))?;
    let obligations = dec(rd32(&cd, ci + 20))?;
    drop(cd);
    drop(ud);
    drop(bd);
    b.try_borrow_mut()?[148] = 3;
    #[cfg(feature = "receipt-prototype")]
    {
        if generation > 0 {
            b.try_borrow_mut()?[148] = receipt::AVAILABLE;
        }
    }
    wr64(&mut u.try_borrow_mut()?, 72, free);
    let mut d = c.try_borrow_mut()?;
    wr64(&mut d, ci + 8, exposure);
    wr32(&mut d, ci + 16, active);
    wr32(&mut d, ci + 20, obligations);
    Ok(())
}

#[inline(always)]
pub(crate) fn finalize(program: &Address, accounts: &mut [AccountView], p: &[u8]) -> ProgramResult {
    let [m, mo, c, b] = accounts else {
        return Err(err(3));
    };
    require(p.is_empty(), 3)?;
    writable(&[c, b])?;
    market(m, program)?;
    let modl = model(mo, program, m.address())?;
    let cd = book(c, program, mo, &modl, false)?;
    let bd = booking(b, program, m.address(), mo.address())?;
    require(
        bd[148] == 1 && Clock::get()?.unix_timestamp > ri64(&bd, 140),
        7,
    )?;
    let ci = slot_offset(&cd, rd32(&bd, 352), false)?;
    let exposure = sub(rd64(&cd, ci + 8), rd64(&bd, 160))?;
    let obligations = dec(rd32(&cd, ci + 20))?;
    drop(cd);
    drop(bd);
    b.try_borrow_mut()?[148] = 4;
    let mut d = c.try_borrow_mut()?;
    wr64(&mut d, ci + 8, exposure);
    wr32(&mut d, ci + 20, obligations);
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
    if d.len() == BOOKING_LEN && &d[..8] == BOOK_MAGIC {
        #[cfg(feature = "receipt-prototype")]
        if d[359] == receipt::CASE {
            require(Clock::get()?.unix_timestamp > ri64(&d, 140), 7)?;
        }
        let terminal = d[148] == 3 || d[148] == 4;
        #[cfg(feature = "receipt-prototype")]
        let terminal = {
            if d[148] == receipt::AVAILABLE {
                require(recipient.is_signer(), 4)?;
            }
            terminal || d[148] == receipt::AVAILABLE
        };
        require(terminal && &d[256..288] == recipient.address().as_ref(), 7)?;
    } else if d.len() == UNBOND_LEN && &d[..8] == UNBOND_MAGIC {
        require(
            d[88] == 1 && &d[128..160] == recipient.address().as_ref(),
            7,
        )?;
    } else {
        return Err(err(2));
    }
    drop(d);
    let lamports = add(recipient.lamports(), a.lamports())?;
    recipient.set_lamports(lamports);
    a.close()
}
