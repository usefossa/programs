use crate::{
    args::{BookingLimits, Offer, ReserveRequest},
    helpers::{add, bps_ceil, dec, fee, fee_budget, inc, require, sub},
    receipt,
    scoped_contexts::*,
    state_types::*,
    RouterError, CLAIM_SECONDS,
};
use anchor_lang::{accounts::Slab, prelude::*, AccountClose, AnchorAccount, Discriminator};

const GROWTH_BYTES: usize = 10_240;
const MAX_ACCOUNT_BYTES: usize = 10 * 1024 * 1024;

pub(crate) fn activate_provider_limits(ctx: &mut Context<ActivateProviderLimits>) -> ProgramResult {
    require(
        ctx.accounts.market.reserved[0] == 1,
        RouterError::InvalidState,
    )?;
    ctx.accounts.market.reserved[0] = 2;
    Ok(())
}

fn growth(old: usize, new: u32, stride: usize, header: usize, maximum: usize) -> ProgramResult {
    let new = new as usize;
    require(
        new > old
            && new <= maximum
            && header + new * stride <= MAX_ACCOUNT_BYTES
            && (new - old) * stride + if old == 0 { header } else { 0 } <= GROWTH_BYTES,
        RouterError::InvalidArgument,
    )
}

pub(crate) fn initialize_provider_limit_page(
    ctx: &mut Context<InitializeProviderLimitPage>,
    limit: u32,
    page: u8,
) -> ProgramResult {
    growth(
        0,
        limit,
        core::mem::size_of::<ProviderLimitEntry>(),
        Slab::<ProviderLimitPage, ProviderLimitEntry>::space_for(0),
        u32::MAX as usize,
    )?;
    require(page < 32, RouterError::InvalidArgument)?;
    ctx.accounts.page.page = page;
    ctx.accounts.page.market = *ctx.accounts.market.address();
    ctx.accounts.page.bump = ctx.bumps.page;
    Ok(())
}

pub(crate) fn grow_provider_limit_page(
    ctx: &mut Context<GrowProviderLimitPage>,
    limit: u32,
) -> ProgramResult {
    let a = &mut ctx.accounts;
    growth(
        a.page.capacity(),
        limit,
        core::mem::size_of::<ProviderLimitEntry>(),
        Slab::<ProviderLimitPage, ProviderLimitEntry>::space_for(0),
        u32::MAX as usize,
    )?;
    a.page.resize_to_capacity(limit)?;
    a.page.top_up(a.payer.account())
}

fn configure_limits(
    entry: &mut ProviderLimitEntry,
    limits: &crate::args::ProviderLimits,
) -> ProgramResult {
    require(
        (1..=86_400).contains(&limits.global.quota_window_seconds)
            && (1..=86_400).contains(&limits.wallet.quota_window_seconds),
        RouterError::InvalidArgument,
    )?;
    // zero means unlimited; the checked counter cannot exceed u32::MAX.
    entry.concurrency_limit = if limits.global.concurrency == 0 {
        u32::MAX
    } else {
        limits.global.concurrency
    }
    .into();
    entry.quota_limit = limits.global.quota.into();
    entry.quota_window_seconds = limits.global.quota_window_seconds.into();
    entry.wallet_concurrency_limit = limits.wallet.concurrency.into();
    entry.wallet_quota_limit = limits.wallet.quota.into();
    entry.wallet_quota_window_seconds = limits.wallet.quota_window_seconds.into();
    Ok(())
}

pub(crate) fn initialize_provider_limits(
    ctx: &mut Context<InitializeProviderLimits>,
    limits: &crate::args::ProviderLimits,
) -> ProgramResult {
    let a = &mut ctx.accounts;
    let mut entry = anchor_lang::bytemuck::Zeroable::zeroed();
    configure_limits(&mut entry, limits)?;
    require(
        a.page.page < 32 && a.page.len() <= (u32::MAX >> 5) as usize,
        RouterError::InvalidAccount,
    )?;
    a.identity.index = (((a.page.len() as u32) << 5) | u32::from(a.page.page)).into();
    a.page.try_push(entry)?;
    a.identity.market = a.page.market;
    a.identity.provider = *a.provider.address();
    a.identity.bump = ctx.bumps.identity;
    Ok(())
}

pub(crate) fn configure_provider_limits(
    ctx: &mut Context<ConfigureProviderLimits>,
    limits: &crate::args::ProviderLimits,
) -> ProgramResult {
    let a = &mut ctx.accounts;
    configure_limits(
        a.page
            .get_mut((a.identity.index.get() >> 5) as usize)
            .ok_or(RouterError::InvalidAccount)?,
        limits,
    )
}

fn provider_permits(entry: &ProviderLimitEntry, now: i64, minimum: u32) -> bool {
    entry.concurrency_limit.get() >= minimum
        && (entry.quota_limit.get() == 0
            || now >= entry.quota_window_end.get()
            || entry.quota_used.get() < entry.quota_limit.get())
}

pub(crate) fn activate(ctx: &mut Context<ActivateListRouting>) -> ProgramResult {
    require(
        ctx.accounts.market.reserved == [0; 7],
        RouterError::InvalidState,
    )?;
    ctx.accounts.market.reserved[0] = 1;
    Ok(())
}

pub(crate) fn initialize_policy(
    ctx: &mut Context<InitializeProviderPolicy>,
    limits: &BookingLimits,
) -> ProgramResult {
    limits.validate()?;
    let a = &mut ctx.accounts;
    a.policy.market = *a.market.address();
    a.policy.provider = *a.provider.address();
    a.policy.wallet_quota_window_seconds = limits.wallet_quota_window_seconds.into();
    a.policy.wallet_quota = limits.wallet_quota.into();
    a.policy.wallet_concurrency = limits.wallet_concurrency.into();
    a.policy.epoch = 1.into();
    a.policy.bump = ctx.bumps.policy;
    Ok(())
}

pub(crate) fn begin_policy_update(ctx: &mut Context<ConfigureProviderPolicy>) -> ProgramResult {
    require(ctx.accounts.policy.updating == 0, RouterError::InvalidState)?;
    ctx.accounts.policy.updating = 1;
    Ok(())
}

pub(crate) fn commit_policy(
    ctx: &mut Context<ConfigureProviderPolicy>,
    limits: &BookingLimits,
) -> ProgramResult {
    limits.validate()?;
    let policy = &mut ctx.accounts.policy;
    require(
        policy.updating == 1 && policy.active_mirrors.get() == 0,
        RouterError::InvalidState,
    )?;
    policy.epoch = add(policy.epoch.get(), 1)?.into();
    policy.wallet_quota_window_seconds = limits.wallet_quota_window_seconds.into();
    policy.wallet_quota = limits.wallet_quota.into();
    policy.wallet_concurrency = limits.wallet_concurrency.into();
    policy.updating = 0;
    Ok(())
}

