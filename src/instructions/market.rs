use crate::{args::Metadata, helpers::require, state_types::MAX_ENTRIES};
use anchor_lang::prelude::{Address, ProgramResult};
use anchor_lang::AnchorAccount;

pub(crate) fn initialize(
    ctx: &mut anchor_lang::Context<crate::contexts::InitializeMarket>,
) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    require(
        accounts.mint.is_initialized() && accounts.mint.decimals() == 6,
        crate::RouterError::InvalidAccount,
    )?;
    accounts.market.admin = *accounts.admin.address();
    accounts.market.mint = *accounts.mint.address();
    Ok(())
}

pub(crate) fn initialize_model(
    ctx: &mut anchor_lang::Context<crate::contexts::InitializeModel>,
    model_id: Address,
    limit: u32,
    metadata_input: &Metadata,
) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    metadata_input.validate()?;
    let limit = limit as usize;
    require(
        (1..=MAX_ENTRIES).contains(&limit)
            && accounts.terms.capacity() == limit
            && accounts.capacity.capacity() == limit,
        crate::RouterError::InvalidArgument,
    )?;
    let bump = ctx.bumps.model;
    let model = &mut accounts.model;
    model.market = *accounts.market.address();
    model.model_id = model_id;
    model.terms = *accounts.terms.address();
    model.capacity = *accounts.capacity.address();
    model.limit = (limit as u32).into();
    model.bump = bump;

    accounts.terms.model = *accounts.model.address();
    accounts.terms.limit = (limit as u32).into();
    accounts.capacity.model = *accounts.model.address();
    accounts.capacity.limit = (limit as u32).into();

    let metadata_bump = ctx.bumps.metadata;
    let metadata = &mut accounts.metadata;
    metadata.market = *accounts.market.address();
    metadata.target = *accounts.model.address();
    metadata.authority = *accounts.provider.address();
    metadata.bump = metadata_bump;
    metadata.document_hash = metadata_input.document_hash;
    metadata.uri_len = (metadata_input.uri.len() as u16).into();
    metadata.uri[..metadata_input.uri.len()].copy_from_slice(metadata_input.uri.as_bytes());
    Ok(())
}

pub(crate) fn update_metadata(
    ctx: &mut anchor_lang::Context<crate::contexts::UpdateModelMetadata>,
    metadata_input: &Metadata,
) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    metadata_input.validate()?;
    accounts.metadata.document_hash = metadata_input.document_hash;
    accounts.metadata.uri_len = (metadata_input.uri.len() as u16).into();
    accounts.metadata.uri[..metadata_input.uri.len()]
        .copy_from_slice(metadata_input.uri.as_bytes());
    accounts.metadata.uri[metadata_input.uri.len()..].fill(0);
    Ok(())
}

pub(crate) fn grow(
    ctx: &mut anchor_lang::Context<crate::contexts::GrowBooks>,
    limit: u32,
) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    let old = accounts.model.limit.get() as usize;
    let limit = limit as usize;
    require(
        limit > old && limit <= MAX_ENTRIES && limit - old <= 85,
        crate::RouterError::InvalidArgument,
    )?;
    accounts.terms.resize_to_capacity(limit as u32)?;
    accounts.capacity.resize_to_capacity(limit as u32)?;
    accounts.terms.top_up(accounts.payer.account())?;
    accounts.capacity.top_up(accounts.payer.account())?;
    accounts.terms.limit = (limit as u32).into();
    accounts.capacity.limit = (limit as u32).into();
    accounts.model.limit = (limit as u32).into();
    Ok(())
}
