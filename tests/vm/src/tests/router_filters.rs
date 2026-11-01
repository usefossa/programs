use super::*;
use anchor_lang::accounts::Slab;
use state_types::{MODALITY_AUDIO, MODALITY_IMAGE, MODALITY_TEXT, MODALITY_VIDEO};

struct FilterFixture {
    svm: LiteSVM,
    payer: Keypair,
    providers: [Keypair; 2],
    payouts: [Address; 2],
    market: Address,
    model: Address,
    model_id: Address,
    terms: Address,
    capacity: Address,
    list: Address,
    user: Address,
    booking: Address,
    mint: Address,
    vault: Address,
    authority: Address,
    fee_destinations: [Address; 2],
}

fn offer(slot: u32) -> args::Offer {
    args::Offer {
        slot,
        input_rate: 500_000 + slot * 500_000,
        cached_rate: 0,
        output_rate: 500_000 + slot * 500_000,
        speed: 100,
        privacy: true,
        active: true,
        capacity: 10,
        checkpoint: [7; 32],
        input_modalities: MODALITY_TEXT | MODALITY_IMAGE,
        output_modalities: MODALITY_TEXT | MODALITY_AUDIO,
        context_window: 200,
        max_input: 100,
        max_output: 100,
    }
}

impl FilterFixture {
    fn new() -> Self {
        let (mut svm, payer) = setup(inference_router_program::ID, "inference_router_program.so");
        let mint = create_mint(&mut svm, &payer, payer.pubkey(), 6);
        let market = initialize_market(&mut svm, &payer, mint);
        let providers = [Keypair::new(), Keypair::new()];
        let mut profiles = [Address::default(); 2];
        let mut payouts = [Address::default(); 2];
        for (i, provider) in providers.iter().enumerate() {
            profiles[i] = Address::find_program_address(
                &[b"provider", market.as_ref(), provider.pubkey().as_ref()],
                &inference_router_program::ID,
            )
            .0;
            payouts[i] = create_token_account(&mut svm, &payer, mint, provider.pubkey());
            send(
                &mut svm,
                &[instruction(
                    inference_router_program::ID,
                    accounts::InitializeProvider {
                        market,
                        provider: provider.pubkey(),
                        payer: payer.pubkey(),
                        profile: profiles[i],
                        system_program: solana_sdk_ids::system_program::ID,
                    },
                    ix::InitializeProvider {
                        metadata: args::Metadata {
                            document_hash: [1; 32],
                            uri: "https://example.com/provider.json".into(),
                        },
                    },
                )],
                &payer,
                &[provider],
            )
            .unwrap();
        }
        let model_id = Keypair::new().pubkey();
        let model = Address::find_program_address(
            &[b"model", market.as_ref(), model_id.as_ref()],
            &inference_router_program::ID,
        )
        .0;
        let metadata = Address::find_program_address(
            &[b"model_metadata", model.as_ref()],
            &inference_router_program::ID,
        )
        .0;
        let terms = Keypair::new();
        let capacity = Keypair::new();
        send(
            &mut svm,
            &[instruction(
                inference_router_program::ID,
                accounts::InitializeModel {
                    market,
                    provider: providers[0].pubkey(),
                    payer: payer.pubkey(),
                    profile: profiles[0],
                    model,
                    terms: terms.pubkey(),
                    capacity: capacity.pubkey(),
                    metadata,
                    system_program: solana_sdk_ids::system_program::ID,
                },
                ix::InitializeModel {
                    model_id,
                    limit: 2,
                    metadata: args::Metadata {
                        document_hash: [2; 32],
                        uri: "https://example.com/model.json".into(),
                    },
                },
            )],
            &payer,
            &[&providers[0], &terms, &capacity],
        )
        .unwrap();
        for (i, provider) in providers.iter().enumerate() {
            let register = |offer| {
                instruction(
                    inference_router_program::ID,
                    accounts::RegisterOffer {
                        market,
                        model,
                        terms: terms.pubkey(),
                        capacity: capacity.pubkey(),
                        profile: profiles[i],
                        provider: provider.pubkey(),
                        payout: payouts[i],
                    },
                    ix::RegisterOffer { offer },
                )
            };
            let mut invalid = offer(i as u32);
            invalid.checkpoint = [0; 32];
            assert!(send(&mut svm, &[register(invalid)], &payer, &[provider]).is_err());
            send(&mut svm, &[register(offer(i as u32))], &payer, &[provider]).unwrap();
        }
        let fee_destinations = [
            create_token_account(&mut svm, &payer, mint, payer.pubkey()),
            litesvm_token::CreateAccount::new(&mut svm, &payer, &mint)
                .send()
                .unwrap(),
        ];
        let payment = litesvm_token::CreateAccount::new(&mut svm, &payer, &mint)
            .send()
            .unwrap();
        mint_to(&mut svm, &payer, mint, payment, 1);
        send(
            &mut svm,
            &[fees(market, payer.pubkey(), mint, true, 1)],
            &payer,
            &[],
        )
        .unwrap();
        let provider_list = Keypair::new();
        send(
            &mut svm,
            &[list(
                market,
                payer.pubkey(),
                provider_list.pubkey(),
                10,
                Some([payment, fee_destinations[0], mint]),
            )],
            &payer,
            &[&provider_list],
        )
        .unwrap();
        for provider in &providers {
            send(
                &mut svm,
                &[instruction(
                    inference_router_program::ID,
                    accounts::UpdateList {
                        market,
                        list: provider_list.pubkey(),
                        authority: payer.pubkey(),
                    },
                    ix::UpdateList {
                        provider: provider.pubkey(),
                        enabled: true,
                    },
                )],
                &payer,
                &[],
            )
            .unwrap();
        }
        let user = Keypair::new();
        let authority = Address::find_program_address(
            &[b"vault", user.pubkey().as_ref()],
            &inference_router_program::ID,
        )
        .0;
        let vault = create_token_account(&mut svm, &payer, mint, authority);
        mint_to(&mut svm, &payer, mint, payment, 100_000);
        let booking = Keypair::new();
        send(
            &mut svm,
            &[
                instruction(
                    inference_router_program::ID,
                    accounts::InitializeUser {
                        owner: payer.pubkey(),
                        market,
                        user: user.pubkey(),
                        authority,
                        vault,
                        system_program: solana_sdk_ids::system_program::ID,
                    },
                    ix::InitializeUser {},
                ),
                instruction(
                    inference_router_program::ID,
                    accounts::Deposit {
                        market,
                        user: user.pubkey(),
                        owner: payer.pubkey(),
                        source: payment,
                        authority,
                        vault,
                        mint,
                        token: anchor_spl::token::ID,
                    },
                    ix::Deposit { amount: 100_000 },
                ),
                instruction(
                    inference_router_program::ID,
                    accounts::InitializeReceipt {
                        market,
                        user: user.pubkey(),
                        owner: payer.pubkey(),
                        payer: payer.pubkey(),
                        booking: booking.pubkey(),
                        system_program: solana_sdk_ids::system_program::ID,
                    },
                    ix::InitializeReceipt {},
                ),
            ],
            &payer,
            &[&user, &booking],
        )
        .unwrap();
        Self {
            svm,
            payer,
            providers,
            payouts,
            market,
            model,
            model_id,
            terms: terms.pubkey(),
            capacity: capacity.pubkey(),
            list: provider_list.pubkey(),
            user: user.pubkey(),
            booking: booking.pubkey(),
            mint,
            vault,
            authority,
            fee_destinations,
        }
    }