pub(crate) fn initialize_routing(
    ctx: &mut Context<InitializeListRouting>,
    limit: u32,
) -> ProgramResult {
    let a = &mut ctx.accounts;
    require(a.list.reserved == [0; 4], RouterError::InvalidState)?;
    growth(
        0,
        limit,
        core::mem::size_of::<PolicyMirror>(),
        Slab::<ListRouting, PolicyMirror>::space_for(0),
        a.list.limit.get() as usize,
    )?;
    a.list.reserved[0] = 1;
    a.routing.market = *a.market.address();
    a.routing.list = *a.list.address();
    a.routing.bump = ctx.bumps.routing;
    Ok(())
}

pub(crate) fn grow_routing(ctx: &mut Context<GrowListRouting>, limit: u32) -> ProgramResult {
    let a = &mut ctx.accounts;
    growth(
        a.routing.capacity(),
        limit,
        core::mem::size_of::<PolicyMirror>(),
        Slab::<ListRouting, PolicyMirror>::space_for(0),
        a.list.limit.get() as usize,
    )?;
    a.routing.resize_to_capacity(limit)?;
    a.routing.top_up(a.payer.account())
}

fn freeze(policy: &mut ProviderPolicy, mirror: &mut PolicyMirror) -> ProgramResult {
    require(
        mirror.provider == policy.provider && mirror.active == 1 && mirror.epoch == policy.epoch,
        RouterError::InvalidState,
    )?;
    policy.active_mirrors = dec(policy.active_mirrors.get())?.into();
    mirror.active = 0;
    Ok(())
}

pub(crate) fn update_list(
    ctx: &mut Context<UpdateRoutingList>,
    provider: Address,
    enabled: bool,
) -> ProgramResult {
    let a = &mut ctx.accounts;
    require(
        a.list.reserved[0] == 1 && provider != Address::default(),
        RouterError::InvalidState,
    )?;
    super::list::change(&mut a.list, provider, enabled)?;
    if !enabled {
        if let Ok(index) = a
            .routing
            .as_slice()
            .binary_search_by_key(&provider, |entry| entry.provider)
        {
            if a.routing[index].active == 1 {
                freeze(
                    a.policy.as_mut().ok_or(RouterError::InvalidAccount)?,
                    &mut a.routing[index],
                )?;
            }
            a.routing.as_mut_slice()[index..].rotate_left(1);
            a.routing.pop();
        }
    }
    Ok(())
}

pub(crate) fn enable_mirror(ctx: &mut Context<EnablePolicyMirror>) -> ProgramResult {
    let a = &mut ctx.accounts;
    require(
        a.policy.updating == 0
            && a.list
                .as_slice()
                .binary_search(a.provider.address())
                .is_ok(),
        RouterError::Unauthorized,
    )?;
    let index = match a
        .routing
        .as_slice()
        .binary_search_by_key(a.provider.address(), |entry| entry.provider)
    {
        Ok(index) => {
            require(a.routing[index].active == 0, RouterError::InvalidState)?;
            index
        }
        Err(index) => {
            a.routing.try_push(PolicyMirror {
                provider: *a.provider.address(),
                wallet_quota_window_seconds: 0.into(),
                wallet_quota: 0.into(),
                wallet_concurrency: 0.into(),
                epoch: 0.into(),
                active: 0,
                reserved: [0; 3],
            })?;
            a.routing.as_mut_slice()[index..].rotate_right(1);
            index
        }
    };
    let mirror = &mut a.routing[index];
    mirror.wallet_quota_window_seconds = a.policy.wallet_quota_window_seconds;
    mirror.wallet_quota = a.policy.wallet_quota;
    mirror.wallet_concurrency = a.policy.wallet_concurrency;
    mirror.epoch = a.policy.epoch;
    mirror.active = 1;
    a.policy.active_mirrors = inc(a.policy.active_mirrors.get())?.into();
    Ok(())
}

pub(crate) fn freeze_mirror(ctx: &mut Context<FreezePolicyMirror>) -> ProgramResult {
    let a = &mut ctx.accounts;
    require(
        *a.signer.address() == a.policy.provider || *a.signer.address() == a.list.authority,
        RouterError::Unauthorized,
    )?;
    let index = a
        .routing
        .as_slice()
        .binary_search_by_key(&a.policy.provider, |entry| entry.provider)
        .map_err(|_| RouterError::InvalidAccount)?;
    freeze(&mut a.policy, &mut a.routing[index])
}

pub(crate) fn close_routing(ctx: &mut Context<CloseListRouting>) -> ProgramResult {
    let a = &mut ctx.accounts;
    require(
        a.list.is_empty() && a.routing.is_empty() && a.routing.book_count.get() == 0,
        RouterError::InvalidState,
    )?;
    a.list.reserved = [0; 4];
    a.routing.close(*a.authority.account())
}

pub(crate) fn initialize_capacity(
    ctx: &mut Context<InitializeModelCapacity>,
    total: u32,
) -> ProgramResult {
    let a = &mut ctx.accounts;
    let view = *a.legacy_terms.account();
    let index = if view.try_borrow()?.starts_with(Terms::DISCRIMINATOR) {
        let terms = Slab::<Terms, TermsEntry>::load(view)?;
        require(
            terms.model == *a.model.address() && terms.len() == a.legacy_capacity.len(),
            RouterError::InvalidAccount,
        )?;
        terms
            .iter()
            .position(|entry| entry.provider == *a.provider.address())
    } else {
        let terms = Slab::<FilteredTerms, LegacyTermsEntry>::load(view)?;
        require(
            terms.model == *a.model.address() && terms.len() == a.legacy_capacity.len(),
            RouterError::InvalidAccount,
        )?;
        terms
            .iter()
            .position(|entry| entry.provider == *a.provider.address())
    };
    let legacy = index
        .map(|index| a.legacy_capacity[index].active.get())
        .unwrap_or(0);
    require(total >= legacy, RouterError::LimitExceeded)?;
    a.budget.model = *a.model.address();
    a.budget.provider = *a.provider.address();
    a.budget.total = total.into();
    a.budget.legacy_active = legacy.into();
    a.budget.legacy_slot = index.map(|index| index as u32).unwrap_or(u32::MAX).into();
    a.budget.bump = ctx.bumps.budget;
    Ok(())
}

