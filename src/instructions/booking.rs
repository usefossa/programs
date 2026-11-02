use crate::{
    args::{Claim, ReserveRequest},
    helpers::{add, bps_ceil, fee, fee_budget, inc, require, sub},
    matching::select,
    receipt,
    state_types::{Booking, Capacity, CapacityEntry, Model, User},
    token::{token_owned_by, user_payments, user_transfer, vault_owned_by},
    CLAIM_SECONDS,
};
use anchor_lang::accounts::Slab;
use anchor_lang::prelude::{
    Account, Address, Clock, ProgramError, ProgramResult, UncheckedAccount,
};
use anchor_lang::solana_program::sysvar::Sysvar as _;
use anchor_lang::{AccountClose, AnchorAccount, ToCpiHandle, ToCpiHandleMut};

fn capacity(
    book: &mut Slab<Capacity, CapacityEntry>,
    slot: u32,
) -> Result<&mut CapacityEntry, ProgramError> {
    require(
        book.len() <= book.limit.get() as usize,
        crate::RouterError::InvalidAccount,
    )?;
    book.get_mut(slot as usize)
        .ok_or(crate::RouterError::InvalidAccount.into())
}

#[inline(always)]
pub(crate) fn reserve(
    ctx: &mut anchor_lang::Context<crate::contexts::ReserveBooking>,
    request: &ReserveRequest,
    generation: Option<u64>,
) -> ProgramResult {
    require(
        ctx.accounts.market.reserved[0] == 0,
        crate::RouterError::InvalidState,
    )?;
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    let generation = receipt::prepare_booking(
        &accounts.booking,
        accounts.owner.address(),
        accounts.market.address(),
        accounts.user.address(),
        accounts.rent_payer.address(),
        generation,
    )?;
    require(
        generation == 0 || generation == add(accounts.user.generation.get(), 1)?,
        crate::RouterError::InvalidState,
    )?;
    require(
        accounts.model.model_id == request.model_id,
        crate::RouterError::InvalidAccount,
    )?;
    super::list::state(&accounts.list)?;
    let fee_bps = accounts.list.fee_bps.get();
    let protocol_share = accounts.list.protocol_share.get();
    super::fees::validate_terms(fee_bps, protocol_share)?;
    require(
        accounts.group.is_some() == (accounts.model.group != Address::default()),
        crate::RouterError::InvalidAccount,
    )?;
    let reserve = accounts
        .group
        .as_ref()
        .filter(|group| group.route_to_reserve == 1);
    let reserve_authority = reserve.map(|group| *group.address()).unwrap_or_default();
    let protocol_destination = reserve
        .map(|group| group.backing_vault)
        .unwrap_or(accounts.market.council);
    let spend = fee_budget(request.validate(accounts.user.free.get())?, fee_bps);
    let mut chosen = select(
        accounts.terms.as_slice(),
        accounts.capacity.as_slice(),
        accounts.list.as_slice(),
        request,
        spend,
    )?;
    chosen.reserve = add(chosen.reserve, bps_ceil(chosen.reserve, fee_bps))?;
    chosen.score = add(chosen.score, bps_ceil(chosen.score, fee_bps))?;
    let terms = accounts
        .terms
        .get(chosen.slot as usize)
        .ok_or(crate::RouterError::InvalidAccount)?;
    let capacity = capacity(&mut accounts.capacity, chosen.slot)?;
    capacity.active = inc(capacity.active.get())?.into();
    accounts.user.free = sub(accounts.user.free.get(), chosen.reserve)?.into();
    if generation > 0 {
        accounts.user.generation = generation.into();
    }
    let claim_seconds = CLAIM_SECONDS
        + i64::from(
            request
                .max_output
                .div_ceil(u32::from(terms.speed.get().max(1))),
        );
    require(claim_seconds <= 86_400, crate::RouterError::InvalidArgument)?;
    let claim_end = Clock::get()?
        .unix_timestamp
        .checked_add(claim_seconds)
        .ok_or(crate::RouterError::Arithmetic)?;
    let reserve_destination = reserve.is_some();
    accounts.booking.owner = *accounts.owner.address();
    accounts.booking.provider = terms.provider;
    accounts.booking.market = *accounts.market.address();
    accounts.booking.reserve = chosen.reserve.into();
    accounts.booking.input_rate = terms.input_rate;
    accounts.booking.cached_rate = terms.cached_rate;
    accounts.booking.output_rate = terms.output_rate;
    accounts.booking.max_input = request.max_input.into();
    accounts.booking.max_output = request.max_output.into();
    accounts.booking.claim_end = claim_end.into();
    accounts.booking.provider_policy = 0.into();
    accounts.booking.reserved = [0; 4];
    accounts.booking.status = 0;
    accounts.booking.reserved2 = [0; 3];
    accounts.booking.generation = generation.into();
    accounts.booking.charge = 0.into();
    accounts.booking.input = 0.into();
    accounts.booking.cached = 0.into();
    accounts.booking.output = 0.into();
    accounts.booking.min_speed = request.min_speed.into();
    accounts.booking.reserved3 = [0; 10];
    accounts.booking.user = *accounts.user.address();
    accounts.booking.model = *accounts.model.address();
    accounts.booking.rent_payer = *accounts.rent_payer.address();
    accounts.booking.payout = terms.payout;
    accounts.booking.provider_list = *accounts.list.address();
    accounts.booking.slot = chosen.slot.into();
    accounts.booking.wallet_booking_counted = 0;
    accounts.booking.requested_privacy = u8::from(request.private);
    accounts.booking.cache_allowed = u8::from(chosen.cache);
    accounts.booking.reserved5 = 0;
    accounts.booking.estimated_score = chosen.score.into();
    accounts.booking.creator = accounts.list.authority;
    accounts.booking.protocol_destination = protocol_destination;
    accounts.booking.reserve_authority = reserve_authority;
    accounts.booking.fee_bps = fee_bps.into();
    accounts.booking.protocol_share = protocol_share.into();
    accounts.booking.reserve_destination = u8::from(reserve_destination);
    accounts.booking.reserved6 = [0; 3];
    accounts.booking.checkpoint = terms.checkpoint;
    accounts.booking.offer_context_window = terms.context_window;
    accounts.booking.offer_max_input = terms.max_input;
    accounts.booking.offer_max_output = terms.max_output;
    accounts.booking.offer_input_modalities = terms.input_modalities;
    accounts.booking.offer_output_modalities = terms.output_modalities;
    accounts.booking.requested_input_modalities = request.input_modalities;
    accounts.booking.requested_output_modalities = request.output_modalities;
    Ok(())
}