    fn request(&self) -> args::ReserveRequest {
        args::ReserveRequest {
            model_id: self.model_id,
            input_rate_cap: 2_000_000,
            cached_rate_cap: 2_000_000,
            output_rate_cap: 2_000_000,
            min_speed: 1,
            max_input: 100,
            max_output: 100,
            private: true,
            spend: 1_000,
            estimated_input: 100,
            estimated_cached: 0,
            estimated_output: 100,
            cache_provider: Address::default(),
            checkpoint: [7; 32],
            input_modalities: MODALITY_TEXT,
            output_modalities: MODALITY_TEXT,
            min_context: 0,
            limits_configured: false,
        }
    }

    fn reserve(
        &self,
        request: args::ReserveRequest,
        generation: u64,
    ) -> solana_instruction::Instruction {
        instruction(
            inference_router_program::ID,
            accounts::ReserveBooking {
                market: self.market,
                model: self.model,
                terms: self.terms,
                capacity: self.capacity,
                user: self.user,
                owner: self.payer.pubkey(),
                booking: self.booking,
                rent_payer: self.payer.pubkey(),
                list: self.list,
                group: None,
            },
            ix::ReserveBooking {
                request,
                generation: Some(generation),
            },
        )
    }

    fn update(&self, offer: args::Offer, signer: Address) -> solana_instruction::Instruction {
        instruction(
            inference_router_program::ID,
            accounts::UpdateOffer {
                market: self.market,
                model: self.model,
                terms: self.terms,
                signer,
            },
            ix::UpdateOffer { offer },
        )
    }

