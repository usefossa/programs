use crate::{
    abi::{LIST_METADATA_LEN, LIST_METADATA_MAGIC, URI_MAX},
    helpers::{err, rd16, require},
    state::{initialize_pda, market, metadata_payload, state, writable, write_metadata},
};
use pinocchio::{
    cpi::Signer, error::ProgramError, instruction::seeds, AccountView, Address, ProgramResult,
};

pub(crate) fn canonical(
    metadata: &AccountView,
    list: &AccountView,
    program: &Address,
) -> Result<u8, ProgramError> {
    let (address, bump) =
        Address::derive_program_address(&[b"list_metadata", list.address().as_ref()], program)
            .ok_or(err(2))?;
    require(metadata.address() == &address, 2)?;
    Ok(bump)
}

pub(crate) fn validate(
    metadata: &AccountView,
    list: &AccountView,
    list_data: &[u8],
    program: &Address,
    bump: u8,
) -> ProgramResult {
    let d = state(metadata, program, LIST_METADATA_LEN, LIST_METADATA_MAGIC)?;
    let len = rd16(&d, 136) as usize;
    require(
        d[8..40] == list_data[8..40]
            && &d[40..72] == list.address().as_ref()
            && d[72..104] == list_data[40..72]
            && d[138] == bump
            && d[139] == 0
            && (1..=URI_MAX).contains(&len),
        2,
    )?;
    let uri = &d[140..140 + len];
    require(
        core::str::from_utf8(uri).is_ok()
            && uri
                .iter()
                .all(|b| !b.is_ascii_control() && !b.is_ascii_whitespace())
            && d[140 + len..].iter().all(|b| *b == 0),
        2,
    )
}

pub(crate) fn initialize(
    program: &Address,
    accounts: &mut [AccountView],
    p: &[u8],
) -> ProgramResult {
    let [m, list, metadata, authority, payer, system] = accounts else {
        return Err(err(3));
    };
    require(system.address() == &pinocchio_system::ID, 3)?;
    metadata_payload(p)?;
    market(m, program)?;
    let ld = super::list::state(list, program)?;
    require(&ld[8..40] == m.address().as_ref(), 2)?;
    require(
        authority.is_signer() && &ld[40..72] == authority.address().as_ref(),
        4,
    )?;
    let bump = canonical(metadata, list, program)?;
    let bump_ref = [bump];
    let signer_seeds = seeds!(b"list_metadata", list.address().as_ref(), &bump_ref);
    initialize_pda(
        metadata,
        payer,
        program,
        LIST_METADATA_LEN,
        Signer::from(&signer_seeds),
    )?;
    let mut d = metadata.try_borrow_mut()?;
    d[..8].copy_from_slice(LIST_METADATA_MAGIC);
    d[8..40].copy_from_slice(m.address().as_ref());
    d[40..72].copy_from_slice(list.address().as_ref());
    d[72..104].copy_from_slice(authority.address().as_ref());
    d[138] = bump;
    write_metadata(&mut d, 104, 140, p);
    Ok(())
}

pub(crate) fn update(program: &Address, accounts: &mut [AccountView], p: &[u8]) -> ProgramResult {
    let [m, list, metadata, authority] = accounts else {
        return Err(err(3));
    };
    metadata_payload(p)?;
    writable(&[metadata])?;
    market(m, program)?;
    let ld = super::list::state(list, program)?;
    require(&ld[8..40] == m.address().as_ref(), 2)?;
    require(
        authority.is_signer() && &ld[40..72] == authority.address().as_ref(),
        4,
    )?;
    let bump = canonical(metadata, list, program)?;
    validate(metadata, list, &ld, program, bump)?;
    write_metadata(&mut metadata.try_borrow_mut()?, 104, 140, p);
    Ok(())
}
