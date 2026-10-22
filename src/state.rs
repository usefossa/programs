use crate::{
    abi::*,
    helpers::{addr, rd16, rd32, require},
};
use pinocchio::{
    account::Ref,
    cpi::Signer,
    error::ProgramError,
    sysvars::{rent::Rent, Sysvar},
    AccountView, Address, ProgramResult,
};
use pinocchio_system::instructions::{Allocate, Assign, Transfer};

pub(crate) fn writable(accounts: &[&AccountView]) -> ProgramResult {
    require(accounts.iter().all(|a| a.is_writable()), 2)
}
pub(crate) fn state<'a>(
    a: &'a AccountView,
    program: &Address,
    len: usize,
    magic: &[u8; 8],
) -> Result<Ref<'a, [u8]>, ProgramError> {
    require(a.owner() == program && a.data_len() == len, 2)?;
    let d = a.try_borrow()?;
    require(&d[..8] == magic, 2)?;
    Ok(d)
}
pub(crate) fn fresh(a: &AccountView, program: &Address, len: usize) -> ProgramResult {
    require(
        a.is_signer() && a.is_writable() && a.owner() == program && a.data_len() == len,
        2,
    )?;
    require(all_zero(&a.try_borrow()?), 2)
}
pub(crate) fn all_zero(data: &[u8]) -> bool {
    let mut words = data.chunks_exact(8);
    words.all(|w| u64::from_le_bytes(w.try_into().unwrap()) == 0)
        && words.remainder().iter().all(|b| *b == 0)
}
pub(crate) fn market<'a>(
    a: &'a AccountView,
    program: &Address,
) -> Result<Ref<'a, [u8]>, ProgramError> {
    state(a, program, MARKET_LEN, MARKET_MAGIC)
}
pub(crate) fn model<'a>(
    a: &'a AccountView,
    program: &Address,
    m: &Address,
) -> Result<Ref<'a, [u8]>, ProgramError> {
    let d = state(a, program, MODEL_LEN, MODEL_MAGIC)?;
    require(
        &d[8..40] == m.as_ref()
            && a.address()
                == &Address::derive_address(
                    &[b"model", m.as_ref(), &d[40..72]],
                    Some(d[140]),
                    program,
                ),
        2,
    )?;
    Ok(d)
}
pub(crate) fn profile<'a>(
    a: &'a AccountView,
    program: &Address,
    m: &Address,
) -> Result<Ref<'a, [u8]>, ProgramError> {
    let d = state(a, program, PROVIDER_LEN, PROVIDER_MAGIC)?;
    require(
        &d[8..40] == m.as_ref()
            && a.address()
                == &Address::derive_address(
                    &[b"provider", m.as_ref(), &d[40..72]],
                    Some(d[107]),
                    program,
                ),
        2,
    )?;
    Ok(d)
}
pub(crate) fn approved_provider(
    a: &AccountView,
    provider: &AccountView,
    m: &Address,
    program: &Address,
) -> ProgramResult {
    let d = profile(a, program, m)?;
    require(
        provider.is_signer() && &d[40..72] == provider.address().as_ref() && d[106] == 1,
        4,
    )
}
pub(crate) fn metadata_payload(p: &[u8]) -> ProgramResult {
    require((33..=32 + URI_MAX).contains(&p.len()), 3)?;
    require(
        core::str::from_utf8(&p[32..]).is_ok()
            && p[32..]
                .iter()
                .all(|b| !b.is_ascii_control() && !b.is_ascii_whitespace()),
        3,
    )
}
pub(crate) fn write_metadata(d: &mut [u8], hash: usize, uri: usize, p: &[u8]) {
    d[hash..hash + 32].copy_from_slice(&p[..32]);
    d[hash + 32..hash + 34].copy_from_slice(&((p.len() - 32) as u16).to_le_bytes());
    let end = uri + p.len() - 32;
    d[uri..end].copy_from_slice(&p[32..]);
    d[end..].fill(0);
}
pub(crate) fn initialize_pda(
    a: &AccountView,
    payer: &AccountView,
    program: &Address,
    len: usize,
    signer: Signer<'_, '_>,
) -> ProgramResult {
    writable(&[a, payer])?;
    require(payer.is_signer(), 4)?;
    require(a.owner() == &pinocchio_system::ID && a.data_len() == 0, 2)?;
    let rent = Rent::get()?.try_minimum_balance(len)?;
    if a.lamports() < rent {
        Transfer {
            from: payer,
            to: a,
            lamports: rent - a.lamports(),
        }
        .invoke()?;
    }
    Allocate {
        account: a,
        space: len as u64,
    }
    .invoke_signed(core::slice::from_ref(&signer))?;
    Assign {
        account: a,
        owner: program,
    }
    .invoke_signed(&[signer])
}
pub(crate) fn book<'a>(
    a: &'a AccountView,
    program: &Address,
    model: &AccountView,
    md: &[u8],
    terms: bool,
) -> Result<Ref<'a, [u8]>, ProgramError> {
    let limit = rd32(md, 136) as usize;
    require((1..=MAX_ENTRIES).contains(&limit), 2)?;
    let (offset, len, magic) = if terms {
        (72, terms_book_len(limit), TERMS_MAGIC)
    } else {
        (104, capacity_book_len(limit), CAPACITY_MAGIC)
    };
    require(&md[offset..offset + 32] == a.address().as_ref(), 2)?;
    let d = state(a, program, len, magic)?;
    require(
        &d[8..40] == model.address().as_ref()
            && rd32(&d, 40) as usize == limit
            && rd32(&d, 44) as usize <= limit,
        2,
    )?;
    Ok(d)
}
pub(crate) fn slot_offset(d: &[u8], slot: u32, terms: bool) -> Result<usize, ProgramError> {
    require(slot < rd32(d, 44), 2)?;
    Ok(HEADER_LEN + slot as usize * if terms { TERMS_LEN } else { CAPACITY_LEN })
}
pub(crate) fn user<'a>(
    a: &'a AccountView,
    program: &Address,
    owner: &Address,
    m: &Address,
) -> Result<Ref<'a, [u8]>, ProgramError> {
    let d = state(a, program, USER_LEN, USER_MAGIC)?;
    require(&d[8..40] == owner.as_ref() && &d[40..72] == m.as_ref(), 2)?;
    Ok(d)
}
pub(crate) fn booking<'a>(
    a: &'a AccountView,
    program: &Address,
    m: &Address,
    model: &Address,
) -> Result<Ref<'a, [u8]>, ProgramError> {
    let d = state(a, program, BOOKING_LEN, BOOK_MAGIC)?;
    require(
        &d[72..104] == m.as_ref() && &d[224..256] == model.as_ref(),
        2,
    )?;
    Ok(d)
}
pub(crate) fn booked_user<'a>(
    u: &'a AccountView,
    program: &Address,
    bd: &[u8],
    m: &Address,
) -> Result<Ref<'a, [u8]>, ProgramError> {
    require(&bd[192..224] == u.address().as_ref(), 2)?;
    user(u, program, &addr(bd, 8), m)
}

pub(crate) fn rates(p: &[u8]) -> ProgramResult {
    require(
        rd32(p, 0) > 0
            && rd32(p, 8) > 0
            && rd32(p, 4) <= rd32(p, 0)
            && rd16(p, 12) > 0
            && p[14] <= 1
            && p[15] <= 1
            && rd32(p, 16) > 0,
        3,
    )
}
