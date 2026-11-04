use crate::{
    args::{Metadata, Offer},
    helpers::require,
    state_types::{CapacityEntry, LegacyTermsEntry as TermsEntry, OFFER_VERSION},
};
use anchor_lang::prelude::ProgramResult;
use anchor_lang::AnchorAccount;

#[inline(always)]
pub(crate) fn register_offer(
    ctx: &mut anchor_lang::Context<crate::contexts::RegisterOffer>,
    offer: &Offer,
) -> ProgramResult {
    require(
        ctx.accounts.market.reserved[0] == 0,
        crate::RouterError::InvalidState,
    )?;
    crate::helpers::require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let model = &ctx.accounts.model;
    let terms = &mut ctx.accounts.terms;
    let capacity = &mut ctx.accounts.capacity;
    let provider = ctx.accounts.provider.account();
    let payout = ctx.accounts.payout.account();
    offer.validate()?;
    let slot = offer.slot;
    require(
        terms.limit.get() == model.limit.get()
            && capacity.limit.get() == model.limit.get()
            && terms.len() == capacity.len()
            && slot as usize == terms.len()
            && slot < terms.limit.get(),
        crate::RouterError::InvalidAccount,
    )?;
    require(
        !terms
            .iter()
            .any(|entry| entry.provider == *provider.address()),
        crate::RouterError::Unavailable,
    )?;
    terms.try_push(TermsEntry {
        provider: *provider.address(),
        payout: *payout.address(),
        checkpoint: offer.checkpoint,
        input_rate: offer.input_rate.into(),
        cached_rate: offer.cached_rate.into(),
        output_rate: offer.output_rate.into(),
        speed: offer.speed.into(),
        privacy: u8::from(offer.privacy),
        active: u8::from(offer.active),
        input_modalities: offer.input_modalities,
        output_modalities: offer.output_modalities,
        version: OFFER_VERSION,
        reserved: 0,
        capacity: offer.capacity.into(),
        context_window: offer.context_window.into(),
        max_input: offer.max_input.into(),
        max_output: offer.max_output.into(),
    })?;
    capacity.try_push(CapacityEntry {
        reserved: [0; 16],
        active: 0.into(),
        reserved2: [0; 12],
    })?;
    Ok(())
}

#[inline(always)]
pub(crate) fn update_offer(
    ctx: &mut anchor_lang::Context<crate::contexts::UpdateOffer>,
    offer: &Offer,
) -> ProgramResult {
    crate::helpers::require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let terms = &mut ctx.accounts.terms;
    let signer = ctx.accounts.signer.account();
    offer.validate()?;
    let entry = terms
        .get_mut(offer.slot as usize)
        .ok_or(crate::RouterError::InvalidAccount)?;
    require(
        entry.provider == *signer.address(),
        crate::RouterError::Unauthorized,
    )?;
    entry.input_rate = offer.input_rate.into();
    entry.cached_rate = offer.cached_rate.into();
    entry.output_rate = offer.output_rate.into();
    entry.speed = offer.speed.into();
    entry.privacy = u8::from(offer.privacy);
    entry.active = u8::from(offer.active);
    entry.capacity = offer.capacity.into();
    entry.checkpoint = offer.checkpoint;
    entry.input_modalities = offer.input_modalities;
    entry.output_modalities = offer.output_modalities;
    entry.version = OFFER_VERSION;
    entry.reserved = 0;
    entry.context_window = offer.context_window.into();
    entry.max_input = offer.max_input.into();
    entry.max_output = offer.max_output.into();
    Ok(())
}

#[inline(always)]
pub(crate) fn initialize(
    ctx: &mut anchor_lang::Context<crate::contexts::InitializeProvider>,
    metadata: &Metadata,
) -> ProgramResult {
    crate::helpers::require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let market = &ctx.accounts.market;
    let profile = &mut ctx.accounts.profile;
    let provider = ctx.accounts.provider.account();
    metadata.validate()?;
    let bump = ctx.bumps.profile;
    profile.market = *market.address();
    profile.authority = *provider.address();
    profile.bump = bump;
    profile.document_hash = metadata.document_hash;
    profile.uri_len = (metadata.uri.len() as u16).into();
    profile.uri[..metadata.uri.len()].copy_from_slice(metadata.uri.as_bytes());
    Ok(())
}

#[inline(always)]
pub(crate) fn update_profile(
    ctx: &mut anchor_lang::Context<crate::contexts::UpdateProfile>,
    metadata: &Metadata,
) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    metadata.validate()?;
    accounts.profile.document_hash = metadata.document_hash;
    accounts.profile.uri_len = (metadata.uri.len() as u16).into();
    accounts.profile.uri[..metadata.uri.len()].copy_from_slice(metadata.uri.as_bytes());
    accounts.profile.uri[metadata.uri.len()..].fill(0);
    Ok(())
}
