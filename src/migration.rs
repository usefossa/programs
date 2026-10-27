use crate::{
    helpers::{rd32, rd64, require},
    state::{book, market, model, slot_offset, writable},
    token::{bond_transfer, bond_vault, token_owned_by},
};
use anchor_lang::prelude::Account;
use anchor_lang::prelude::{Address, ProgramResult};
use anchor_lang::AnchorAccount;
use anchor_spl::token::TokenAccount;

// temporary migration artifact only; no default-build dispatch or legacy token authority code.
pub(crate) fn sweep(
    ctx: &mut anchor_lang::Context<crate::contexts::SweepLegacy>,
    p: &[u8],
) -> ProgramResult {
    crate::helpers::require(ctx.remaining_accounts()?.is_empty(), 3)?;
    let program = ctx.program_id;
    let m = ctx.accounts.m.account();
    let mo = ctx.accounts.mo.account();
    let mut t = *ctx.accounts.t.account();
    let t = &mut t;
    let mut c = *ctx.accounts.c.account();
    let c = &mut c;
    let mut provider = *ctx.accounts.provider.account();
    let provider = &mut provider;
    let mut vault = *ctx.accounts.vault.account();
    let vault = &mut vault;
    let mut payout = *ctx.accounts.payout.account();
    let payout = &mut payout;
    let mint = ctx.accounts.mint.account();
    let authority = ctx.accounts.authority.account();
    let token = ctx.accounts.token.account();
    require(p.len() == 4 && token.address() == &anchor_spl::token::ID, 3)?;
    writable(&[t, c, provider, vault, payout])?;
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
    require(&td[ti + 32..ti + 64] == payout.address().as_ref(), 2)?;
    require(
        rd64(&cd, ci) == 0
            && rd64(&cd, ci + 8) == 0
            && rd32(&cd, ci + 16) == 0
            && rd32(&cd, ci + 20) == 0
            && rd64(&cd, ci + 24) == 0,
        7,
    )?;
    let bump = td[ti + 113];
    bond_vault(
        [vault, mint],
        &md,
        mo.address(),
        slot,
        &td[ti + 64..ti + 96],
        bump,
        program,
    )?;
    token_owned_by(payout, mint.address(), provider.address())?;
    let amount = Account::<TokenAccount>::load(*vault)?.amount();
    let slot_bytes = slot.to_le_bytes();
    require(
        authority.address()
            == &Address::derive_address(
                &[b"bond", mo.address().as_ref(), &slot_bytes],
                Some(bump),
                program,
            ),
        2,
    )?;
    drop(cd);
    drop(td);
    if amount > 0 {
        bond_transfer(
            [vault, mint, payout, authority],
            mo.address(),
            slot,
            bump,
            program,
            amount,
        )?;
    }
    let bump_ref = [bump];
    let signer_seeds: &[&[u8]] = &[b"bond", mo.address().as_ref(), &slot_bytes, &bump_ref];
    crate::cross_program::close_token(
        vault,
        provider,
        authority,
        &anchor_spl::token::ID,
        &[signer_seeds],
    )?;
    let mut td = t.try_borrow_mut()?;
    td[ti + 64..ti + 96].fill(0);
    td[ti + 112..ti + 116].fill(0);
    c.try_borrow_mut()?[ci..ci + crate::abi::CAPACITY_LEN].fill(0);
    Ok(())
}