fn sync_capacity(
    budget: &mut ProviderModelCapacity,
    capacity: &Slab<Capacity, CapacityEntry>,
) -> ProgramResult {
    let active = if budget.legacy_slot.get() == u32::MAX {
        0
    } else {
        capacity
            .get(budget.legacy_slot.get() as usize)
            .ok_or(RouterError::InvalidAccount)?
            .active
            .get()
    };
    require(
        active <= budget.legacy_active.get(),
        RouterError::InvalidState,
    )?;
    budget.legacy_active = active.into();
    Ok(())
}

pub(crate) fn sync_legacy(ctx: &mut Context<SyncLegacyCapacity>) -> ProgramResult {
    sync_capacity(&mut ctx.accounts.budget, &ctx.accounts.legacy_capacity)
}

pub(crate) fn configure_capacity(
    ctx: &mut Context<ConfigureModelCapacity>,
    total: u32,
) -> ProgramResult {
    let a = &mut ctx.accounts;
    sync_capacity(&mut a.budget, &a.legacy_capacity)?;
    require(
        u64::from(total)
            >= u64::from(a.budget.allocated.get()) + u64::from(a.budget.legacy_active.get()),
        RouterError::LimitExceeded,
    )?;
    a.budget.total = total.into();
    Ok(())
}

pub(crate) fn initialize_books(
    ctx: &mut Context<InitializeListBooks>,
    limit: u32,
) -> ProgramResult {
    growth(
        0,
        limit,
        core::mem::size_of::<ListOffer>(),
        Slab::<ListTerms, ListOffer>::space_for(0),
        MAX_ENTRIES,
    )?;
    let a = &mut ctx.accounts;
    a.terms.model = *a.model.address();
    a.terms.list = *a.list.address();
    a.terms.capacity = *a.capacity.address();
    a.terms.limit = limit.into();
    a.terms.bump = ctx.bumps.terms;
    a.terms.limits_version = 1;
    a.capacity.model = *a.model.address();
    a.capacity.list = *a.list.address();
    a.capacity.terms = *a.terms.address();
    a.capacity.limit = limit.into();
    a.capacity.bump = ctx.bumps.capacity;
    a.routing.book_count = inc(a.routing.book_count.get())?.into();
    Ok(())
}

pub(crate) fn grow_books(ctx: &mut Context<GrowListBooks>, limit: u32) -> ProgramResult {
    let a = &mut ctx.accounts;
    growth(
        a.terms.capacity(),
        limit,
        core::mem::size_of::<ListOffer>(),
        Slab::<ListTerms, ListOffer>::space_for(0),
        MAX_ENTRIES,
    )?;
    a.terms.resize_to_capacity(limit)?;
    a.capacity.resize_to_capacity(limit)?;
    a.terms.top_up(a.payer.account())?;
    a.capacity.top_up(a.payer.account())?;
    a.terms.limit = limit.into();
    a.capacity.limit = limit.into();
    Ok(())
}

pub(crate) fn close_books(ctx: &mut Context<CloseListBooks>) -> ProgramResult {
    let a = &mut ctx.accounts;
    require(
        a.terms.is_empty() && a.capacity.is_empty(),
        RouterError::InvalidState,
    )?;
    a.routing.book_count = dec(a.routing.book_count.get())?.into();
    a.terms.close(*a.authority.account())?;
    a.capacity.close(*a.authority.account())
}

pub(crate) fn set_offer(ctx: &mut Context<SetListOffer>, offer: &Offer) -> ProgramResult {
    let a = &mut ctx.accounts;
    require(
        a.terms.len() == a.capacity.len(),
        RouterError::InvalidAccount,
    )?;
    let found = a
        .terms
        .as_slice()
        .binary_search_by_key(a.provider.address(), |entry| entry.terms.provider);
    let old = found
        .ok()
        .map(|index| a.terms[index].terms.capacity.get())
        .unwrap_or(0);
    let active = found
        .ok()
        .map(|index| a.capacity[index].active.get())
        .unwrap_or(0);
    require(offer.capacity >= active, RouterError::LimitExceeded)?;
    if offer.capacity == 0 {
        let index = found.map_err(|_| RouterError::InvalidAccount)?;
        a.terms.as_mut_slice()[index..].rotate_left(1);
        a.terms.pop();
        a.capacity.as_mut_slice()[index..].rotate_left(1);
        a.capacity.pop();
    } else {
        let mirror = a
            .routing
            .as_slice()
            .binary_search_by_key(a.provider.address(), |entry| entry.provider)
            .ok();
        let admitted = mirror.is_some_and(|index| a.routing[index].active == 1);
        if !admitted {
            // removal cannot trap a provider's allocation; recovery never changes live terms.
            let index = found.map_err(|_| RouterError::Unauthorized)?;
            require(
                offer.capacity <= old && !offer.active,
                RouterError::Unauthorized,
            )?;
            a.terms[index].terms.capacity = offer.capacity.into();
            a.terms[index].terms.active = 0;
        } else {
            offer.validate()?;
            let index = match found {
                Ok(index) => index,
                Err(index) => {
                    require(
                        a.terms.len() < a.terms.limit.get() as usize,
                        RouterError::Unavailable,
                    )?;
                    let order = a.terms.next_order;
                    a.terms.next_order = inc(order.get())?.into();
                    a.terms.try_push(ListOffer {
                        terms: anchor_lang::bytemuck::Zeroable::zeroed(),
                        order,
                    })?;
                    a.terms.as_mut_slice()[index..].rotate_right(1);
                    a.capacity.try_push(ListCapacityEntry {
                        provider: *a.provider.address(),
                        active: 0.into(),
                    })?;
                    a.capacity.as_mut_slice()[index..].rotate_right(1);
                    index
                }
            };
            a.terms[index].terms = TermsEntry {
                provider: *a.provider.address(),
                payout: *a.payout.address(),
                checkpoint: offer.checkpoint,
                input_rate: offer.input_rate.into(),
                cached_rate: offer.cached_rate.into(),
                output_rate: offer.output_rate.into(),
                speed: offer.speed.into(),
                privacy: u8::from(offer.privacy),
                active: u8::from(offer.active),
                input_modalities: offer.input_modalities,
                output_modalities: offer.output_modalities,
                version: LIST_OFFER_VERSION,
                reserved: 0,
                capacity: offer.capacity.into(),
                context_window: offer.context_window.into(),
                max_input: offer.max_input.into(),
                max_output: offer.max_output.into(),
                provider_policy: a.identity.index,
            };
        }
    }
    // old mirror indices become usable only after every row is authenticated and drained.
    if a.terms.limits_version == 0
        && a.terms
            .iter()
            .all(|row| row.terms.version == LIST_OFFER_VERSION)
        && a.capacity.iter().all(|row| row.active.get() == 0)
    {
        a.terms.limits_version = 1;
    }
    Ok(())
}