    fn release(&self, provider: usize, generation: u64) -> solana_instruction::Instruction {
        instruction(
            inference_router_program::ID,
            accounts::ReleaseBooking {
                market: self.market,
                model: self.model,
                capacity: self.capacity,
                user: self.user,
                booking: self.booking,
                provider: self.providers[provider].pubkey(),
            },
            ix::ReleaseBooking {
                generation: Some(generation),
            },
        )
    }

    fn booked(&self) -> state_types::Booking {
        account(&self.svm, self.booking)
    }

    fn free(&self) -> u64 {
        account::<state_types::User>(&self.svm, self.user)
            .free
            .get()
    }

    fn entries<T: anchor_lang::bytemuck::Pod>(&self, address: Address, offset: usize) -> Vec<T> {
        anchor_lang::bytemuck::cast_slice(&self.svm.get_account(&address).unwrap().data[offset..])
            .to_vec()
    }

    fn terms(&self) -> Vec<state_types::TermsEntry> {
        self.entries(
            self.terms,
            Slab::<state_types::Terms, state_types::TermsEntry>::space_for(0),
        )
    }

    fn active(&self) -> Vec<u32> {
        self.entries::<state_types::CapacityEntry>(
            self.capacity,
            Slab::<state_types::Capacity, state_types::CapacityEntry>::space_for(0),
        )
        .iter()
        .map(|entry| entry.active.get())
        .collect()
    }
}

#[test]
fn matching_skips_incompatible_cheapest_offer_and_keeps_funds_on_rejection() {
    let mut f = FilterFixture::new();
    let changes: &[fn(&mut args::Offer)] = &[
        |o| o.checkpoint = [8; 32],
        |o| o.input_modalities = MODALITY_IMAGE,
        |o| o.output_modalities = MODALITY_AUDIO,
        |o| {
            o.context_window = 199;
        },
        |o| o.max_input = 99,
        |o| o.max_output = 99,
    ];
    for (i, change) in changes.iter().enumerate() {
        let mut incompatible = offer(0);
        change(&mut incompatible);
        let update = f.update(incompatible, f.providers[0].pubkey());
        send(&mut f.svm, &[update], &f.payer, &[&f.providers[0]]).unwrap();
        let generation = i as u64 + 1;
        let reserve = f.reserve(f.request(), generation);
        send(&mut f.svm, &[reserve], &f.payer, &[]).unwrap();
        assert_eq!(f.booked().provider, f.providers[1].pubkey());
        assert_eq!(f.active(), [0, 1]);
        assert_eq!(f.free(), 100_000 - f.booked().reserve.get());
        let release = f.release(1, generation);
        send(&mut f.svm, &[release], &f.payer, &[&f.providers[1]]).unwrap();
        assert_eq!(f.free(), 100_000);
        assert_eq!(f.active(), [0, 0]);
    }
    let changes: &[fn(&mut args::ReserveRequest)] = &[
        |r| r.checkpoint = [9; 32],
        |r| r.input_modalities = MODALITY_VIDEO,
        |r| r.output_modalities = MODALITY_VIDEO,
        |r| {
            r.max_input = 101;
            r.max_output = 99;
            r.estimated_output = 99;
        },
        |r| {
            r.max_input = 99;
            r.estimated_input = 99;
            r.max_output = 101;
        },
        |r| r.max_input = 101,
        |r| r.checkpoint = [0; 32],
        |r| r.input_modalities = 0,
        |r| r.output_modalities = 0,
        |r| r.input_modalities |= 16,
        |r| r.output_modalities |= 128,
        |r| r.max_input = u32::MAX,
        |r| r.max_output = u32::MAX,
    ];
    for change in changes {
        let before = f.svm.get_account(&f.booking).unwrap().data;
        let mut request = f.request();
        change(&mut request);
        let reserve = f.reserve(request, 7);
        assert!(send(&mut f.svm, &[reserve], &f.payer, &[]).is_err());
        assert_eq!(f.svm.get_account(&f.booking).unwrap().data, before);
        assert_eq!(f.free(), 100_000);
        assert_eq!(f.active(), [0, 0]);
    }
}

