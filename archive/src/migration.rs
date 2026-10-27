use crate::{
    helpers::{err, rd32, rd64, require},
    state::{book, market, model, slot_offset, writable},
    token::{bond_transfer, bond_vault, token_owned_by},
};
use pinocchio::{cpi::Signer, instruction::seeds, AccountView, Address, ProgramResult};
use pinocchio_token::{
    instructions::close_account::CloseAccount, state::Account as TokenAccount, TokenProgram,
};

// temporary migration artifact only; no default-build dispatch or legacy token authority code.
pub(crate) fn sweep(program: &Address, accounts: &mut [AccountView], p: &[u8]) -> ProgramResult {
    let [m, mo, t, c, provider, vault, payout, mint, authority, token] = accounts else {
        return Err(err(3));
    };
    require(p.len() == 4 && token.address() == &pinocchio_token::ID, 3)?;
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
    let amount = TokenAccount::from_account_view(vault)?.amount();
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
    let signer_seeds = seeds!(b"bond", mo.address().as_ref(), &slot_bytes, &bump_ref);
    CloseAccount::<&AccountView, TokenProgram>::new(vault, provider, authority)
        .invoke_signed(&[Signer::from(&signer_seeds)])?;
    let mut td = t.try_borrow_mut()?;
    td[ti + 64..ti + 96].fill(0);
    td[ti + 112..ti + 116].fill(0);
    c.try_borrow_mut()?[ci..ci + crate::abi::CAPACITY_LEN].fill(0);
    Ok(())
}