pub(crate) fn prune_offer(ctx: &mut Context<PruneListOffer>) -> ProgramResult {
    let a = &mut ctx.accounts;
    require(
        a.list
            .as_slice()
            .binary_search(a.provider.address())
            .is_err(),
        RouterError::InvalidState,
    )?;
    let index = a
        .terms
        .as_slice()
        .binary_search_by_key(a.provider.address(), |entry| entry.terms.provider)
        .map_err(|_| RouterError::InvalidAccount)?;
    require(
        a.capacity[index].provider == *a.provider.address() && a.capacity[index].active.get() == 0,
        RouterError::InvalidState,
    )?;
    a.terms.as_mut_slice()[index..].rotate_left(1);
    a.terms.pop();
    a.capacity.as_mut_slice()[index..].rotate_left(1);
    a.capacity.pop();
    Ok(())
}

pub(crate) fn initialize_wallet(
    ctx: &mut Context<InitializeProviderWallet>,
    limit: u32,
) -> ProgramResult {
    growth(
        0,
        limit,
        core::mem::size_of::<ProviderUsage>(),
        Slab::<ProviderWallet, ProviderUsage>::space_for(0),
        u32::MAX as usize,
    )?;
    let a = &mut ctx.accounts;
    a.wallet.market = *a.market.address();
    a.wallet.owner = *a.owner.address();
    a.wallet.rent_payer = *a.payer.address();
    a.wallet.bump = ctx.bumps.wallet;
    Ok(())
}

pub(crate) fn grow_wallet(ctx: &mut Context<GrowProviderWallet>, limit: u32) -> ProgramResult {
    let a = &mut ctx.accounts;
    growth(
        a.wallet.capacity(),
        limit,
        core::mem::size_of::<ProviderUsage>(),
        Slab::<ProviderWallet, ProviderUsage>::space_for(0),
        u32::MAX as usize,
    )?;
    a.wallet.resize_to_capacity(limit)?;
    a.wallet.top_up(a.payer.account())
}

pub(crate) fn prune_wallet(ctx: &mut Context<PruneProviderWallet>) -> ProgramResult {
    let now = Clock::get()?.unix_timestamp;
    let wallet = &mut ctx.accounts.wallet;
    let mut retained = 0;
    for index in 0..wallet.len() {
        if wallet[index].active_bookings.get() != 0 || now < wallet[index].quota_window_end.get() {
            wallet.as_mut_slice().swap(retained, index);
            retained += 1;
        }
    }
    wallet.truncate(retained);
    Ok(())
}

pub(crate) fn close_wallet(ctx: &mut Context<CloseProviderWallet>) -> ProgramResult {
    let now = Clock::get()?.unix_timestamp;
    require(
        ctx.accounts
            .wallet
            .iter()
            .all(|entry| entry.active_bookings.get() == 0 && now >= entry.quota_window_end.get()),
        RouterError::InvalidState,
    )?;
    ctx.accounts.wallet.close(*ctx.accounts.recipient.account())
}

// canonical constructors are the only writers of immutable account identity headers.
// warm paths retain typed ownership/discriminator checks and those immutable bindings.
pub(crate) fn usage_position(
    entries: &[ProviderUsage],
    provider: &Address,
    hint: usize,
) -> core::result::Result<usize, usize> {
    if entries
        .get(hint)
        .is_some_and(|entry| &entry.provider == provider)
    {
        Ok(hint)
    } else {
        entries.binary_search_by_key(provider, |entry| entry.provider)
    }
}

pub(crate) fn settle(
    capacity: &mut UncheckedAccount,
    wallet: Option<&mut UncheckedAccount>,
    model: &Account<Model>,
    booking: &Booking,
) -> ProgramResult {
    require(
        booking.wallet_booking_counted <= 3,
        RouterError::InvalidState,
    )?;
    if booking.wallet_booking_counted >= 2 {
        let wallet = wallet.ok_or(RouterError::InvalidAccount)?;
        // context duplicate checks exclude aliasing; unchecked views hold no data references.
        // each temporary typed slab exclusively borrows its account and drops before token cpis.
        let mut wallet =
            unsafe { Slab::<ProviderWallet, ProviderUsage>::load_mut(*wallet.account())? };
        require(
            wallet.market == booking.market && wallet.owner == booking.owner,
            RouterError::InvalidAccount,
        )?;
        let index = usage_position(
            wallet.as_slice(),
            &booking.provider,
            booking.provider_policy.get() as usize,
        )
        .map_err(|_| RouterError::InvalidAccount)?;
        wallet[index].active_bookings = dec(wallet[index].active_bookings.get())?.into();
        if booking.wallet_booking_counted == 3 {
            // unchecked account owns no borrow; this typed slab exclusively borrows before token cpis.
            let mut shared = unsafe {
                Slab::<ProviderLimitPage, ProviderLimitEntry>::load_mut(*capacity.account())?
            };
            require(
                shared.market == booking.market
                    && u32::from(shared.page) == booking.slot.get() & 31,
                RouterError::InvalidAccount,
            )?;
            let entry = shared
                .get_mut((booking.slot.get() >> 5) as usize)
                .ok_or(RouterError::InvalidAccount)?;
            entry.active_bookings = dec(entry.active_bookings.get())?.into();
            return Ok(());
        }
        let mut capacity =
            unsafe { Slab::<ListCapacity, ListCapacityEntry>::load_mut(*capacity.account())? };
        require(
            capacity.model == booking.model && capacity.list == booking.provider_list,
            RouterError::InvalidAccount,
        )?;
        let hint = booking.slot.get() as usize;
        let index = if capacity
            .get(hint)
            .is_some_and(|entry| entry.provider == booking.provider)
        {
            hint
        } else {
            capacity
                .as_slice()
                .binary_search_by_key(&booking.provider, |entry| entry.provider)
                .map_err(|_| RouterError::InvalidAccount)?
        };
        capacity[index].active = dec(capacity[index].active.get())?.into();
    } else {
        require(
            *capacity.address() == model.capacity,
            RouterError::InvalidAccount,
        )?;
        // same exclusive-borrow invariant as the scoped branch; header/discriminator checks remain typed.
        let mut capacity =
            unsafe { Slab::<Capacity, CapacityEntry>::load_mut(*capacity.account())? };
        require(
            capacity.model == booking.model && capacity.limit == model.limit,
            RouterError::InvalidAccount,
        )?;
        let index = booking.slot.get() as usize;
        let entry = capacity.get_mut(index).ok_or(RouterError::InvalidAccount)?;
        entry.active = dec(entry.active.get())?.into();
        if booking.wallet_booking_counted == 1 {
            let wallet = wallet.ok_or(RouterError::InvalidAccount)?;
            let mut wallet =
                unsafe { Slab::<WalletBookings, WalletBookingEntry>::load_mut(*wallet.account())? };
            require(
                wallet.market == booking.market && wallet.owner == booking.owner,
                RouterError::InvalidAccount,
            )?;
            super::booking_limits::settle(Some(&mut wallet), booking)?;
        }
    }
    Ok(())
}

