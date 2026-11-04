use crate::{
    args::Metadata,
    helpers::require,
    state_types::{List, ListMetadata, URI_MAX},
};
use anchor_lang::accounts::Slab;
use anchor_lang::prelude::{Account, AccountView, Address, ProgramResult};
use anchor_lang::{AnchorAccount, Space};

pub(crate) fn validate(
    metadata: &AccountView,
    list: &Slab<List, Address>,
    _program: &Address,
    bump: u8,
) -> ProgramResult {
    let state = Account::<ListMetadata>::load(*metadata)?;
    let len = state.uri_len.get() as usize;
    require(
        state.account().data_len() == Account::<ListMetadata>::INIT_SPACE
            && state.market == list.market
            && state.target == *list.address()
            && state.authority == list.authority
            && state.bump == bump
            && state.reserved == 0
            && (1..=URI_MAX).contains(&len),
        crate::RouterError::InvalidAccount,
    )?;
    let uri = &state.uri[..len];
    require(
        core::str::from_utf8(uri).is_ok()
            && uri
                .iter()
                .all(|byte| !byte.is_ascii_control() && !byte.is_ascii_whitespace())
            && state.uri[len..].iter().all(|byte| *byte == 0),
        crate::RouterError::InvalidAccount,
    )
}

pub(crate) fn initialize(
    ctx: &mut anchor_lang::Context<crate::contexts::InitializeListMetadata>,
    metadata_input: &Metadata,
) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    metadata_input.validate()?;
    super::list::state(&accounts.list)?;
    let bump = ctx.bumps.metadata;
    let metadata = &mut accounts.metadata;
    metadata.market = *accounts.market.address();
    metadata.target = *accounts.list.address();
    metadata.authority = *accounts.authority.address();
    metadata.bump = bump;
    metadata.document_hash = metadata_input.document_hash;
    metadata.uri_len = (metadata_input.uri.len() as u16).into();
    metadata.uri[..metadata_input.uri.len()].copy_from_slice(metadata_input.uri.as_bytes());
    Ok(())
}

pub(crate) fn update(
    ctx: &mut anchor_lang::Context<crate::contexts::UpdateListMetadata>,
    metadata_input: &Metadata,
) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    metadata_input.validate()?;
    super::list::state(&accounts.list)?;
    accounts.metadata.document_hash = metadata_input.document_hash;
    accounts.metadata.uri_len = (metadata_input.uri.len() as u16).into();
    accounts.metadata.uri[..metadata_input.uri.len()]
        .copy_from_slice(metadata_input.uri.as_bytes());
    accounts.metadata.uri[metadata_input.uri.len()..].fill(0);
    Ok(())
}