#[test]
fn book_growth_respects_runtime_account_resize_limit() {
    let mut f = FilterFixture::new();
    let before = f.svm.get_account(&f.terms).unwrap();
    let current = account::<state_types::Model>(&f.svm, f.model).limit.get();
    let grow = |limit| {
        instruction(
            inference_router_program::ID,
            accounts::GrowBooks {
                market: f.market,
                model: f.model,
                terms: f.terms,
                capacity: f.capacity,
                payer: f.payer.pubkey(),
                system_program: solana_sdk_ids::system_program::ID,
            },
            ix::GrowBooks { limit },
        )
    };
    let oversized = grow(current + 78);
    assert!(send(&mut f.svm, &[oversized], &f.payer, &[]).is_err());
    assert_eq!(f.svm.get_account(&f.terms).unwrap().data, before.data);
    let valid = grow(current + 77);
    send(&mut f.svm, &[valid], &f.payer, &[]).unwrap();
    assert_eq!(
        f.svm.get_account(&f.terms).unwrap().data.len(),
        before.data.len() + 77 * 132
    );
    assert_eq!(
        account::<state_types::Model>(&f.svm, f.model).limit.get(),
        current + 77
    );
}

#[test]
fn minimum_context_is_signed_and_enforced_before_reserving_funds() {
    let mut f = FilterFixture::new();
    let mut request = f.request();
    request.min_context = 201;
    let reserve = f.reserve(request, 1);
    assert!(send(&mut f.svm, &[reserve], &f.payer, &[]).is_err());
    request.limits_configured = true;
    let reserve = f.reserve(request, 1);
    assert!(send(&mut f.svm, &[reserve], &f.payer, &[]).is_err());
    assert_eq!(f.free(), 100_000);
    assert_eq!(f.active(), [0, 0]);
    let mut capable = offer(1);
    capable.context_window = 201;
    let update = f.update(capable, f.providers[1].pubkey());
    send(&mut f.svm, &[update], &f.payer, &[&f.providers[1]]).unwrap();
    let reserve = f.reserve(request, 1);
    send(&mut f.svm, &[reserve], &f.payer, &[]).unwrap();
    assert_eq!(f.booked().provider, f.providers[1].pubkey());
    assert_eq!(f.booked().offer_context_window.get(), 201);
    let release = f.release(1, 1);
    send(&mut f.svm, &[release], &f.payer, &[&f.providers[1]]).unwrap();
    assert_eq!(f.free(), 100_000);
}

#[test]
fn offer_updates_require_provider_authority_and_valid_capabilities() {
    let mut f = FilterFixture::new();
    let before = f.svm.get_account(&f.terms).unwrap().data;
    let update = f.update(offer(0), f.providers[1].pubkey());
    assert!(send(&mut f.svm, &[update], &f.payer, &[&f.providers[1]]).is_err());
    let update = f.update(offer(0), f.payer.pubkey());
    assert!(send(&mut f.svm, &[update], &f.payer, &[]).is_err());
    let changes: &[fn(&mut args::Offer)] = &[
        |o| o.checkpoint = [0; 32],
        |o| o.input_modalities = 0,
        |o| o.output_modalities = 0,
        |o| o.input_modalities |= 16,
        |o| o.output_modalities |= 128,
        |o| o.context_window = 0,
        |o| o.max_input = 0,
        |o| o.max_output = 0,
        |o| o.max_input = 201,
        |o| o.max_output = 201,
    ];
    for change in changes {
        let mut invalid = offer(0);
        change(&mut invalid);
        let update = f.update(invalid, f.providers[0].pubkey());
        assert!(send(&mut f.svm, &[update], &f.payer, &[&f.providers[0]]).is_err());
        assert_eq!(f.svm.get_account(&f.terms).unwrap().data, before);
    }
    let mut changed = offer(0);
    changed.checkpoint = [9; 32];
    changed.input_modalities = MODALITY_AUDIO;
    changed.output_modalities = MODALITY_AUDIO;
    changed.context_window = 400;
    changed.max_input = 300;
    changed.max_output = 200;
    let update = f.update(changed, f.providers[0].pubkey());
    send(&mut f.svm, &[update], &f.payer, &[&f.providers[0]]).unwrap();
    let updated = f.terms()[0];
    assert_eq!(updated.checkpoint, changed.checkpoint);
    assert_eq!(updated.input_modalities, changed.input_modalities);
    assert_eq!(updated.output_modalities, changed.output_modalities);
    assert_eq!(updated.context_window.get(), 400);
    assert_eq!(updated.max_input.get(), 300);
    assert_eq!(updated.max_output.get(), 200);
}