fn advance<T>(
    entries: &[T],
    provider: &Address,
    mut index: usize,
    key: impl Fn(&T) -> &Address,
) -> usize {
    let local_end = (index + 4).min(entries.len());
    while index < local_end {
        if key(&entries[index]) >= provider {
            return index;
        }
        index += 1;
    }
    if index == entries.len() {
        return index;
    }
    let mut step = 1;
    while index + step < entries.len() && key(&entries[index + step]) < provider {
        step *= 2;
    }
    let end = (index + step).min(entries.len());
    index += step / 2;
    index + entries[index..end].partition_point(|entry| key(entry) < provider)
}

fn permits(quota: &ProviderLimitEntry, usage: Option<&ProviderUsage>, now: i64) -> bool {
    usage.is_none_or(|usage| {
        (quota.wallet_concurrency_limit.get() == 0
            || usage.active_bookings.get() < quota.wallet_concurrency_limit.get())
            && (quota.wallet_quota_limit.get() == 0
                || now >= usage.quota_window_end.get()
                || usage.quota_used.get() < quota.wallet_quota_limit.get())
    })
}

#[allow(clippy::too_many_arguments)]
#[inline(never)]
fn missing_eligible(
    index: usize,
    offer: &TermsEntry,
    request: &ReserveRequest,
    spend: u64,
    context: u32,
    mirrors: &[PolicyMirror],
) -> bool {
    offer.input_rate.get() <= request.input_rate_cap
        && offer.cached_rate.get() <= request.cached_rate_cap
        && offer.output_rate.get() <= request.output_rate_cap
        && offer.checkpoint == request.checkpoint
        && offer.input_modalities & request.input_modalities == request.input_modalities
        && offer.output_modalities & request.output_modalities == request.output_modalities
        && context <= offer.context_window.get()
        && request.max_input <= offer.max_input.get()
        && request.max_output <= offer.max_output.get()
        && fee(request.max_input, offer.input_rate.get())
            + fee(request.max_output, offer.output_rate.get())
            + 1
            <= spend
        && mirrors
            .get(index)
            .filter(|entry| entry.provider == offer.provider)
            .or_else(|| {
                mirrors
                    .binary_search_by_key(&offer.provider, |entry| entry.provider)
                    .ok()
                    .map(|at| &mirrors[at])
            })
            .is_some_and(|entry| entry.active == 1)
}

// defer expensive eligibility checks until an omitted price can still beat the final winner.
#[inline(never)]
fn missing_competitor(
    offers: &[ListOffer],
    capacities: &[Option<&[ProviderLimitEntry]>; 32],
    mirrors: &[PolicyMirror],
    request: &ReserveRequest,
    spend: u64,
    context: u32,
    speed: u32,
    best: (u64, u32),
) -> bool {
    offers.iter().enumerate().any(|(index, row)| {
        let offer = &row.terms;
        if capacities[(offer.provider_policy.get() & 31) as usize].is_some()
            || offer.active != 1
            || request.private && offer.privacy != 1
            || u32::from(offer.speed.get()) < speed
        {
            return false;
        }
        let input = if request.estimated_cached > 0 && offer.provider == request.cache_provider {
            fee(
                request.estimated_input - request.estimated_cached,
                offer.input_rate.get(),
            ) + fee(request.estimated_cached, offer.cached_rate.get())
        } else {
            fee(request.estimated_input, offer.input_rate.get())
        };
        let score = input + fee(request.estimated_output, offer.output_rate.get());
        (score, row.order.get()) < best
            && missing_eligible(index, offer, request, spend, context, mirrors)
    })
}

