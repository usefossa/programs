use crate::{
    abi::{
        capacity_book_len, terms_book_len, CAPACITY_MAGIC, HEADER_LEN, MARKET_LEN, MARKET_MAGIC,
        MAX_ENTRIES, MODEL_LEN, MODEL_MAGIC, MODEL_METADATA_LEN, MODEL_METADATA_MAGIC, TERMS_MAGIC,
    },
    helpers::{err, rd32, require, wr32},
    state::{
        fresh, initialize_pda, market, metadata_payload, model, registered_provider, state,
        writable, write_metadata,
    },
};
use anchor_lang::prelude::Rent;
use anchor_lang::prelude::{AccountView, Address, ProgramResult};
use anchor_lang::solana_program::sysvar::Sysvar as _;
use anchor_lang::AnchorAccount;

use anchor_lang::prelude::Account;
use anchor_spl::mint::Mint;

#[inline(always)]
pub(crate) fn initialize(
    ctx: &mut anchor_lang::Context<crate::contexts::InitializeMarket>,
    p: &[u8],
) -> ProgramResult {
    crate::helpers::require(ctx.remaining_accounts()?.is_empty(), 3)?;
    let program = ctx.program_id;
    let mut m = *ctx.accounts.m.account();
    let m = &mut m;
    let admin = ctx.accounts.admin.account();
    let mint = ctx.accounts.mint.account();
    require(p.is_empty() && admin.is_signer(), 3)?;
    fresh(m, program, MARKET_LEN)?;
    let mt = Account::<Mint>::load(*mint)?;
    require(mt.is_initialized() && mt.decimals() == 6, 2)?;
    let mut d = m.try_borrow_mut()?;
    d[..8].copy_from_slice(MARKET_MAGIC);
    d[8..40].copy_from_slice(admin.address().as_ref());
    d[40..72].copy_from_slice(mint.address().as_ref());
    Ok(())
}

#[inline(always)]
pub(crate) fn initialize_model(
    ctx: &mut anchor_lang::Context<crate::contexts::InitializeModel>,
    p: &[u8],
) -> ProgramResult {
    crate::helpers::require(ctx.remaining_accounts()?.is_empty(), 3)?;
    let program = ctx.program_id;
    let m = ctx.accounts.m.account();
    let mut mo = *ctx.accounts.mo.account();
    let mo = &mut mo;
    let mut t = *ctx.accounts.t.account();
    let t = &mut t;
    let mut c = *ctx.accounts.c.account();
    let c = &mut c;
    let provider = ctx.accounts.provider.account();
    let mut payer = *ctx.accounts.payer.account();
    let payer = &mut payer;
    let system = ctx.accounts.system.account();
    let profile = ctx.accounts.profile.account();
    let mut metadata = *ctx.accounts.metadata.account();
    let metadata = &mut metadata;
    require(
        p.len() >= 36 && system.address() == &anchor_lang::system_program::ID,
        3,
    )?;
    metadata_payload(&p[36..])?;
    market(m, program)?;
    registered_provider(profile, provider, m.address(), program)?;
    let limit = rd32(p, 32) as usize;
    require((1..=MAX_ENTRIES).contains(&limit), 3)?;
    require(t.address() != c.address(), 2)?;
    // zero headers are sufficient: this program never writes uninitialized book entries or closes books.
    for (account, len) in [
        (t as &AccountView, terms_book_len(limit)),
        (c as &AccountView, capacity_book_len(limit)),
    ] {
        require(
            account.is_signer()
                && account.is_writable()
                && account.owner() == program
                && account.data_len() == len
                && account.try_borrow()?[..HEADER_LEN].iter().all(|x| *x == 0),
            2,
        )?;
    }
    let (address, bump) =
        Address::derive_program_address(&[b"model", m.address().as_ref(), &p[..32]], program)
            .ok_or(err(2))?;
    require(mo.address() == &address, 2)?;
    let bump_ref = [bump];
    let signer_seeds: &[&[u8]] = &[b"model", m.address().as_ref(), &p[..32], &bump_ref];
    initialize_pda(mo, payer, program, MODEL_LEN, signer_seeds)?;
    let (address, meta_bump) =
        Address::derive_program_address(&[b"model_metadata", mo.address().as_ref()], program)
            .ok_or(err(2))?;
    require(metadata.address() == &address, 2)?;
    let meta_bump_ref = [meta_bump];
    let meta_seeds: &[&[u8]] = &[b"model_metadata", mo.address().as_ref(), &meta_bump_ref];
    initialize_pda(metadata, payer, program, MODEL_METADATA_LEN, meta_seeds)?;
    let mut d = metadata.try_borrow_mut()?;
    d[..8].copy_from_slice(MODEL_METADATA_MAGIC);
    d[8..40].copy_from_slice(m.address().as_ref());
    d[40..72].copy_from_slice(mo.address().as_ref());
    d[72..104].copy_from_slice(provider.address().as_ref());
    d[138] = meta_bump;
    write_metadata(&mut d, 104, 140, &p[36..]);
    drop(d);
    let mut d = mo.try_borrow_mut()?;
    d[..8].copy_from_slice(MODEL_MAGIC);
    d[8..40].copy_from_slice(m.address().as_ref());
    d[40..72].copy_from_slice(&p[..32]);
    d[72..104].copy_from_slice(t.address().as_ref());
    d[104..136].copy_from_slice(c.address().as_ref());
    wr32(&mut d, 136, limit as u32);
    d[140] = bump;
    drop(d);
    for (account, magic) in [(t, TERMS_MAGIC), (c, CAPACITY_MAGIC)] {
        let mut d = account.try_borrow_mut()?;
        d[..8].copy_from_slice(magic);
        d[8..40].copy_from_slice(mo.address().as_ref());
        wr32(&mut d, 40, limit as u32);
    }
    Ok(())
}

