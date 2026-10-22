use crate::{
    abi::{UNBOND_LEN, UNBOND_MAGIC},
    helpers::{add, collateral, err, rd32, rd64, require, ri64, sub, wi64, wr32, wr64},
    state::{book, fresh, market, model, slot_offset, state, writable},
    token::{bond_transfer, bond_vault, token_owned_by, transfer},
    UNBOND_SECONDS,
};
use pinocchio::{
    sysvars::{clock::Clock, Sysvar},
    AccountView, Address, ProgramResult,
};

#[inline(always)]
pub(crate) fn fund(program: &Address, accounts: &mut [AccountView], p: &[u8]) -> ProgramResult {
    let [m, mo, t, c, provider, source, vault, mint, token] = accounts else {
        return Err(err(3));
    };
    require(p.len() == 12 && token.address() == &pinocchio_token::ID, 3)?;
    writable(&[c, source, vault])?;
    let md = market(m, program)?;
    let modl = model(mo, program, m.address())?;
    let td = book(t, program, mo, &modl, true)?;
    let cd = book(c, program, mo, &modl, false)?;
    let slot = rd32(p, 0);
    let ti = slot_offset(&td, slot, true)?;
    let ci = slot_offset(&cd, slot, false)?;
    require(
        provider.is_signer() && &td[ti..ti + 32] == provider.address().as_ref(),
        4,
    )?;
    bond_vault(
        [vault, mint],
        &md,
        mo.address(),
        slot,
        &td[ti + 64..ti + 96],
        td[ti + 113],
        program,
    )?;
    token_owned_by(source, mint.address(), provider.address())?;
    let amount = rd64(p, 4);
    require(amount > 0, 3)?;
    let balance = add(rd64(&cd, ci), amount)?;
    drop(cd);
    transfer(source, mint, vault, provider, amount)?;
    wr64(&mut c.try_borrow_mut()?, ci, balance);
    Ok(())
}

#[inline(always)]
pub(crate) fn request_unbond(
    program: &Address,
    accounts: &mut [AccountView],
    p: &[u8],
) -> ProgramResult {
    let [m, mo, t, c, provider, request, rent_payer] = accounts else {
        return Err(err(3));
    };
    require(p.len() == 12, 3)?;
    require(rent_payer.is_signer(), 4)?;
    writable(&[c])?;
    fresh(request, program, UNBOND_LEN)?;
    market(m, program)?;
    let modl = model(mo, program, m.address())?;
    let td = book(t, program, mo, &modl, true)?;
    let cd = book(c, program, mo, &modl, false)?;
    let slot = rd32(p, 0);
    let ti = slot_offset(&td, slot, true)?;
    let ci = slot_offset(&cd, slot, false)?;
    require(
        provider.is_signer() && &td[ti..ti + 32] == provider.address().as_ref(),
        4,
    )?;
    let amount = rd64(p, 4);
    require(amount > 0, 3)?;
    let remaining = sub(rd64(&cd, ci), amount)?;
    require(remaining >= collateral(rd64(&cd, ci + 8))?, 8)?;
    let pending = add(rd64(&cd, ci + 24), amount)?;
    let ready = Clock::get()?
        .unix_timestamp
        .checked_add(UNBOND_SECONDS)
        .ok_or(err(1))?;
    drop(cd);
    let mut d = request.try_borrow_mut()?;
    d[..8].copy_from_slice(UNBOND_MAGIC);
    d[8..40].copy_from_slice(m.address().as_ref());
    d[40..72].copy_from_slice(provider.address().as_ref());
    wr64(&mut d, 72, amount);
    wi64(&mut d, 80, ready);
    wr32(&mut d, 92, slot);
    d[96..128].copy_from_slice(mo.address().as_ref());
    d[128..160].copy_from_slice(rent_payer.address().as_ref());
    drop(d);
    let mut d = c.try_borrow_mut()?;
    wr64(&mut d, ci, remaining);
    wr64(&mut d, ci + 24, pending);
    Ok(())
}

#[inline(always)]
pub(crate) fn execute_unbond(
    program: &Address,
    accounts: &mut [AccountView],
    p: &[u8],
) -> ProgramResult {
    let [m, mo, t, c, request, provider, vault, payout, mint, authority, token] = accounts else {
        return Err(err(3));
    };
    require(
        p.is_empty() && token.address() == &pinocchio_token::ID && provider.is_signer(),
        3,
    )?;
    writable(&[c, request, vault, payout])?;
    let md = market(m, program)?;
    let modl = model(mo, program, m.address())?;
    let td = book(t, program, mo, &modl, true)?;
    let cd = book(c, program, mo, &modl, false)?;
    let r = state(request, program, UNBOND_LEN, UNBOND_MAGIC)?;
    require(
        &r[8..40] == m.address().as_ref()
            && &r[40..72] == provider.address().as_ref()
            && &r[96..128] == mo.address().as_ref()
            && r[88] == 0
            && Clock::get()?.unix_timestamp >= ri64(&r, 80),
        7,
    )?;
    let slot = rd32(&r, 92);
    let ti = slot_offset(&td, slot, true)?;
    let ci = slot_offset(&cd, slot, false)?;
    require(
        &td[ti..ti + 32] == provider.address().as_ref()
            && &td[ti + 32..ti + 64] == payout.address().as_ref(),
        2,
    )?;
    bond_vault(
        [vault, mint],
        &md,
        mo.address(),
        slot,
        &td[ti + 64..ti + 96],
        td[ti + 113],
        program,
    )?;
    token_owned_by(payout, mint.address(), provider.address())?;
    let amount = rd64(&r, 72);
    let pending = sub(rd64(&cd, ci + 24), amount)?;
    let bump = td[ti + 113];
    drop(r);
    drop(cd);
    bond_transfer(
        [vault, mint, payout, authority],
        mo.address(),
        slot,
        bump,
        program,
        amount,
    )?;
    request.try_borrow_mut()?[88] = 1;
    wr64(&mut c.try_borrow_mut()?, ci + 24, pending);
    Ok(())
}