#[test]
fn booking_pins_offer_capabilities_through_update_claim_release_expiry_and_reuse() {
    let mut f = FilterFixture::new();
    let reserve = f.reserve(f.request(), 1);
    send(&mut f.svm, &[reserve], &f.payer, &[]).unwrap();
    let booked = f.booked();
    assert_eq!(booked.provider, f.providers[0].pubkey());
    assert_eq!(booked.checkpoint, [7; 32]);
    assert_eq!(
        (
            booked.offer_context_window.get(),
            booked.offer_max_input.get(),
            booked.offer_max_output.get()
        ),
        (200, 100, 100)
    );
    assert_eq!(
        (
            booked.offer_input_modalities,
            booked.offer_output_modalities
        ),
        (
            MODALITY_TEXT | MODALITY_IMAGE,
            MODALITY_TEXT | MODALITY_AUDIO
        )
    );
    assert_eq!(
        (
            booked.requested_input_modalities,
            booked.requested_output_modalities
        ),
        (MODALITY_TEXT, MODALITY_TEXT)
    );
    let mut changed = offer(0);
    changed.checkpoint = [9; 32];
    changed.input_modalities = MODALITY_AUDIO;
    changed.output_modalities = MODALITY_AUDIO;
    changed.context_window = 60;
    changed.max_input = 40;
    changed.max_output = 30;
    changed.input_rate = 2_000_000;
    changed.output_rate = 2_000_000;
    let snapshot = f.svm.get_account(&f.booking).unwrap().data;
    let update = f.update(changed, f.providers[0].pubkey());
    send(&mut f.svm, &[update], &f.payer, &[&f.providers[0]]).unwrap();
    assert_eq!(f.svm.get_account(&f.booking).unwrap().data, snapshot);
    let mut claim = instruction(
        inference_router_program::ID,
        accounts::ClaimBooking {
            market: f.market,
            model: f.model,
            capacity: f.capacity,
            user: f.user,
            booking: f.booking,
            provider: f.providers[0].pubkey(),
            vault: f.vault,
            payout: f.payouts[0],
            mint: f.mint,
            authority: f.authority,
            token: anchor_spl::token::ID,
        },
        ix::ClaimBooking {
            claim: args::Claim {
                uncached: 100,
                cached: 0,
                output: 100,
                receipt: Some(args::ReceiptInput {
                    owner: f.payer.pubkey(),
                    user: f.user,
                    booking: f.booking,
                    generation: 1,
                    charge: 101,
                    claim_end: booked.claim_end.get(),
                }),
            },
        },
    );
    claim.accounts.extend(
        f.fee_destinations
            .map(|address| solana_instruction::AccountMeta::new(address, false)),
    );
    send(&mut f.svm, &[claim.clone()], &f.payer, &[&f.providers[0]]).unwrap();
    assert_eq!(f.free(), 99_899);
    assert_eq!(token_account(&f.svm, f.payouts[0]).amount, 100);
    assert_eq!(token_account(&f.svm, f.fee_destinations[1]).amount, 1);
    assert_eq!(f.active(), [0, 0]);
    assert_eq!(f.booked().checkpoint, [7; 32]);
    assert!(send(&mut f.svm, &[claim], &f.payer, &[&f.providers[0]]).is_err());
    let mut request = f.request();
    request.checkpoint = changed.checkpoint;
    request.input_modalities = MODALITY_AUDIO;
    request.output_modalities = MODALITY_AUDIO;
    request.max_input = 40;
    request.max_output = 20;
    request.estimated_input = 40;
    request.estimated_output = 20;
    let reserve = f.reserve(request, 1);
    assert!(send(&mut f.svm, &[reserve], &f.payer, &[]).is_err());
    let reserve = f.reserve(request, 2);
    send(&mut f.svm, &[reserve], &f.payer, &[]).unwrap();
    assert_eq!(f.booked().checkpoint, [9; 32]);
    assert_eq!(f.booked().offer_context_window.get(), 60);
    assert_eq!(f.booked().requested_input_modalities, MODALITY_AUDIO);
    let stale_release = f.release(0, 1);
    assert!(send(&mut f.svm, &[stale_release], &f.payer, &[&f.providers[0]]).is_err());
    let release = f.release(0, 2);
    send(&mut f.svm, &[release], &f.payer, &[&f.providers[0]]).unwrap();
    assert_eq!(f.free(), 99_899);
    let reserve = f.reserve(request, 3);
    send(&mut f.svm, &[reserve], &f.payer, &[]).unwrap();
    let expire = instruction(
        inference_router_program::ID,
        accounts::ExpireBooking {
            market: f.market,
            model: f.model,
            capacity: f.capacity,
            user: f.user,
            booking: f.booking,
        },
        ix::ExpireBooking {
            generation: Some(3),
        },
    );
    assert!(send(&mut f.svm, &[expire.clone()], &f.payer, &[]).is_err());
    let mut clock: solana_clock::Clock = f.svm.get_sysvar();
    clock.unix_timestamp = f.booked().claim_end.get() + 1;
    f.svm.set_sysvar(&clock);
    send(&mut f.svm, &[expire], &f.payer, &[]).unwrap();
    assert_eq!(f.free(), 99_899);
    assert_eq!(f.active(), [0, 0]);
    assert_eq!(
        account::<state_types::User>(&f.svm, f.user)
            .generation
            .get(),
        3
    );
    let close = instruction(
        inference_router_program::ID,
        accounts::CloseBooking {
            booking: f.booking,
            recipient: f.payer.pubkey(),
        },
        ix::CloseBooking {},
    );
    send(&mut f.svm, &[close], &f.payer, &[]).unwrap();
    assert!(f
        .svm
        .get_account(&f.booking)
        .is_none_or(|account| account.lamports == 0));
}

