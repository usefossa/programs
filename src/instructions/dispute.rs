use crate::{
    helpers::{add, collateral, dec, err, rd32, rd64, require, ri64, sub, wr32, wr64},
    state::{book, booked_user, booking, market, model, slot_offset, writable},
    token::{bond_transfer, bond_vault, user_vault},
};
use pinocchio::{
    sysvars::{clock::Clock, Sysvar},
    AccountView, Address, ProgramResult,
};

#[inline(always)]
pub(crate) fn open(program: &Address, accounts: &mut [AccountView], p: &[u8]) -> ProgramResult {
    let [m, mo, b, owner] = accounts else {
        return Err(err(3));
    };
    require(p.is_empty(), 3)?;
    writable(&[b])?;
    market(m, program)?;
    model(mo, program, m.address())?;
    let bd = booking(b, program, m.address(), mo.address())?;
    require(
        owner.is_signer()
            && bd[148] == 1
            && &bd[8..40] == owner.address().as_ref()
            && Clock::get()?.unix_timestamp <= ri64(&bd, 140),
        7,
    )?;
    drop(bd);
    b.try_borrow_mut()?[148] = 2;
    Ok(())
}

#[inline(always)]
pub(crate) fn resolve(program: &Address, accounts: &mut [AccountView], p: &[u8]) -> ProgramResult {
    let [m, mo, c, u, b, admin, bond, vault, mint, authority, token] = accounts else {
        return Err(err(3));
    };
    require(p.len() == 8 && token.address() == &pinocchio_token::ID, 3)?;
    writable(&[c, u, b, bond, vault])?;
    let md = market(m, program)?;
    require(
        admin.is_signer() && &md[8..40] == admin.address().as_ref(),
        4,
    )?;
    let modl = model(mo, program, m.address())?;
    let cd = book(c, program, mo, &modl, false)?;
    let bd = booking(b, program, m.address(), mo.address())?;
    require(bd[148] == 2, 7)?;
    let slot = rd32(&bd, 352);
    let ci = slot_offset(&cd, slot, false)?;
    let ud = booked_user(u, program, &bd, m.address())?;
    bond_vault(
        [bond, mint],
        &md,
        mo.address(),
        slot,
        &bd[320..352],
        bd[356],
        program,
    )?;
    user_vault(vault, mint, &md, u, &ud, program)?;
    let refund = rd64(p, 0);
    let charge = rd64(&bd, 160);
    require(refund <= charge, 8)?;
    let free = add(rd64(&ud, 72), refund)?;
    let balance = sub(rd64(&cd, ci), refund)?;
    let exposure = sub(rd64(&cd, ci + 8), charge)?;
    require(balance >= collateral(exposure)?, 8)?;
    let obligations = dec(rd32(&cd, ci + 20))?;
    let bump = bd[356];
    drop(cd);
    drop(ud);
    drop(bd);
    if refund > 0 {
        bond_transfer(
            [bond, mint, vault, authority],
            mo.address(),
            slot,
            bump,
            program,
            refund,
        )?;
    }
    b.try_borrow_mut()?[148] = 4;
    wr64(&mut u.try_borrow_mut()?, 72, free);
    let mut d = c.try_borrow_mut()?;
    wr64(&mut d, ci, balance);
    wr64(&mut d, ci + 8, exposure);
    wr32(&mut d, ci + 20, obligations);
    Ok(())
}
