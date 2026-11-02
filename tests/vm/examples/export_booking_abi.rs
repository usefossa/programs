use anchor_lang::{prelude::Address, Discriminator, InstructionData, ToAccountMetas};
use inference_router_program::{accounts, args, instruction as ix, state_types, ID};

fn emit(name: &str, metas: impl ToAccountMetas, data: impl InstructionData) {
    let metas = metas
        .to_account_metas(None)
        .into_iter()
        .map(|meta| {
            format!(
                "{{\"address\":\"{}\",\"writable\":{},\"signer\":{}}}",
                meta.pubkey, meta.is_writable, meta.is_signer
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    println!(
        "{{\"name\":\"{name}\",\"data\":{:?},\"accounts\":[{metas}]}}",
        data.data()
    );
}
fn main() {
    let k = |byte| Address::new_from_array([byte; 32]);
    let market = k(1);
    let mint = k(2);
    let owner = k(3);
    let user = k(4);
    let vault = k(5);
    let model_id = k(6);
    let terms = k(7);
    let capacity = k(8);
    let booking = k(9);
    let provider = k(10);
    let payout = k(11);
    let payer = k(12);
    let list = k(13);
    let model =
        Address::find_program_address(&[b"model", market.as_ref(), model_id.as_ref()], &ID).0;
    let profile =
        Address::find_program_address(&[b"provider", market.as_ref(), provider.as_ref()], &ID).0;
    let authority = Address::find_program_address(&[b"vault", user.as_ref()], &ID).0;
    let limits = args::BookingLimits {
        wallet_window_seconds: 60,
        wallet_bookings_per_window: 5,
        wallet_booking_capacity: 2,
    };

    let offer = args::Offer {
        slot: 0,
        input_rate: 1000,
        cached_rate: 100,
        output_rate: 2000,
        speed: 32,
        privacy: true,
        active: true,
        capacity: 10,
        checkpoint: [7; 32],
        input_modalities: 1,
        output_modalities: 1,
        context_window: 1_000_000,
        max_input: 1_000_000,
        max_output: 1_000_000,
    };
    emit(
        "register_offer",
        accounts::RegisterOffer {
            market,
            model,
            terms,
            capacity,
            profile,
            provider,
            payout,
        },
        ix::RegisterOffer { offer },
    );
    let generation = 72_057_594_037_927_943;
    let request = args::ReserveRequest {
        model_id,
        input_rate_cap: 10_000,
        cached_rate_cap: 1000,
        output_rate_cap: 20_000,
        min_speed: 32,
        max_input: 100,
        max_output: 200,
        private: true,
        spend: 50_000,
        estimated_input: 10,
        estimated_cached: 3,
        estimated_output: 20,
        cache_provider: provider,
        checkpoint: [7; 32],
        input_modalities: 1,
        output_modalities: 1,
        min_context: 0,
        limits_configured: false,
    };
    emit(
        "reserve_booking",
        accounts::ReserveBooking {
            market,
            model,
            terms,
            capacity,
            user,
            owner,
            booking,
            rent_payer: payer,
            list,
            group: None,
        },
        ix::ReserveBooking {
            request,
            generation: Some(generation),
        },
    );
    let receipt = args::ReceiptInput {
        owner,
        user,
        booking,
        generation,
        charge: 43,
        claim_end: 1_793_420_000,
    };
    emit(
        "claim_booking",
        accounts::ClaimBooking {
            market,
            model,
            capacity,
            user,
            booking,
            provider,
            vault,
            payout,
            mint,
            authority,
            token: anchor_spl::token::ID,
        },
        ix::ClaimBooking {
            claim: args::Claim {
                uncached: 7,
                cached: 3,
                output: 20,
                receipt: Some(receipt),
            },
        },
    );
    emit(
        "expire_booking",
        accounts::ExpireBooking {
            market,
            model,
            capacity,
            user,
            booking,
        },
        ix::ExpireBooking {
            generation: Some(generation),
        },
    );
    emit(
        "release_booking",
        accounts::ReleaseBooking {
            market,
            model,
            capacity,
            user,
            booking,
            provider,
        },
        ix::ReleaseBooking {
            generation: Some(generation),
        },
    );
    for (name, disc, bytes) in [
        (
            "Terms",
            state_types::Terms::DISCRIMINATOR,
            8 + std::mem::size_of::<state_types::Terms>(),
        ),
        (
            "BookingPolicies",
            state_types::BookingPolicies::DISCRIMINATOR,
            8 + std::mem::size_of::<state_types::BookingPolicies>(),
        ),
        (
            "WalletBookings",
            state_types::WalletBookings::DISCRIMINATOR,
            8 + std::mem::size_of::<state_types::WalletBookings>(),
        ),
    ] {
        println!("{{\"layout\":\"{name}\",\"discriminator\":{disc:?},\"bytes\":{bytes}}}");
    }
    emit_scoped(
        market, model, model_id, owner, user, booking, provider, profile, payer, list, payout,
        mint, vault, authority, request, offer, limits, generation,
    );
}

#[allow(clippy::too_many_arguments)]
fn emit_scoped(
    market: Address,
    model: Address,
    _model_id: Address,
    owner: Address,
    user: Address,
    booking: Address,
    provider: Address,
    profile: Address,
    payer: Address,
    list: Address,
    payout: Address,
    mint: Address,
    vault: Address,
    authority: Address,
    request: args::ReserveRequest,
    offer: args::Offer,
    limits: args::BookingLimits,
    generation: u64,
) {
    use anchor_lang::{
        accounts::Slab,
        bytemuck::{bytes_of, cast_slice, Zeroable},
    };
    let derive = |seeds: &[&[u8]]| Address::find_program_address(seeds, &ID);
    let (policy, policy_bump) = derive(&[b"provider_policy", market.as_ref(), provider.as_ref()]);
    let (routing, routing_bump) = derive(&[b"list_routing", list.as_ref()]);
    let (terms, terms_bump) = derive(&[b"list_terms", list.as_ref(), model.as_ref()]);
    let (capacity, capacity_bump) = derive(&[b"list_capacity", list.as_ref(), model.as_ref()]);
    let (wallet, wallet_bump) = derive(&[b"provider_wallet", market.as_ref(), owner.as_ref()]);
    let (budget, budget_bump) = derive(&[b"provider_capacity", model.as_ref(), provider.as_ref()]);
    let system_program = solana_sdk_ids::system_program::ID;
    emit(
        "activate_list_routing",
        accounts::ActivateListRouting {
            market,
            council: payer,
        },
        ix::ActivateListRouting {},
    );
    emit(
        "initialize_provider_policy",
        accounts::InitializeProviderPolicy {
            market,
            provider,
            payer,
            profile,
            policy,
            system_program,
        },
        ix::InitializeProviderPolicy { limits },
    );
    emit(
        "begin_provider_policy_update",
        accounts::ConfigureProviderPolicy { policy, provider },
        ix::BeginProviderPolicyUpdate {},
    );
    emit(
        "commit_provider_policy_update",
        accounts::ConfigureProviderPolicy { policy, provider },
        ix::CommitProviderPolicyUpdate { limits },
    );
    emit(
        "initialize_list_routing",
        accounts::InitializeListRouting {
            market,
            list,
            authority: payer,
            payer,
            routing,
            system_program,
        },
        ix::InitializeListRouting { limit: 8 },
    );
    emit(
        "grow_list_routing",
        accounts::GrowListRouting {
            list,
            authority: payer,
            routing,
            payer,
            system_program,
        },
        ix::GrowListRouting { limit: 16 },
    );
    emit(
        "update_routing_list",
        accounts::UpdateRoutingList {
            list,
            authority: payer,
            routing,
            policy: Some(policy),
        },
        ix::UpdateRoutingList {
            provider,
            enabled: false,
        },
    );
    emit(
        "enable_policy_mirror",
        accounts::EnablePolicyMirror {
            provider,
            policy,
            list,
            routing,
        },
        ix::EnablePolicyMirror {},
    );
    emit(
        "freeze_policy_mirror",
        accounts::FreezePolicyMirror {
            signer: provider,
            policy,
            list,
            routing,
        },
        ix::FreezePolicyMirror {},
    );
    emit(
        "initialize_list_books",
        accounts::InitializeListBooks {
            market,
            model,
            list,
            authority: payer,
            routing,
            payer,
            terms,
            capacity,
            system_program,
        },
        ix::InitializeListBooks { limit: 8 },
    );
    emit(
        "set_list_offer",
        accounts::SetListOffer {
            market,
            model,
            provider,
            budget,
            list,
            routing,
            terms,
            capacity,
            payout,
        },
        ix::SetListOffer { offer },
    );
    emit(
        "prune_list_offer",
        accounts::PruneListOffer {
            list,
            terms,
            capacity,
            budget,
        },
        ix::PruneListOffer {},
    );
    emit(
        "initialize_provider_wallet",
        accounts::InitializeProviderWallet {
            market,
            owner,
            payer,
            wallet,
            system_program,
        },
        ix::InitializeProviderWallet { limit: 8 },
    );
    emit(
        "close_list_routing",
        accounts::CloseListRouting {
            list,
            authority: payer,
            routing,
        },
        ix::CloseListRouting {},
    );
    emit(
        "initialize_model_capacity",
        accounts::InitializeModelCapacity {
            market,
            model,
            provider,
            payer,
            profile,
            legacy_terms: Address::new_from_array([7; 32]),
            legacy_capacity: Address::new_from_array([8; 32]),
            budget,
            system_program,
        },
        ix::InitializeModelCapacity { total: 20 },
    );
    emit(
        "configure_model_capacity",
        accounts::ConfigureModelCapacity {
            provider,
            budget,
            model,
            legacy_capacity: Address::new_from_array([8; 32]),
        },
        ix::ConfigureModelCapacity { total: 30 },
    );
    emit(
        "sync_legacy_capacity",
        accounts::SyncLegacyCapacity {
            budget,
            model,
            legacy_capacity: Address::new_from_array([8; 32]),
        },
        ix::SyncLegacyCapacity {},
    );
    emit(
        "grow_list_books",
        accounts::GrowListBooks {
            list,
            authority: payer,
            terms,
            capacity,
            payer,
            system_program,
        },
        ix::GrowListBooks { limit: 16 },
    );
    emit(
        "close_list_books",
        accounts::CloseListBooks {
            list,
            authority: payer,
            routing,
            terms,
            capacity,
        },
        ix::CloseListBooks {},
    );
    emit(
        "grow_provider_wallet",
        accounts::GrowProviderWallet {
            owner,
            wallet,
            payer,
            system_program,
        },
        ix::GrowProviderWallet { limit: 16 },
    );
    emit(
        "prune_provider_wallet",
        accounts::PruneProviderWallet { owner, wallet },
        ix::PruneProviderWallet {},
    );
    emit(
        "close_provider_wallet",
        accounts::CloseProviderWallet {
            owner,
            wallet,
            recipient: payer,
        },
        ix::CloseProviderWallet {},
    );
    emit(
        "reserve_list_booking",
        accounts::ReserveListBooking {
            market,
            model,
            terms,
            capacity,
            user,
            owner,
            booking,
            rent_payer: payer,
            list,
            routing,
            wallet,
            group: None,
        },
        ix::ReserveListBooking {
            minimum_wallet_capacity: None,
            request,
            generation: Some(generation),
        },
    );
    emit(
        "claim_list_booking",
        accounts::ClaimListBooking {
            base: accounts::ClaimBooking {
                market,
                model,
                capacity,
                user,
                booking,
                provider,
                vault,
                payout,
                mint,
                authority,
                token: anchor_spl::token::ID,
            },
            wallet_bookings: wallet,
        },
        ix::ClaimListBooking {
            claim: args::Claim {
                uncached: 7,
                cached: 0,
                output: 20,
                receipt: None,
            },
        },
    );
    emit(
        "release_list_booking",
        accounts::ReleaseListBooking {
            base: accounts::ReleaseBooking {
                market,
                model,
                capacity,
                user,
                booking,
                provider,
            },
            wallet_bookings: wallet,
        },
        ix::ReleaseListBooking {
            generation: Some(generation),
        },
    );
    emit(
        "expire_list_booking",
        accounts::ExpireListBooking {
            base: accounts::ExpireBooking {
                market,
                model,
                capacity,
                user,
                booking,
            },
            wallet_bookings: wallet,
        },
        ix::ExpireListBooking {
            generation: Some(generation),
        },
    );
    let mut policy_state = state_types::ProviderPolicy::zeroed();
    policy_state.market = market;
    policy_state.provider = provider;
    policy_state.wallet_window_seconds = limits.wallet_window_seconds.into();
    policy_state.wallet_bookings_per_window = limits.wallet_bookings_per_window.into();
    policy_state.wallet_booking_capacity = limits.wallet_booking_capacity.into();
    policy_state.epoch = 1.into();
    policy_state.active_mirrors = 1.into();
    policy_state.bump = policy_bump;
    let mut budget_state = state_types::ProviderModelCapacity::zeroed();
    budget_state.model = model;
    budget_state.provider = provider;
    budget_state.total = 20.into();
    budget_state.allocated = 10.into();
    budget_state.legacy_slot = u32::MAX.into();
    budget_state.bump = budget_bump;
    for (name, key, disc, data) in [
        (
            "ProviderPolicy",
            policy,
            state_types::ProviderPolicy::DISCRIMINATOR,
            bytes_of(&policy_state),
        ),
        (
            "ProviderModelCapacity",
            budget,
            state_types::ProviderModelCapacity::DISCRIMINATOR,
            bytes_of(&budget_state),
        ),
    ] {
        let data = [disc, data].concat();
        println!("{{\"layout\":\"{name}\",\"address\":\"{key}\",\"discriminator\":{disc:?},\"bytes\":{},\"data\":{data:?}}}", data.len());
    }
    let mut routing_state = state_types::ListRouting::zeroed();
    routing_state.market = market;
    routing_state.list = list;
    routing_state.book_count = 1.into();
    routing_state.bump = routing_bump;
    let mirror = state_types::PolicyMirror {
        provider,
        wallet_window_seconds: limits.wallet_window_seconds.into(),
        wallet_bookings_per_window: limits.wallet_bookings_per_window.into(),
        wallet_booking_capacity: limits.wallet_booking_capacity.into(),
        epoch: 1.into(),
        active: 1,
        reserved: [0; 3],
    };
    let mut terms_state = state_types::ListTerms::zeroed();
    terms_state.model = model;
    terms_state.list = list;
    terms_state.capacity = capacity;
    terms_state.limit = 1.into();
    terms_state.next_order = 1.into();
    terms_state.bump = terms_bump;
    let entry = state_types::ListOffer {
        terms: state_types::TermsEntry {
            provider,
            payout,
            checkpoint: offer.checkpoint,
            input_rate: offer.input_rate.into(),
            cached_rate: offer.cached_rate.into(),
            output_rate: offer.output_rate.into(),
            speed: offer.speed.into(),
            privacy: 1,
            active: 1,
            input_modalities: offer.input_modalities,
            output_modalities: offer.output_modalities,
            version: state_types::OFFER_VERSION,
            reserved: 0,
            capacity: 10.into(),
            context_window: offer.context_window.into(),
            max_input: offer.max_input.into(),
            max_output: offer.max_output.into(),
            provider_policy: 0.into(),
        },
        order: 0.into(),
    };
    let mut capacity_state = state_types::ListCapacity::zeroed();
    capacity_state.model = model;
    capacity_state.list = list;
    capacity_state.terms = terms;
    capacity_state.limit = 1.into();
    capacity_state.bump = capacity_bump;
    let active = state_types::ListCapacityEntry {
        provider,
        active: 1.into(),
    };
    let mut wallet_state = state_types::ProviderWallet::zeroed();
    wallet_state.market = market;
    wallet_state.owner = owner;
    wallet_state.rent_payer = payer;
    wallet_state.bump = wallet_bump;
    let usage = state_types::ProviderUsage {
        provider,
        window_end: 1_793_420_000i64.into(),
        bookings: 2.into(),
        outstanding: 1.into(),
    };
    macro_rules! slab { ($name:literal, $key:expr, $header:expr, $entry:expr, $h:ty, $t:ty) => {{
        let offset=Slab::<$h,$t>::space_for(0); let mut data=vec![0;Slab::<$h,$t>::space_for(1)]; let disc=<$h>::DISCRIMINATOR;
        data[..8].copy_from_slice(disc); let header=bytes_of(&$header); data[8..8+header.len()].copy_from_slice(header); data[8+header.len()..12+header.len()].copy_from_slice(&1u32.to_le_bytes()); data[offset..].copy_from_slice(cast_slice(&[$entry]));
        println!("{{\"layout\":\"{}\",\"address\":\"{}\",\"discriminator\":{disc:?},\"entry_offset\":{offset},\"entry_bytes\":{},\"data\":{data:?}}}",$name,$key,core::mem::size_of::<$t>());
    }}}
    slab!(
        "ListRouting",
        routing,
        routing_state,
        mirror,
        state_types::ListRouting,
        state_types::PolicyMirror
    );
    slab!(
        "ListTerms",
        terms,
        terms_state,
        entry,
        state_types::ListTerms,
        state_types::ListOffer
    );
    slab!(
        "ListCapacity",
        capacity,
        capacity_state,
        active,
        state_types::ListCapacity,
        state_types::ListCapacityEntry
    );
    slab!(
        "ProviderWallet",
        wallet,
        wallet_state,
        usage,
        state_types::ProviderWallet,
        state_types::ProviderUsage
    );
}