pub(crate) fn select<const COMPLETE: bool>(
    offers: &[ListOffer],
    capacities: &[Option<&[ProviderLimitEntry]>; 32],
    mirrors: &[PolicyMirror],
    wallet: &[ProviderUsage],
    request: &ReserveRequest,
    spend: u64,
    now: i64,
    minimum_wallet_concurrency: u32,
) -> core::result::Result<(usize, usize, usize, u64, u64, bool), ProgramError> {
    require(offers.len() <= MAX_ENTRIES, RouterError::InvalidAccount)?;
    let context = request
        .max_input
        .checked_add(request.max_output)
        .ok_or(RouterError::InvalidArgument)?
        .max(request.min_context);
    let speed = u32::from(request.min_speed)
        .max(request.max_output.div_ceil((86_400 - CLAIM_SECONDS) as u32));
    let mut best = (u64::MAX, u32::MAX);
    let mut chosen = None;
    let mut unknown = (u64::MAX, u32::MAX);
    let mut mirror_at = 0;
    let mut wallet_at = 0;
    let mut start = 0;
    let mut width = 1;
    let mut candidates = [(0u64, 0u32, 0usize, 0usize); 16];
    while start < offers.len() {
        let end = (start + width).min(offers.len());
        let mut count = 0;
        let mut lowest = 0;
        let mut lowest_key = (u64::MAX, u32::MAX);
        for index in start..end {
            let row = &offers[index];
            let offer = &row.terms;
            let id = offer.provider_policy.get();
            let known_limits = if best.0 == u64::MAX {
                let entry = capacities[(id & 31) as usize]
                    .map(|page| {
                        page.get((id >> 5) as usize)
                            .ok_or(RouterError::InvalidAccount)
                    })
                    .transpose()?;
                if entry.is_some_and(|entry| {
                    entry.active_bookings.get() >= entry.concurrency_limit.get()
                }) {
                    continue;
                }
                entry
            } else {
                None
            };
            if offer.active != 1
                || request.private && offer.privacy != 1
                || u32::from(offer.speed.get()) < speed
            {
                continue;
            }
            let score_for = || {
                let input =
                    if request.estimated_cached > 0 && offer.provider == request.cache_provider {
                        fee(
                            request.estimated_input - request.estimated_cached,
                            offer.input_rate.get(),
                        ) + fee(request.estimated_cached, offer.cached_rate.get())
                    } else {
                        fee(request.estimated_input, offer.input_rate.get())
                    };
                input + fee(request.estimated_output, offer.output_rate.get())
            };
            let known_score = if best.0 == u64::MAX {
                None
            } else {
                let score = score_for();
                if (score, row.order.get()) >= best {
                    continue;
                }
                Some(score)
            };
            let Some(page) = capacities[(id & 31) as usize] else {
                if COMPLETE {
                    return Err(RouterError::InvalidAccount.into());
                }
                let score = known_score.unwrap_or_else(score_for);
                unknown = unknown.min((score, row.order.get()));
                continue;
            };
            let provider_limits = if let Some(entry) = known_limits {
                entry
            } else {
                page.get((id >> 5) as usize)
                    .ok_or(RouterError::InvalidAccount)?
            };
            if provider_limits.active_bookings.get() >= provider_limits.concurrency_limit.get()
                || offer.input_rate.get() > request.input_rate_cap
                || offer.cached_rate.get() > request.cached_rate_cap
                || offer.output_rate.get() > request.output_rate_cap
            {
                continue;
            }
            if best.0 == u64::MAX
                && !provider_permits(provider_limits, now, minimum_wallet_concurrency)
            {
                continue;
            }
            // before an incumbent, traverse quotas in provider order to reject exhaustion cheaply.
            let policy_at = if best.0 == u64::MAX {
                let hint = index;
                let policy = if let Some(policy) = mirrors
                    .get(hint)
                    .filter(|entry| entry.provider == offer.provider)
                {
                    mirror_at = hint;
                    policy
                } else {
                    mirror_at =
                        advance(mirrors, &offer.provider, mirror_at, |entry| &entry.provider);
                    let Some(policy) = mirrors
                        .get(mirror_at)
                        .filter(|entry| entry.provider == offer.provider)
                    else {
                        continue;
                    };
                    policy
                };
                let quota = provider_limits;
                if policy.active != 1
                    || (quota.wallet_concurrency_limit.get() != 0
                        && quota.wallet_concurrency_limit.get() < minimum_wallet_concurrency)
                {
                    continue;
                }
                if quota.wallet_concurrency_limit.get() != 0 || quota.wallet_quota_limit.get() != 0
                {
                    let usage = if let Some(usage) = wallet
                        .get(index)
                        .filter(|entry| entry.provider == offer.provider)
                    {
                        wallet_at = index;
                        Some(usage)
                    } else {
                        wallet_at =
                            advance(wallet, &offer.provider, wallet_at, |entry| &entry.provider);
                        wallet
                            .get(wallet_at)
                            .filter(|entry| entry.provider == offer.provider)
                    };
                    if !permits(quota, usage, now) {
                        continue;
                    }
                }
                mirror_at
            } else {
                usize::MAX
            };
            let score = known_score.unwrap_or_else(score_for);
            if offer.checkpoint != request.checkpoint
                || offer.input_modalities & request.input_modalities != request.input_modalities
                || offer.output_modalities & request.output_modalities != request.output_modalities
                || context > offer.context_window.get()
                || request.max_input > offer.max_input.get()
                || request.max_output > offer.max_output.get()
            {
                continue;
            }
            if (score, row.order.get()) < lowest_key {
                lowest = count;
                lowest_key = (score, row.order.get());
            }
            candidates[count] = (score, row.order.get(), index, policy_at);
            count += 1;
        }
        width = 16;
        while count > 0 {
            let (score, order, index, policy_at) = candidates[lowest];
            let already_eligible = policy_at != usize::MAX;
            let offer = &offers[index].terms;
            // ranked candidates use independent lookups; monotonic cursors are never rewound.
            let policy_at = if policy_at == usize::MAX {
                let hint = index;
                mirrors
                    .get(hint)
                    .filter(|entry| entry.provider == offer.provider)
                    .map(|_| hint)
                    .or_else(|| {
                        mirrors
                            .binary_search_by_key(&offer.provider, |entry| entry.provider)
                            .ok()
                    })
            } else {
                Some(policy_at)
            };
            let eligible = already_eligible
                || policy_at.is_some_and(|policy_at| {
                    let policy = &mirrors[policy_at];
                    let id = offer.provider_policy.get();
                    let quota = &capacities[(id & 31) as usize].unwrap()[(id >> 5) as usize];
                    if policy.active != 1
                        || !provider_permits(quota, now, minimum_wallet_concurrency)
                        || (quota.wallet_concurrency_limit.get() != 0
                            && quota.wallet_concurrency_limit.get() < minimum_wallet_concurrency)
                    {
                        return false;
                    }
                    if quota.wallet_concurrency_limit.get() == 0
                        && quota.wallet_quota_limit.get() == 0
                    {
                        return true;
                    }
                    let usage = usage_position(wallet, &offer.provider, index)
                        .ok()
                        .map(|index| &wallet[index]);
                    permits(quota, usage, now)
                });
            let reserve = fee(request.max_input, offer.input_rate.get())
                + fee(request.max_output, offer.output_rate.get())
                + 1;
            if eligible && reserve <= spend {
                best = (score, order);
                chosen = Some((
                    index,
                    policy_at.unwrap(),
                    reserve,
                    score,
                    offer.provider == request.cache_provider,
                ));
                break;
            }
            count -= 1;
            candidates[lowest] = candidates[count];
            lowest = 0;
            for index in 1..count {
                if candidates[index] < candidates[lowest] {
                    lowest = index;
                }
            }
        }
        if best == (0, 0) {
            break;
        }
        start = end;
    }
    if !COMPLETE && unknown != (u64::MAX, u32::MAX) && (chosen.is_none() || unknown < best) {
        require(
            !missing_competitor(
                offers, capacities, mirrors, request, spend, context, speed, best,
            ),
            RouterError::ProviderLimitPageRequired,
        )?;
    }
    let (index, policy, reserve, score, cache) = chosen.ok_or(RouterError::Unavailable)?;
    let usage =
        usage_position(wallet, &offers[index].terms.provider, index).unwrap_or_else(|index| index);
    Ok((index, policy, usage, reserve, score, cache))
}