#[test]
fn unconfigured_reserved_and_legacy_layouts_fail_closed() {
    let mut f = FilterFixture::new();
    let original = f.svm.get_account(&f.terms).unwrap();
    let offset = Slab::<state_types::Terms, state_types::TermsEntry>::space_for(0);
    let changes: &[fn(&mut state_types::TermsEntry)] = &[
        |o| o.version = 0,
        |o| o.version = 2,
        |o| o.reserved = 1,
        |o| o.checkpoint = [0; 32],
        |o| o.context_window = 0.into(),
        |o| o.input_modalities = 0,
        |o| o.output_modalities = 0,
        |o| o.input_modalities |= 128,
        |o| o.output_modalities |= 128,
    ];
    for change in changes {
        let mut altered = original.clone();
        let entries: &mut [state_types::TermsEntry] =
            anchor_lang::bytemuck::cast_slice_mut(&mut altered.data[offset..]);
        for entry in entries {
            change(entry);
        }
        f.svm.set_account(f.terms, altered).unwrap();
        let reserve = f.reserve(f.request(), 1);
        assert!(send(&mut f.svm, &[reserve], &f.payer, &[]).is_err());
        assert_eq!(f.free(), 100_000);
        assert_eq!(f.active(), [0, 0]);
    }
    // original anchor discriminators, retained only to prove old layouts are rejected.
    let legacy_terms_discriminator = anchor_lang::solana_program::hash::hash(b"account:Terms");
    let legacy_booking_discriminator = anchor_lang::solana_program::hash::hash(b"account:Booking");
    let mut legacy_terms = original.clone();
    legacy_terms.data[..8].copy_from_slice(&legacy_terms_discriminator.to_bytes()[..8]);
    f.svm.set_account(f.terms, legacy_terms).unwrap();
    let reserve = f.reserve(f.request(), 1);
    assert!(send(&mut f.svm, &[reserve], &f.payer, &[]).is_err());
    f.svm.set_account(f.terms, original).unwrap();
    let mut legacy_booking = f.svm.get_account(&f.booking).unwrap();
    legacy_booking.data[..8].copy_from_slice(&legacy_booking_discriminator.to_bytes()[..8]);
    f.svm.set_account(f.booking, legacy_booking).unwrap();
    let reserve = f.reserve(f.request(), 1);
    assert!(send(&mut f.svm, &[reserve], &f.payer, &[]).is_err());
    assert_eq!(f.free(), 100_000);
    assert_eq!(f.active(), [0, 0]);
    assert_eq!(core::mem::size_of::<state_types::TermsEntry>(), 132);
    assert_eq!(8 + core::mem::size_of::<state_types::Booking>(), 520);
}
