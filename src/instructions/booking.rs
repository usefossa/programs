use crate::{
    args::{Claim, ReserveRequest},
    helpers::{add, bps_ceil, dec, fee, fee_budget, inc, require, sub},
    matching::select,
    receipt,
    state_types::{Booking, Capacity, CapacityEntry, User},
    token::{token_owned_by, user_payments, user_transfer, vault_owned_by},
    CLAIM_SECONDS,
};
use anchor_lang::accounts::Slab;
use anchor_lang::prelude::{Account, Clock, ProgramError, ProgramResult};
use anchor_lang::solana_program::sysvar::Sysvar as _;
use anchor_lang::{AccountClose, AnchorAccount};

fn capacity<'a>(
    book: &'a mut Slab<Capacity, CapacityEntry>,
    slot: u32,
) -> Result<&'a mut CapacityEntry, ProgramError> {
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
    if fee_bps > 0 {
        super::fees::validate_terms(fee_bps, protocol_share)?;
    } else {
        require(protocol_share == 0, crate::RouterError::InvalidAccount)?;
    }
    let mut request_state = request.validate(accounts.user.free.get())?;
    request_state.spend = fee_budget(request_state.spend, fee_bps);
    let mut chosen = select(
        accounts.terms.as_slice(),
        accounts.capacity.as_slice(),
        accounts.list.as_slice(),
        &request_state,
    )?;
    chosen.reserve = add(chosen.reserve, bps_ceil(chosen.reserve, fee_bps))?;
    chosen.score = add(chosen.score, bps_ceil(chosen.score, fee_bps))?;
    let terms = *accounts
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
            request_state
                .max_output
                .div_ceil(u32::from(terms.speed.get().max(1))),
        );
    require(claim_seconds <= 86_400, crate::RouterError::InvalidArgument)?;
    let claim_end = Clock::get()?
        .unix_timestamp
        .checked_add(claim_seconds)
        .ok_or(crate::RouterError::Arithmetic)?;
    let reserve_destination = accounts.model.route_to_reserve == 1;
    accounts.booking.owner = *accounts.owner.address();
    accounts.booking.provider = terms.provider;
    accounts.booking.market = *accounts.market.address();
    accounts.booking.reserve = chosen.reserve.into();
    accounts.booking.input_rate = terms.input_rate;
    accounts.booking.cached_rate = terms.cached_rate;
    accounts.booking.output_rate = terms.output_rate;
    accounts.booking.max_input = request_state.max_input.into();
    accounts.booking.max_output = request_state.max_output.into();
    accounts.booking.claim_end = claim_end.into();
    accounts.booking.reserved = [0; 8];
    accounts.booking.status = 0;
    accounts.booking.reserved2 = [0; 3];
    accounts.booking.generation = generation.into();
    accounts.booking.charge = 0.into();
    accounts.booking.input = 0.into();
    accounts.booking.cached = 0.into();
    accounts.booking.output = 0.into();
    accounts.booking.min_speed = request_state.min_speed.into();
    accounts.booking.reserved3 = [0; 10];
    accounts.booking.user = *accounts.user.address();
    accounts.booking.model = *accounts.model.address();
    accounts.booking.rent_payer = *accounts.rent_payer.address();
    accounts.booking.payout = terms.payout;
    accounts.booking.provider_list = *accounts.list.address();
    accounts.booking.slot = chosen.slot.into();
    accounts.booking.reserved4 = 0;
    accounts.booking.requested_privacy = u8::from(request_state.private);
    accounts.booking.cache_allowed = u8::from(chosen.cache);
    accounts.booking.reserved5 = 0;
    accounts.booking.estimated_score = chosen.score.into();
    accounts.booking.creator = accounts.list.authority;
    accounts.booking.protocol_destination = if reserve_destination {
        accounts.model.backing_vault
    } else {
        accounts.market.treasury
    };
    accounts.booking.fee_bps = fee_bps.into();
    accounts.booking.protocol_share = protocol_share.into();
    accounts.booking.reserve_destination = u8::from(reserve_destination);
    accounts.booking.reserved6 = [0; 3];
    Ok(())
}

#[inline(always)]
pub(crate) fn claim(
    ctx: &mut anchor_lang::Context<crate::contexts::ClaimBooking>,
    claim: &Claim,
) -> ProgramResult {
    let mut remaining = ctx.remaining_accounts()?;
    let destinations = remaining.as_mut_slice();
    let accounts = &mut ctx.accounts;
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
            && accounts.booking.reserved == [0; 8]
            && accounts.booking.reserved4 == 0
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
    vault_owned_by(
        accounts.vault.account(),
        accounts.mint.address(),
        accounts.authority.address(),
    )?;
    if !destinations.is_empty() {
        super::fees::validate_terms(
            accounts.booking.fee_bps.get(),
            accounts.booking.protocol_share.get(),
        )?;
        token_owned_by(
            &destinations[0],
            accounts.mint.address(),
            &accounts.booking.creator,
        )?;
        if accounts.booking.reserve_destination == 1 {
            require(
                accounts.booking.protocol_destination == *destinations[1].address(),
                crate::RouterError::InvalidAccount,
            )?;
            vault_owned_by(
                &destinations[1],
                accounts.mint.address(),
                accounts.model.address(),
            )?;
        } else {
            token_owned_by(
                &destinations[1],
                accounts.mint.address(),
                &accounts.booking.protocol_destination,
            )?;
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
    let capacity = capacity(&mut accounts.capacity, accounts.booking.slot.get())?;
    capacity.active = dec(capacity.active.get())?.into();
    if !destinations.is_empty() {
        user_payments(
            [
                accounts.vault.account(),
                accounts.mint.account(),
                accounts.authority.account(),
            ],
            [
                (accounts.payout.account(), provider_charge),
                (&destinations[0], creator_fee),
                (&destinations[1], protocol_fee),
            ],
            accounts.user.address(),
            accounts.user.bump,
            ctx.program_id,
        )?;
    } else if provider_charge > 0 {
        user_transfer(
            [
                accounts.vault.account(),
                accounts.mint.account(),
                accounts.payout.account(),
                accounts.authority.account(),
            ],
            accounts.user.address(),
            accounts.user.bump,
            ctx.program_id,
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
    capacity_book: &mut Slab<Capacity, CapacityEntry>,
    user_state: &mut Account<User>,
    booking: &mut Account<Booking>,
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
    let entry = capacity(capacity_book, booking.slot.get())?;
    entry.active = dec(entry.active.get())?.into();
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
    let accounts = &mut ctx.accounts;
    release_core(
        &mut accounts.capacity,
        &mut accounts.user,
        &mut accounts.booking,
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
    let accounts = &mut ctx.accounts;
    release_core(
        &mut accounts.capacity,
        &mut accounts.user,
        &mut accounts.booking,
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
