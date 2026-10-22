use crate::{
    abi::{
        capacity_book_len, terms_book_len, CAPACITY_MAGIC, HEADER_LEN, MARKET_LEN, MARKET_MAGIC,
        MAX_ENTRIES, MODEL_LEN, MODEL_MAGIC, MODEL_METADATA_LEN, MODEL_METADATA_MAGIC, TERMS_MAGIC,
    },
    helpers::{err, rd32, require, wr32},
    state::{
        approved_provider, fresh, initialize_pda, market, metadata_payload, model, state, writable,
        write_metadata,
    },
};
use pinocchio::{cpi::Signer, instruction::seeds, AccountView, Address, ProgramResult};
use pinocchio_token::state::Mint;

#[inline(always)]
pub(crate) fn initialize(
    program: &Address,
    accounts: &mut [AccountView],
    p: &[u8],
) -> ProgramResult {
    let [m, admin, mint] = accounts else {
        return Err(err(3));
    };
    require(p.is_empty() && admin.is_signer(), 3)?;
    fresh(m, program, MARKET_LEN)?;
    let mt = Mint::from_account_view(mint)?;
    require(mt.is_initialized() && mt.decimals() == 6, 2)?;
    let mut d = m.try_borrow_mut()?;
    d[..8].copy_from_slice(MARKET_MAGIC);
    d[8..40].copy_from_slice(admin.address().as_ref());
    d[40..72].copy_from_slice(mint.address().as_ref());
    Ok(())
}

#[inline(always)]
pub(crate) fn initialize_model(
    program: &Address,
    accounts: &mut [AccountView],
    p: &[u8],
) -> ProgramResult {
    require(accounts.len() == 9 || accounts.len() == 10, 3)?;
    let (accounts, extra) = accounts.split_at_mut(9);
    let [m, mo, t, c, provider, payer, system, profile, metadata] = accounts else {
        return Err(err(3));
    };
    require(
        p.len() >= 36 && system.address() == &pinocchio_system::ID,
        3,
    )?;
    metadata_payload(&p[36..])?;
    let md = market(m, program)?;
    let admin = extra.first().unwrap_or(provider);
    require(
        admin.is_signer() && &md[8..40] == admin.address().as_ref(),
        4,
    )?;
    approved_provider(profile, provider, m.address(), program)?;
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
    let signer_seeds = seeds!(b"model", m.address().as_ref(), &p[..32], &bump_ref);
    initialize_pda(mo, payer, program, MODEL_LEN, Signer::from(&signer_seeds))?;
    let (address, meta_bump) =
        Address::derive_program_address(&[b"model_metadata", mo.address().as_ref()], program)
            .ok_or(err(2))?;
    require(metadata.address() == &address, 2)?;
    let meta_bump_ref = [meta_bump];
    let meta_seeds = seeds!(b"model_metadata", mo.address().as_ref(), &meta_bump_ref);
    initialize_pda(
        metadata,
        payer,
        program,
        MODEL_METADATA_LEN,
        Signer::from(&meta_seeds),
    )?;
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
    program: &Address,
    accounts: &mut [AccountView],
    p: &[u8],
) -> ProgramResult {
    let [m, mo, metadata, provider] = accounts else {
        return Err(err(3));
    };
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