pub(crate) fn claim(
    ctx: &mut anchor_lang::Context<crate::contexts::ClaimBooking>,
    claim: &Claim,
) -> ProgramResult {
    require(
        ctx.accounts.booking.wallet_booking_counted == 0,
        crate::RouterError::InvalidState,
    )?;
    let mut remaining = ctx.remaining_accounts()?;
    claim_core(
        &mut ctx.accounts,
        None,
        remaining.as_mut_slice(),
        ctx.program_id,
        claim,
    )
}

pub(crate) fn claim_list(
    ctx: &mut anchor_lang::Context<crate::contexts::ClaimListBooking>,
    claim: &Claim,
) -> ProgramResult {
    require(
        ctx.accounts.base.booking.wallet_booking_counted > 0,
        crate::RouterError::InvalidState,
    )?;
    let mut remaining = ctx.remaining_accounts()?;
    claim_core(
        &mut ctx.accounts.base,
        Some(&mut ctx.accounts.wallet_bookings),
        remaining.as_mut_slice(),
        ctx.program_id,
        claim,
    )
}

#[inline(always)]
fn claim_core(
    accounts: &mut crate::contexts::ClaimBooking,
    wallet_bookings: Option<&mut UncheckedAccount>,
    destinations: &mut [anchor_lang::prelude::AccountView],
    program_id: &Address,
    claim: &Claim,
) -> ProgramResult {
    require(
        destinations.len()
            == if accounts.booking.fee_bps.get() > 0 {
                2
            } else {
                0
            },
        crate::RouterError::InvalidArgument,
    )?;
    require(
        accounts.booking.status == 0
            && accounts.booking.reserved == [0; 4]
            && accounts.booking.wallet_booking_counted <= 2
            && accounts.booking.reserved5 == 0
            && Clock::get()?.unix_timestamp <= accounts.booking.claim_end.get(),
        crate::RouterError::InvalidState,
    )?;
    let generation = accounts.booking.generation.get();
    require(
        if generation == 0 {
            claim.receipt.is_none()
        } else {
            claim
                .receipt
                .map(|receipt| receipt.generation == generation)
                .unwrap_or(false)
        },
        crate::RouterError::InvalidState,
    )?;
    if !destinations.is_empty() {
        super::fees::validate_terms(
            accounts.booking.fee_bps.get(),
            accounts.booking.protocol_share.get(),
        )?;
        let destination_owner = |destination: &anchor_lang::prelude::AccountView,
                                 owner: &Address| {
            if destination.address() == accounts.payout.address() {
                require(
                    accounts.payout.owner() == owner,
                    crate::RouterError::InvalidAccount,
                )
            } else {
                token_owned_by(destination, accounts.mint.address(), owner)
            }
        };
        destination_owner(&destinations[0], &accounts.booking.creator)?;
        if accounts.booking.reserve_destination == 1 {
            require(
                accounts.booking.protocol_destination == *destinations[1].address(),
                crate::RouterError::InvalidAccount,
            )?;
            vault_owned_by(
                &destinations[1],
                accounts.mint.address(),
                &accounts.booking.reserve_authority,
            )?;
        } else {
            destination_owner(&destinations[1], &accounts.booking.protocol_destination)?;
        }
    }
    let uncached = claim.uncached;
    let cached = claim.cached;
    let output = claim.output;
    require(
        u64::from(uncached) + u64::from(cached) <= u64::from(accounts.booking.max_input.get())
            && output <= accounts.booking.max_output.get()
            && (cached == 0 || accounts.booking.cache_allowed == 1),
        crate::RouterError::LimitExceeded,
    )?;
    let provider_charge = fee(uncached, accounts.booking.input_rate.get())
        + fee(cached, accounts.booking.cached_rate.get())
        + fee(output, accounts.booking.output_rate.get());
    let list_fee = bps_ceil(provider_charge, accounts.booking.fee_bps.get());
    let protocol_fee = bps_ceil(list_fee, accounts.booking.protocol_share.get());
    let creator_fee = list_fee - protocol_fee;
    let charge = add(provider_charge, list_fee)?;
    require(
        charge <= accounts.booking.reserve.get(),
        crate::RouterError::LimitExceeded,
    )?;
    accounts.user.free = add(
        accounts.user.free.get(),
        accounts.booking.reserve.get() - charge,
    )?
    .into();
    super::scoped::settle(
        &mut accounts.capacity,
        wallet_bookings,
        &accounts.model,
        &accounts.booking,
    )?;
    if !destinations.is_empty() {
        let payout = accounts.payout.try_to_cpi_handle_mut()?;
        let (creator, protocol) = destinations.split_at_mut(1);
        let creator = if creator[0].address() == payout.address() {
            payout
        } else {
            creator[0].try_to_cpi_handle_mut()?
        };
        let protocol = if protocol[0].address() == payout.address() {
            payout
        } else {
            protocol[0].try_to_cpi_handle_mut()?
        };
        user_payments(
            (
                accounts.vault.try_to_cpi_handle_mut()?,
                accounts.mint.to_cpi_handle(),
                accounts.authority.to_cpi_handle(),
            ),
            [
                (payout, provider_charge),
                (creator, creator_fee),
                (protocol, protocol_fee),
            ],
            accounts.user.address(),
            accounts.user.bump,
            program_id,
        )?;
    } else if provider_charge > 0 {
        user_transfer(
            (
                accounts.vault.try_to_cpi_handle_mut()?,
                accounts.mint.to_cpi_handle(),
                accounts.payout.try_to_cpi_handle_mut()?,
                accounts.authority.to_cpi_handle(),
            ),
            accounts.user.address(),
            accounts.user.bump,
            program_id,
            provider_charge,
        )?;
    }
    accounts.booking.status = if generation == 0 {
        4
    } else {
        receipt::AVAILABLE
    };
    accounts.booking.charge = charge.into();
    accounts.booking.input = uncached.into();
    accounts.booking.cached = cached.into();
    accounts.booking.output = output.into();
    if generation > 0 {
        receipt::settled(
            &accounts.booking,
            charge,
            claim.receipt.ok_or(crate::RouterError::InvalidArgument)?,
        )?;
    }
    Ok(())
}