#[inline(always)]
pub(crate) fn update_metadata(
    ctx: &mut anchor_lang::Context<crate::contexts::UpdateModelMetadata>,
    p: &[u8],
) -> ProgramResult {
    crate::helpers::require(ctx.remaining_accounts()?.is_empty(), 3)?;
    let program = ctx.program_id;
    let m = ctx.accounts.m.account();
    let mo = ctx.accounts.mo.account();
    let mut metadata = *ctx.accounts.metadata.account();
    let metadata = &mut metadata;
    let provider = ctx.accounts.provider.account();
    metadata_payload(p)?;
    writable(&[metadata])?;
    market(m, program)?;
    model(mo, program, m.address())?;
    let d = state(metadata, program, MODEL_METADATA_LEN, MODEL_METADATA_MAGIC)?;
    require(
        &d[8..40] == m.address().as_ref()
            && &d[40..72] == mo.address().as_ref()
            && metadata.address()
                == &Address::derive_address(
                    &[b"model_metadata", mo.address().as_ref()],
                    Some(d[138]),
                    program,
                ),
        2,
    )?;
    require(
        provider.is_signer() && &d[72..104] == provider.address().as_ref(),
        4,
    )?;
    drop(d);
    write_metadata(&mut metadata.try_borrow_mut()?, 104, 140, p);
    Ok(())
}

pub(crate) fn grow(
    ctx: &mut anchor_lang::Context<crate::contexts::GrowBooks>,
    p: &[u8],
) -> ProgramResult {
    crate::helpers::require(ctx.remaining_accounts()?.is_empty(), 3)?;
    let program = ctx.program_id;
    let m = ctx.accounts.m.account();
    let mut mo = *ctx.accounts.mo.account();
    let mo = &mut mo;
    let mut t = *ctx.accounts.t.account();
    let t = &mut t;
    let mut c = *ctx.accounts.c.account();
    let c = &mut c;
    let mut payer = *ctx.accounts.payer.account();
    let payer = &mut payer;
    let system = ctx.accounts.system.account();
    require(
        p.len() == 4 && system.address() == &anchor_lang::system_program::ID,
        3,
    )?;
    require(payer.is_signer(), 4)?;
    writable(&[mo, t, c, payer])?;
    market(m, program)?;
    let md = model(mo, program, m.address())?;
    let old = rd32(&md, 136) as usize;
    let limit = rd32(p, 0) as usize;
    require(limit > old && limit <= MAX_ENTRIES && limit - old <= 85, 3)?;
    let td = crate::state::book(t, program, mo, &md, true)?;
    let cd = crate::state::book(c, program, mo, &md, false)?;
    require(rd32(&td, 44) == rd32(&cd, 44), 2)?;
    drop((td, cd, md));
    let rent = Rent::get()?;
    for (account, len) in [(t, terms_book_len(limit)), (c, capacity_book_len(limit))] {
        let minimum = rent.try_minimum_balance(len)?;
        if account.lamports() < minimum {
            crate::cross_program::transfer_lamports(payer, account, minimum - account.lamports())?;
        }
        anchor_lang::cpi::realloc_account(account, len, payer, true)?;
        wr32(&mut account.try_borrow_mut()?, 40, limit as u32);
    }
    wr32(&mut mo.try_borrow_mut()?, 136, limit as u32);
    Ok(())
}
