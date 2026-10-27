use crate::{
    abi::{HEADER_LEN, PROVIDER_LEN, PROVIDER_MAGIC, TERMS_LEN},
    helpers::{addr, err, rd32, require, wr32},
    state::{
        book, initialize_pda, market, metadata_payload, model, rates, registered_provider,
        slot_offset, writable, write_metadata,
    },
    token::token_owned_by,
};
use anchor_lang::prelude::{Address, ProgramResult};
use anchor_lang::AnchorAccount;

#[inline(always)]
pub(crate) fn register_offer(
    ctx: &mut anchor_lang::Context<crate::contexts::RegisterOffer>,
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
    let profile = ctx.accounts.profile.account();
    let provider = ctx.accounts.provider.account();
    let payout = ctx.accounts.payout.account();
    require(p.len() == 24, 3)?;
    rates(&p[4..])?;
    writable(&[t, c])?;
    let md = market(m, program)?;
    let modl = model(mo, program, m.address())?;
    registered_provider(profile, provider, m.address(), program)?;
    let td = book(t, program, mo, &modl, true)?;
    let cd = book(c, program, mo, &modl, false)?;
    let slot = rd32(p, 0);
    require(
        slot == rd32(&td, 44) && slot == rd32(&cd, 44) && slot < rd32(&td, 40),
        6,
    )?;
    require(
        !td[HEADER_LEN..HEADER_LEN + slot as usize * TERMS_LEN]
            .chunks_exact(TERMS_LEN)
            .any(|entry| &entry[..32] == provider.address().as_ref()),
        6,
    )?;
    token_owned_by(payout, &addr(&md, 40), provider.address())?;
    drop(td);
    drop(cd);
    let at = HEADER_LEN + slot as usize * TERMS_LEN;
    let mut d = t.try_borrow_mut()?;
    let entry = &mut d[at..at + TERMS_LEN];
    entry.fill(0);
    entry[..32].copy_from_slice(provider.address().as_ref());
    entry[32..64].copy_from_slice(payout.address().as_ref());
    entry[96..112].copy_from_slice(&p[4..20]);
    wr32(entry, 116, rd32(p, 20));
    wr32(&mut d, 44, slot + 1);
    let mut cd = c.try_borrow_mut()?;
    let ci = HEADER_LEN + slot as usize * crate::abi::CAPACITY_LEN;
    cd[ci..ci + crate::abi::CAPACITY_LEN].fill(0);
    wr32(&mut cd, 44, slot + 1);
    Ok(())
}

#[inline(always)]
pub(crate) fn update_offer(
    ctx: &mut anchor_lang::Context<crate::contexts::UpdateOffer>,
    p: &[u8],
) -> ProgramResult {
    crate::helpers::require(ctx.remaining_accounts()?.is_empty(), 3)?;
    let program = ctx.program_id;
    let m = ctx.accounts.m.account();
    let mo = ctx.accounts.mo.account();
    let mut t = *ctx.accounts.t.account();
    let t = &mut t;
    let signer = ctx.accounts.signer.account();
    require(p.len() == 24, 3)?;
    writable(&[t])?;
    market(m, program)?;
    let modl = model(mo, program, m.address())?;
    let td = book(t, program, mo, &modl, true)?;
    let ti = slot_offset(&td, rd32(p, 0), true)?;
    require(signer.is_signer(), 4)?;
    rates(&p[4..])?;
    require(&td[ti..ti + 32] == signer.address().as_ref(), 4)?;
    drop(td);
    let mut d = t.try_borrow_mut()?;
    d[ti + 96..ti + 112].copy_from_slice(&p[4..20]);
    wr32(&mut d, ti + 116, rd32(p, 20));
    Ok(())
}

#[inline(always)]
pub(crate) fn initialize(
    ctx: &mut anchor_lang::Context<crate::contexts::InitializeProvider>,
    p: &[u8],
) -> ProgramResult {
    crate::helpers::require(ctx.remaining_accounts()?.is_empty(), 3)?;
    let program = ctx.program_id;
    let m = ctx.accounts.m.account();
    let mut profile = *ctx.accounts.profile.account();
    let profile = &mut profile;
    let provider = ctx.accounts.provider.account();
    let mut payer = *ctx.accounts.payer.account();
    let payer = &mut payer;
    let system = ctx.accounts.system.account();
    metadata_payload(p)?;
    require(system.address() == &anchor_lang::system_program::ID, 3)?;
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
    let signer_seeds: &[&[u8]] = &[
        b"provider",
        m.address().as_ref(),
        provider.address().as_ref(),
        &bump_ref,
    ];
    initialize_pda(profile, payer, program, PROVIDER_LEN, signer_seeds)?;
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
    ctx: &mut anchor_lang::Context<crate::contexts::UpdateProfile>,
    p: &[u8],
) -> ProgramResult {
    require(ctx.remaining_accounts()?.is_empty(), 3)?;
    let accounts = &mut ctx.accounts;
    market(accounts.m.account(), ctx.program_id)?;
    require(
        accounts.pr.account().data_len() == PROVIDER_LEN
            && accounts.pr.market == *accounts.m.address(),
        2,
    )?;
    require(
        accounts.pr.address()
            == &Address::derive_address(
                &[
                    b"provider",
                    accounts.m.address().as_ref(),
                    accounts.pr.authority.as_ref(),
                ],
                Some(accounts.pr.bump),
                ctx.program_id,
            ),
        2,
    )?;
    metadata_payload(p)?;
    require(accounts.pr.authority == *accounts.signer.address(), 4)?;
    accounts.pr.document_hash.copy_from_slice(&p[..32]);
    accounts.pr.uri_len = ((p.len() - 32) as u16).to_le_bytes();
    accounts.pr.uri[..p.len() - 32].copy_from_slice(&p[32..]);
    accounts.pr.uri[p.len() - 32..].fill(0);
    Ok(())
}