fn release_core(
    capacity_book: &mut UncheckedAccount,
    model: &Account<Model>,
    user_state: &mut Account<User>,
    booking: &mut Account<Booking>,
    wallet_bookings: Option<&mut UncheckedAccount>,
    provider_release: bool,
    requested_generation: Option<u64>,
) -> ProgramResult {
    let generation = booking.generation.get();
    require(
        if generation == 0 {
            requested_generation.is_none()
        } else {
            requested_generation == Some(generation)
        },
        crate::RouterError::InvalidState,
    )?;
    require(booking.status == 0, crate::RouterError::InvalidState)?;
    if !provider_release {
        require(
            Clock::get()?.unix_timestamp > booking.claim_end.get(),
            crate::RouterError::InvalidState,
        )?;
    }
    super::scoped::settle(capacity_book, wallet_bookings, model, booking)?;
    user_state.free = add(user_state.free.get(), booking.reserve.get())?.into();
    booking.status = if generation == 0 {
        3
    } else {
        receipt::AVAILABLE
    };
    Ok(())
}

pub(crate) fn expire(
    ctx: &mut anchor_lang::Context<crate::contexts::ExpireBooking>,
    generation: Option<u64>,
) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let a = &mut ctx.accounts;
    require(
        a.booking.wallet_booking_counted == 0,
        crate::RouterError::InvalidState,
    )?;
    release_core(
        &mut a.capacity,
        &a.model,
        &mut a.user,
        &mut a.booking,
        None,
        false,
        generation,
    )
}