pub(crate) fn reserve(
    ctx: &mut Context<ReserveListBooking>,
    request: &ReserveRequest,
    requested_generation: Option<u64>,
    minimum_wallet_concurrency: Option<u32>,
) -> ProgramResult {
    let minimum_wallet_concurrency = minimum_wallet_concurrency.unwrap_or(1);
    require(minimum_wallet_concurrency > 0, RouterError::InvalidArgument)?;
    let remaining = ctx.remaining_accounts()?;
    let a = &mut ctx.accounts;
    let generation = receipt::prepare_booking(
        &a.booking,
        a.owner.address(),
        a.market.address(),
        a.user.address(),
        a.rent_payer.address(),
        requested_generation,
    )?;
    require(
        generation == 0 || generation == add(a.user.generation.get(), 1)?,
        RouterError::InvalidState,
    )?;
    require(
        a.model.model_id == request.model_id
            && a.group.is_some() == (a.model.group != Address::default()),
        RouterError::InvalidAccount,
    )?;
    let fee_bps = a.list.fee_bps.get();
    let protocol_share = a.list.protocol_share.get();
    super::fees::validate_terms(fee_bps, protocol_share)?;
    let spend = fee_budget(request.validate(a.user.free.get())?, fee_bps);
    let now = Clock::get()?.unix_timestamp;
    require(
        a.limits_page.page < 32 && remaining.len() < 32,
        RouterError::InvalidAccount,
    )?;
    let mut views = [None; 32];
    views[a.limits_page.page as usize] = Some(*a.limits_page.account());
    let (slot, policy_at, usage_at, reserve, score, cache) = {
        let mut pages = Vec::with_capacity(remaining.len());
        for view in remaining {
            let page = Slab::<ProviderLimitPage, ProviderLimitEntry>::load(view)?;
            require(
                page.market == *a.market.address()
                    && page.page < 32
                    && views[page.page as usize].is_none(),
                RouterError::InvalidAccount,
            )?;
            views[page.page as usize] = Some(view);
            pages.push(page);
        }
        let mut capacities = [None; 32];
        capacities[a.limits_page.page as usize] = Some(a.limits_page.as_slice());
        for page in &pages {
            capacities[page.page as usize] = Some(page.as_slice());
        }
        let selector = if pages.len() == 31 {
            select::<true>
        } else {
            select::<false>
        };
        selector(
            a.terms.as_slice(),
            &capacities,
            a.routing.as_slice(),
            a.wallet.as_slice(),
            request,
            spend,
            now,
            minimum_wallet_concurrency,
        )?
    };
    let offer = &a.terms[slot].terms;
    let _ = policy_at;
    let id = offer.provider_policy.get();
    let provider_at = (id >> 5) as usize;
    let view = views[(id & 31) as usize].ok_or(RouterError::InvalidAccount)?;
    require(view.is_writable(), RouterError::ProviderLimitPageReadonly)?;
    let mut selected_page;
    let capacity = if id & 31 == u32::from(a.limits_page.page) {
        &mut a.limits_page
    } else {
        // selection slabs dropped; distinct page ids exclude aliases with declared mutable accounts.
        selected_page = unsafe { Slab::<ProviderLimitPage, ProviderLimitEntry>::load_mut(view)? };
        &mut selected_page
    };
    let policy = &capacity[provider_at];
    if a.wallet
        .get(usage_at)
        .is_none_or(|entry| entry.provider != offer.provider)
    {
        require(!a.wallet.is_full(), RouterError::WalletBookingsFull)?;
        a.wallet.try_push(ProviderUsage {
            provider: offer.provider,
            quota_window_end: now.into(),
            quota_used: 0.into(),
            active_bookings: 0.into(),
        })?;
        a.wallet.as_mut_slice()[usage_at..].rotate_right(1);
    }
    let usage = &mut a.wallet[usage_at];
    if now >= usage.quota_window_end.get() {
        usage.quota_window_end = now
            .checked_add(i64::from(policy.wallet_quota_window_seconds.get()))
            .ok_or(RouterError::Arithmetic)?
            .into();
        usage.quota_used = 0.into();
    }
    usage.quota_used = usage.quota_used.get().saturating_add(1).into();
    usage.active_bookings = inc(usage.active_bookings.get())?.into();
    let provider_limits = &mut capacity[provider_at];
    if now >= provider_limits.quota_window_end.get() {
        provider_limits.quota_window_end = now
            .checked_add(i64::from(provider_limits.quota_window_seconds.get()))
            .ok_or(RouterError::Arithmetic)?
            .into();
        provider_limits.quota_used = 0.into();
    }
    provider_limits.quota_used = provider_limits.quota_used.get().saturating_add(1).into();
    provider_limits.active_bookings = inc(provider_limits.active_bookings.get())?.into();
    let reserve = add(reserve, bps_ceil(reserve, fee_bps))?;
    a.user.free = sub(a.user.free.get(), reserve)?.into();
    if generation > 0 {
        a.user.generation = generation.into();
    }
    let backing = a.group.as_ref().filter(|group| group.route_to_reserve == 1);
    let booking = &mut a.booking;
    booking.owner = *a.owner.address();
    booking.provider = offer.provider;
    booking.market = *a.market.address();
    booking.reserve = reserve.into();
    booking.input_rate = offer.input_rate;
    booking.cached_rate = offer.cached_rate;
    booking.output_rate = offer.output_rate;
    booking.max_input = request.max_input.into();
    booking.max_output = request.max_output.into();
    booking.claim_end = now
        .checked_add(
            CLAIM_SECONDS + i64::from(request.max_output.div_ceil(u32::from(offer.speed.get()))),
        )
        .ok_or(RouterError::Arithmetic)?
        .into();
    booking.provider_policy = (usage_at as u32).into();
    booking.reserved = [0; 4];
    booking.status = 0;
    booking.reserved2 = [0; 3];
    booking.generation = generation.into();
    booking.charge = 0.into();
    booking.input = 0.into();
    booking.cached = 0.into();
    booking.output = 0.into();
    booking.min_speed = request.min_speed.into();
    booking.reserved3 = [0; 10];
    booking.user = *a.user.address();
    booking.model = *a.model.address();
    booking.rent_payer = *a.rent_payer.address();
    booking.payout = offer.payout;
    booking.provider_list = *a.list.address();
    booking.slot = id.into();
    booking.wallet_booking_counted = 3;
    booking.requested_privacy = u8::from(request.private);
    booking.cache_allowed = u8::from(cache);
    booking.reserved5 = 0;
    booking.estimated_score = add(score, bps_ceil(score, fee_bps))?.into();
    booking.creator = a.list.authority;
    booking.protocol_destination = backing
        .map(|group| group.backing_vault)
        .unwrap_or(a.market.council);
    booking.reserve_authority = backing.map(|group| *group.address()).unwrap_or_default();
    booking.fee_bps = fee_bps.into();
    booking.protocol_share = protocol_share.into();
    booking.reserve_destination = u8::from(backing.is_some());
    booking.reserved6 = [0; 3];
    booking.checkpoint = offer.checkpoint;
    booking.offer_context_window = offer.context_window;
    booking.offer_max_input = offer.max_input;
    booking.offer_max_output = offer.max_output;
    booking.offer_input_modalities = offer.input_modalities;
    booking.offer_output_modalities = offer.output_modalities;
    booking.requested_input_modalities = request.input_modalities;
    booking.requested_output_modalities = request.output_modalities;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;
    use anchor_lang::bytemuck::Zeroable;

    #[test]
    fn scoped_matching_equals_independent_ordered_oracle_with_stale_hints_and_gaps() {
        for seed in 0..256u32 {
            let mut offers: Vec<ListOffer> = (0..64u32)
                .map(|index| {
                    let mut row = ListOffer::zeroed();
                    row.terms.provider = Address::new_from_array([index as u8 + 1; 32]);
                    row.terms.checkpoint = [7; 32];
                    row.terms.input_rate = (1 + (index * 7919 + seed * 23) % 2_000_000).into();
                    row.terms.output_rate = (1 + (index * 179 + seed * 29) % 2_000_000).into();
                    row.terms.cached_rate = 0.into();
                    row.terms.speed = 100.into();
                    row.terms.active = 1;
                    row.terms.privacy = 1;
                    row.terms.input_modalities = 3;
                    row.terms.output_modalities = 5;
                    row.terms.version = OFFER_VERSION;
                    row.terms.context_window = 200.into();
                    row.terms.max_input = 100.into();
                    row.terms.max_output = 100.into();
                    row.terms.capacity = 4.into();
                    row.terms.provider_policy = (index << 5).into();
                    row.order = ((index * 13 + seed) % 64).into();
                    row
                })
                .collect();
            let capacities: Vec<_> = offers
                .iter()
                .map(|_| ProviderLimitEntry {
                    concurrency_limit: 4.into(),
                    quota_limit: 0.into(),
                    quota_window_seconds: 60.into(),
                    quota_window_end: 0.into(),
                    quota_used: 0.into(),
                    active_bookings: 0.into(),
                    wallet_concurrency_limit: 2.into(),
                    wallet_quota_limit: 3.into(),
                    wallet_quota_window_seconds: 60.into(),
                })
                .collect();
            let mirrors: Vec<_> = offers
                .iter()
                .enumerate()
                .filter(|(index, _)| !(index + seed as usize).is_multiple_of(7))
                .map(|(index, row)| PolicyMirror {
                    provider: row.terms.provider,
                    wallet_quota_window_seconds: 60.into(),
                    wallet_quota: 3.into(),
                    wallet_concurrency: 2.into(),
                    epoch: 1.into(),
                    active: u8::from(!(index + seed as usize).is_multiple_of(5)),
                    reserved: [0; 3],
                })
                .collect();
            let wallet: Vec<_> = (0..128u32)
                .filter(|index| (index + seed) % 3 != 0)
                .map(|index| ProviderUsage {
                    provider: Address::new_from_array([index as u8; 32]),
                    quota_window_end: 100.into(),
                    quota_used: ((index + seed) % 4).into(),
                    active_bookings: ((index + seed) % 3).into(),
                })
                .collect();
            let request = ReserveRequest {
                model_id: Address::default(),
                input_rate_cap: 2_000_000,
                cached_rate_cap: 2_000_000,
                output_rate_cap: 2_000_000,
                min_speed: 1,
                max_input: 100,
                max_output: 100,
                private: true,
                spend: 40,
                estimated_input: if seed % 4 == 0 { 0 } else { 50 },
                estimated_cached: 0,
                estimated_output: if seed % 4 == 0 { 0 } else { 50 },
                cache_provider: Address::default(),
                checkpoint: [7; 32],
                input_modalities: 1,
                output_modalities: 1,
                min_context: 0,
                limits_configured: false,
            };
            if seed % 2 == 0 {
                offers[3].terms.checkpoint = [8; 32];
            }
            let mut pages = [None; 32];
            pages[0] = Some(capacities.as_slice());
            let actual = select::<false>(
                &offers,
                &pages,
                &mirrors,
                &wallet,
                &request,
                request.spend,
                50,
                1,
            )
            .ok()
            .map(|(index, _, _, reserve, score, _)| (offers[index].terms.provider, reserve, score));
            let mut ordered = offers.clone();
            ordered.sort_by_key(|row| row.order.get());
            let mut expected = None;
            for row in ordered {
                let offer = row.terms;
                let policy = mirrors
                    .iter()
                    .find(|entry| entry.provider == offer.provider);
                let usage = wallet.iter().find(|entry| entry.provider == offer.provider);
                if policy.is_none_or(|p| {
                    p.active != 1
                        || usage.is_some_and(|u| {
                            u.active_bookings.get() >= 2 || u.quota_used.get() >= 3
                        })
                }) || offer.checkpoint != request.checkpoint
                {
                    continue;
                }
                let rounded = |tokens: u32, rate: u32| {
                    (u64::from(tokens) * u64::from(rate)).div_ceil(1_000_000)
                };
                let reserve = rounded(100, offer.input_rate.get())
                    + rounded(100, offer.output_rate.get())
                    + 1;
                let score = rounded(request.estimated_input, offer.input_rate.get())
                    + rounded(request.estimated_output, offer.output_rate.get());
                if reserve <= request.spend && expected.is_none_or(|(_, _, best)| score < best) {
                    expected = Some((offer.provider, reserve, score));
                }
            }
            assert_eq!(actual, expected, "seed {seed}");
        }
    }

    #[test]
    fn galloping_cursor_matches_lower_bound_for_sparse_histories() {
        let entries: Vec<_> = (0..10_000u32)
            .map(|index| {
                let mut key = [0; 32];
                key[28..].copy_from_slice(&(index * 3).to_be_bytes());
                Address::new_from_array(key)
            })
            .collect();
        let mut cursor = 0;
        for value in [0u32, 1, 3, 7, 20_000, 20_001, 29_996, 29_999, 40_000] {
            let mut key = [0; 32];
            key[28..].copy_from_slice(&value.to_be_bytes());
            let key = Address::new_from_array(key);
            cursor = advance(&entries, &key, cursor, |key| key);
            assert_eq!(cursor, entries.partition_point(|entry| entry < &key));
        }
    }
}
