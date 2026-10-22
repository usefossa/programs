use crate::{
    abi::{HEADER_LEN, PROVIDER_LEN, PROVIDER_MAGIC, TERMS_LEN},
    helpers::{addr, err, rd32, require, wr32},
    state::{
        approved_provider, book, initialize_pda, market, metadata_payload, model, profile, rates,
        slot_offset, writable, write_metadata,
    },
    token::{token_owned_by, vault_owned_by},
};
use pinocchio::{cpi::Signer, instruction::seeds, AccountView, Address, ProgramResult};

#[inline(always)]
pub(crate) fn register_offer(
    program: &Address,
    accounts: &mut [AccountView],
    p: &[u8],
) -> ProgramResult {
    require(accounts.len() == 8 || accounts.len() == 9, 3)?;
    let (accounts, extra) = accounts.split_at_mut(8);
    let [m, mo, t, c, profile, provider, payout, vault] = accounts else {
        return Err(err(3));
    };
    require(p.len() == 24, 3)?;
    rates(&p[4..])?;
    writable(&[t, c])?;
    let md = market(m, program)?;
    let modl = model(mo, program, m.address())?;
    approved_provider(profile, provider, m.address(), program)?;
    let td = book(t, program, mo, &modl, true)?;
    let cd = book(c, program, mo, &modl, false)?;
    let slot = rd32(p, 0);
    require(
        slot == rd32(&td, 44) && slot == rd32(&cd, 44) && slot < rd32(&td, 40),
        6,
    )?;
    if td[HEADER_LEN..HEADER_LEN + slot as usize * TERMS_LEN]
        .chunks_exact(TERMS_LEN)
        .any(|entry| &entry[..32] == provider.address().as_ref())
    {
        let admin = extra.first().unwrap_or(provider);
        require(
            admin.is_signer() && &md[8..40] == admin.address().as_ref(),
            4,
        )?;
    }
    token_owned_by(payout, &addr(&md, 40), provider.address())?;
    let (authority, bump) = Address::derive_program_address(
        &[b"bond", mo.address().as_ref(), &slot.to_le_bytes()],
        program,
    )
    .ok_or(err(2))?;
    vault_owned_by(vault, &addr(&md, 40), &authority)?;
    drop(td);
    drop(cd);
    let at = HEADER_LEN + slot as usize * TERMS_LEN;
    let mut d = t.try_borrow_mut()?;
    let entry = &mut d[at..at + TERMS_LEN];
    entry[..32].copy_from_slice(provider.address().as_ref());
    entry[32..64].copy_from_slice(payout.address().as_ref());
    entry[64..96].copy_from_slice(vault.address().as_ref());
    entry[96..112].copy_from_slice(&p[4..20]);
    entry[112] = 1;
    entry[113] = bump;
    wr32(entry, 116, rd32(p, 20));
    wr32(&mut d, 44, slot + 1);
    wr32(&mut c.try_borrow_mut()?, 44, slot + 1);
    Ok(())
}

#[inline(always)]
pub(crate) fn update_offer(
    program: &Address,
    accounts: &mut [AccountView],
    op: u8,
    p: &[u8],
) -> ProgramResult {
    let [m, mo, t, signer] = accounts else {
        return Err(err(3));
    };
    require(p.len() == if op == 5 { 24 } else { 5 }, 3)?;
    writable(&[t])?;
    let md = market(m, program)?;
    let modl = model(mo, program, m.address())?;
    let td = book(t, program, mo, &modl, true)?;
    let ti = slot_offset(&td, rd32(p, 0), true)?;
    require(signer.is_signer(), 4)?;
    if op == 5 {
        rates(&p[4..])?;
        require(&td[ti..ti + 32] == signer.address().as_ref(), 4)?;
    } else {
        require(p[4] <= 1, 3)?;
        require(&md[8..40] == signer.address().as_ref(), 4)?;
    }
    drop(td);
    let mut d = t.try_borrow_mut()?;
    if op == 5 {
        d[ti + 96..ti + 112].copy_from_slice(&p[4..20]);
        wr32(&mut d, ti + 116, rd32(p, 20));
    } else {
        d[ti + 112] = p[4];
    }
    Ok(())
}

#[inline(always)]
pub(crate) fn initialize(
    program: &Address,
    accounts: &mut [AccountView],
    p: &[u8],
) -> ProgramResult {
    let [m, profile, provider, payer, system] = accounts else {
        return Err(err(3));
    };
    metadata_payload(p)?;
    require(system.address() == &pinocchio_system::ID, 3)?;
    market(m, program)?;
    require(provider.is_signer(), 4)?;
    let (address, bump) = Address::derive_program_address(
        &[
            b"provider",
            m.address().as_ref(),
            provider.address().as_ref(),
        ],
        program,
    )
    .ok_or(err(2))?;
    require(profile.address() == &address, 2)?;
    let bump_ref = [bump];
    let signer_seeds = seeds!(
        b"provider",
        m.address().as_ref(),
        provider.address().as_ref(),
        &bump_ref
    );
    initialize_pda(
        profile,
        payer,
        program,
        PROVIDER_LEN,
        Signer::from(&signer_seeds),
    )?;
    let mut d = profile.try_borrow_mut()?;
    d[..8].copy_from_slice(PROVIDER_MAGIC);
    d[8..40].copy_from_slice(m.address().as_ref());
    d[40..72].copy_from_slice(provider.address().as_ref());
    d[107] = bump;
    write_metadata(&mut d, 72, 108, p);
    Ok(())
}

#[inline(always)]
pub(crate) fn update_profile(
    program: &Address,
    accounts: &mut [AccountView],
    op: u8,
    p: &[u8],
) -> ProgramResult {
    let [m, pr, signer] = accounts else {
        return Err(err(3));
    };
    writable(&[pr])?;
    let md = market(m, program)?;
    let pd = profile(pr, program, m.address())?;
    require(signer.is_signer(), 4)?;
    if op == 20 {
        require(p.len() == 1 && p[0] <= 1, 3)?;
        require(&md[8..40] == signer.address().as_ref(), 4)?;
    } else {
        metadata_payload(p)?;
        require(&pd[40..72] == signer.address().as_ref(), 4)?;
    }
    drop(pd);
    let mut d = pr.try_borrow_mut()?;
    if op == 20 {
        d[106] = p[0];
    } else {
        write_metadata(&mut d, 72, 108, p);
    }
    Ok(())
}