pub(crate) fn expire_list(
    ctx: &mut anchor_lang::Context<crate::contexts::ExpireListBooking>,
    generation: Option<u64>,
) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let a = &mut ctx.accounts;
    require(
        a.base.booking.wallet_booking_counted > 0,
        crate::RouterError::InvalidState,
    )?;
    let base = &mut *a.base;
    release_core(
        &mut base.capacity,
        &base.model,
        &mut base.user,
        &mut base.booking,
        Some(&mut a.wallet_bookings),
        false,
        generation,
    )
}

pub(crate) fn release(
    ctx: &mut anchor_lang::Context<crate::contexts::ReleaseBooking>,
    generation: Option<u64>,
) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let a = &mut ctx.accounts;
    require(
        a.booking.wallet_booking_counted == 0,
        crate::RouterError::InvalidState,
    )?;
    release_core(
        &mut a.capacity,
        &a.model,
        &mut a.user,
        &mut a.booking,
        None,
        true,
        generation,
    )
}

pub(crate) fn release_list(
    ctx: &mut anchor_lang::Context<crate::contexts::ReleaseListBooking>,
    generation: Option<u64>,
) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let a = &mut ctx.accounts;
    require(
        a.base.booking.wallet_booking_counted > 0,
        crate::RouterError::InvalidState,
    )?;
    let base = &mut *a.base;
    release_core(
        &mut base.capacity,
        &base.model,
        &mut base.user,
        &mut base.booking,
        Some(&mut a.wallet_bookings),
        true,
        generation,
    )
}

#[inline(always)]
pub(crate) fn close(
    ctx: &mut anchor_lang::Context<crate::contexts::CloseBooking>,
) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    if accounts.booking.status == receipt::AVAILABLE {
        require(
            accounts.recipient.is_signer(),
            crate::RouterError::Unauthorized,
        )?;
    }
    require(
        matches!(accounts.booking.status, 3 | 4 | receipt::AVAILABLE),
        crate::RouterError::InvalidState,
    )?;
    accounts.booking.close(*accounts.recipient.account())
}
