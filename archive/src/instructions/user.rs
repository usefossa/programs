use crate::{
    abi::{USER_LEN, USER_MAGIC},
    helpers::{add, addr, err, rd64, require, sub, wr64},
    state::{fresh, market, user, writable},
    token::{token_owned_by, transfer, user_transfer, user_vault, vault_owned_by},
};
use pinocchio::{AccountView, Address, ProgramResult};

#[inline(always)]
pub(crate) fn initialize(
    program: &Address,
    accounts: &mut [AccountView],
    p: &[u8],
) -> ProgramResult {
    let [u, owner, m, vault] = accounts else {
        return Err(err(3));
    };
    require(p.is_empty() && owner.is_signer(), 3)?;
    fresh(u, program, USER_LEN)?;
    let md = market(m, program)?;
    let (authority, bump) =
        Address::derive_program_address(&[b"vault", u.address().as_ref()], program)
            .ok_or(err(2))?;
    vault_owned_by(vault, &addr(&md, 40), &authority)?;
    let mut d = u.try_borrow_mut()?;
    d[..8].copy_from_slice(USER_MAGIC);
    d[8..40].copy_from_slice(owner.address().as_ref());
    d[40..72].copy_from_slice(m.address().as_ref());
    d[80..112].copy_from_slice(vault.address().as_ref());
    d[112] = bump;
    Ok(())
}

#[inline(always)]
pub(crate) fn deposit(program: &Address, accounts: &mut [AccountView], p: &[u8]) -> ProgramResult {
    let [m, u, owner, source, vault, mint, token] = accounts else {
        return Err(err(3));
    };
    require(p.len() == 8 && token.address() == &pinocchio_token::ID, 3)?;
    require(owner.is_signer(), 4)?;
    writable(&[u, source, vault])?;
    let md = market(m, program)?;
    let ud = user(u, program, owner.address(), m.address())?;
    user_vault(vault, mint, &md, u, &ud, program)?;
    token_owned_by(source, mint.address(), owner.address())?;
    let amount = rd64(p, 0);
    require(amount > 0, 3)?;
    let balance = add(rd64(&ud, 72), amount)?;
    drop(ud);
    transfer(source, mint, vault, owner, amount)?;
    wr64(&mut u.try_borrow_mut()?, 72, balance);
    Ok(())
}

#[inline(always)]
pub(crate) fn withdraw(program: &Address, accounts: &mut [AccountView], p: &[u8]) -> ProgramResult {
    let [m, u, owner, vault, destination, mint, authority, token] = accounts else {
        return Err(err(3));
    };
    require(p.len() == 8 && token.address() == &pinocchio_token::ID, 3)?;
    require(owner.is_signer(), 4)?;
    writable(&[u, vault, destination])?;
    let md = market(m, program)?;
    let ud = user(u, program, owner.address(), m.address())?;
    user_vault(vault, mint, &md, u, &ud, program)?;
    token_owned_by(destination, mint.address(), owner.address())?;
    let amount = rd64(p, 0);
    require(amount > 0, 3)?;
    let free = sub(rd64(&ud, 72), amount)?;
    let bump = ud[112];
    drop(ud);
    user_transfer(
        [vault, mint, destination, authority],
        u.address(),
        bump,
        program,
        amount,
    )?;
    wr64(&mut u.try_borrow_mut()?, 72, free);
    Ok(())
}
