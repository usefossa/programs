use super::*;

pub(super) fn legacy_terms(svm: &mut LiteSVM, address: Address) {
    let mut account = svm.get_account(&address).unwrap();
    assert_eq!(&account.data[..8], state_types::Terms::DISCRIMINATOR);
    let mut data = account.data[..48].to_vec();
    // sha256("account:FilteredTerms")[..8], the archived baseline discriminator.
    data[..8].copy_from_slice(&[136, 206, 216, 233, 146, 248, 21, 65]);
    let (entries, remainder) = account.data[48..].as_chunks::<136>();
    assert!(remainder.is_empty());
    for entry in entries {
        data.extend_from_slice(&entry[..132]);
    }
    account.data = data;
    svm.set_account(address, account).unwrap();
}

fn transaction(
    svm: &LiteSVM,
    payer: &Keypair,
    ix: &solana_instruction::Instruction,
) -> Transaction {
    // fixture provider addresses have no private keys; message signer checks still run in SBF.
    let mut tx = Transaction::new_unsigned(Message::new(
        std::slice::from_ref(ix),
        Some(&payer.pubkey()),
    ));
    tx.message.recent_blockhash = svm.latest_blockhash();
    tx
}

fn measure_instruction(
    svm: &LiteSVM,
    payer: &Keypair,
    ix: &solana_instruction::Instruction,
    mode: &str,
    name: &str,
    descending: bool,
) -> litesvm::types::SimulatedTransactionInfo {
    let tx = transaction(svm, payer, ix);
    let results: Vec<_> = (0..3)
        .map(|_| {
            svm.simulate_transaction(tx.clone())
                .unwrap_or_else(|e| panic!("{mode} {name}: {e:?}"))
        })
        .collect();
    let cu = results[0].meta.compute_units_consumed;
    assert!(results
        .iter()
        .all(|result| result.meta.compute_units_consumed == cu));
    eprintln!(
        "LIFECYCLE_CU mode={mode} instruction={name} offers=32 order={} cu={cu} samples=3",
        if descending { "last" } else { "first" }
    );
    results.into_iter().next().unwrap()
}

pub(super) fn post<T: AccountDeserialize>(
    result: &litesvm::types::SimulatedTransactionInfo,
    key: Address,
) -> T {
    let account = &result
        .post_accounts
        .iter()
        .find(|(address, _)| *address == key)
        .unwrap()
        .1;
    T::try_deserialize(&mut account.data()).unwrap()
}

fn commit(svm: &mut LiteSVM, payer: &Keypair, ix: &solana_instruction::Instruction) {
    svm.expire_blockhash();
    svm.send_transaction(transaction(svm, payer, ix)).unwrap();
}

#[test]
#[ignore = "explicit matched baseline/final lifecycle SBF CU comparison"]
fn booking_lifecycle_cu_benchmark() {
    let baseline = std::env::var_os("FOSSA_CU_BASELINE")
        .expect("FOSSA_CU_BASELINE must name the archived pre-change SBF");
    let mut template = LiteSVM::new().with_sigverify(false);
    template
        .add_program_from_file(
            inference_router_program::ID,
            program_path("inference_router_program.so"),
        )
        .unwrap();
    let payer = Keypair::new_from_array([71; 32]);
    template.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    let mint = create_mint(&mut template, &payer, payer.pubkey(), 6);
    let fee_destination = create_token_account(&mut template, &payer, mint, payer.pubkey());

    for descending in [false, true] {
        let mut prepared = template.clone();
        let (reserve, booking) = fixture(
            &mut prepared,
            &payer,
            32,
            descending,
            Case::Compatible,
            false,
        );
        let [market, model, terms, _capacity, user] =
            std::array::from_fn(|index| reserve.accounts[index].pubkey);
        let wallet = reserve.accounts[10].pubkey;
        let slot = if descending { 31usize } else { 0 };
        let terms_account = prepared.get_account(&terms).unwrap();
        let offers: &[state_types::TermsEntry] =
            anchor_lang::bytemuck::cast_slice(&terms_account.data[48..]);
        let provider = offers[slot].provider;
        let payout = create_token_account(&mut prepared, &payer, mint, provider);
        let (authority, bump) = Address::find_program_address(
            &[b"vault", user.as_ref()],
            &inference_router_program::ID,
        );
        let vault = create_token_account(&mut prepared, &payer, mint, authority);
        mint_to(&mut prepared, &payer, mint, vault, 1_000_000_000);
        let mut state: state_types::Market = account(&prepared, market);
        state.mint = mint;
        store_account(&mut prepared, market, &state);
        let mut state: state_types::User = account(&prepared, user);
        state.vault = vault;
        state.bump = bump;
        store_account(&mut prepared, user, &state);
        let mut terms_account = prepared.get_account(&terms).unwrap();
        let offers: &mut [state_types::TermsEntry] =
            anchor_lang::bytemuck::cast_slice_mut(&mut terms_account.data[48..]);
        offers[slot].payout = payout;
        prepared.set_account(terms, terms_account).unwrap();

        for mode in [
            "baseline",
            "quota_disabled",
            "quota_dense",
            "scoped_disabled",
            "scoped_dense",
        ] {
            let scoped = mode.starts_with("scoped");
            let legacy = mode == "baseline";
            let mut svm = prepared.clone();
            let mut reserve = reserve.clone();
            if legacy {
                svm.add_program_from_file(inference_router_program::ID, &baseline)
                    .unwrap();
                legacy_terms(&mut svm, terms);
                reserve.accounts.drain(9..11);
            } else if !scoped {
                svm.add_program_from_file(
                    inference_router_program::ID,
                    std::env::var_os("FOSSA_PRE_QUOTA_SBF")
                        .expect("FOSSA_PRE_QUOTA_SBF must name the historical fixture program"),
                )
                .unwrap();
            }
            if mode == "quota_dense" {
                let policies = Address::find_program_address(
                    &[b"booking_policies", market.as_ref()],
                    &inference_router_program::ID,
                )
                .0;
                let mut stored = svm.get_account(&policies).unwrap();
                stored.data[40..44].copy_from_slice(&32u32.to_le_bytes());
                for entry in anchor_lang::bytemuck::cast_slice_mut::<_, state_types::BookingPolicy>(
                    &mut stored.data[52..],
                ) {
                    entry.wallet_quota = 10.into();
                    entry.wallet_concurrency = 10.into();
                }
                svm.set_account(policies, stored).unwrap();
                let header: state_types::WalletBookings = account(&svm, wallet);
                let entries: Vec<_> = (0..32)
                    .map(|index| state_types::WalletBookingEntry {
                        provider_policy: index.into(),
                        quota_window_end: i64::MAX.into(),
                        quota_used: 1.into(),
                        active_bookings: 0.into(),
                    })
                    .collect();
                store_slab(&mut svm, wallet, &header, &entries, 32);
            }
            if scoped {
                reserve = super::scoped_bench::convert(
                    &mut svm,
                    &payer,
                    &reserve,
                    mode == "scoped_dense",
                    mode == "scoped_dense",
                    0,
                );
            }
            let capacity = reserve.accounts[3].pubkey;
            let wallet = if scoped {
                reserve.accounts[10].pubkey
            } else {
                wallet
            };
            let result =
                measure_instruction(&svm, &payer, &reserve, mode, "reserve_one_shot", descending);
            let booked: state_types::Booking = post(&result, booking);
            assert_eq!(booked.slot.get(), slot as u32);
            assert_eq!(
                booked.wallet_booking_counted,
                if scoped { 2 } else { u8::from(!legacy) }
            );
            let request: args::ReserveRequest =
                anchor_lang::wincode::deserialize(&reserve.data[8..]).unwrap();
            reserve.data = if scoped {
                ix::ReserveListBooking {
                    minimum_wallet_concurrency: None,
                    request,
                    generation: Some(1),
                }
                .data()
            } else {
                ix::ReserveBooking {
                    request,
                    generation: Some(1),
                }
                .data()
            };
            measure_instruction(
                &svm,
                &payer,
                &reserve,
                mode,
                "reserve_receipt_initial",
                descending,
            );
            commit(&mut svm, &payer, &reserve);
            let booked: state_types::Booking = account(&svm, booking);
            assert_eq!(booked.generation.get(), 1);
            let mut claim = instruction(
                inference_router_program::ID,
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
                        uncached: 512,
                        cached: 0,
                        output: 256,
                        receipt: Some(args::ReceiptInput {
                            owner: payer.pubkey(),
                            user,
                            booking,
                            generation: 1,
                            charge: 776,
                            claim_end: booked.claim_end.get(),
                        }),
                    },
                },
            );
            if !scoped {
                claim.data[..8].copy_from_slice(ix::ClaimBooking::DISCRIMINATOR);
            }
            if legacy {
                claim.accounts.pop();
            }
            claim.accounts.extend([
                solana_instruction::AccountMeta::new(fee_destination, false),
                solana_instruction::AccountMeta::new(fee_destination, false),
            ]);
            let result =
                measure_instruction(&svm, &payer, &claim, mode, "claim_receipt", descending);
            let claimed: state_types::Booking = post(&result, booking);
            assert_eq!((claimed.status, claimed.charge.get()), (5, 776));
            let mut claimed_svm = svm.clone();
            commit(&mut claimed_svm, &payer, &claim);
            assert_eq!(token_account(&claimed_svm, payout).amount, 768);
            assert_eq!(token_account(&claimed_svm, fee_destination).amount, 8);
            let mut release = instruction(
                inference_router_program::ID,
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
                    generation: Some(1),
                },
            );
            if !scoped {
                release.data[..8].copy_from_slice(ix::ReleaseBooking::DISCRIMINATOR);
            }
            if legacy {
                release.accounts.pop();
            }
            let result =
                measure_instruction(&svm, &payer, &release, mode, "release_receipt", descending);
            assert_eq!(post::<state_types::Booking>(&result, booking).status, 5);
            let mut expired_svm = svm.clone();
            let mut clock: solana_clock::Clock = expired_svm.get_sysvar();
            clock.unix_timestamp = booked.claim_end.get() + 1;
            expired_svm.set_sysvar(&clock);
            let mut expire = instruction(
                inference_router_program::ID,
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
                    generation: Some(1),
                },
            );
            if !scoped {
                expire.data[..8].copy_from_slice(ix::ExpireBooking::DISCRIMINATOR);
            }
            if legacy {
                expire.accounts.pop();
            }
            let result = measure_instruction(
                &expired_svm,
                &payer,
                &expire,
                mode,
                "expire_receipt",
                descending,
            );
            assert_eq!(post::<state_types::Booking>(&result, booking).status, 5);
            commit(&mut svm, &payer, &release);
            if scoped {
                let stored = svm.get_account(&wallet).unwrap();
                let len = u32::from_le_bytes(stored.data[108..112].try_into().unwrap()) as usize;
                let entries: &[state_types::ProviderUsage] =
                    anchor_lang::bytemuck::cast_slice(&stored.data[112..112 + len * 48]);
                assert_eq!(
                    entries
                        .iter()
                        .find(|entry| entry.provider == provider)
                        .unwrap()
                        .active_bookings
                        .get(),
                    0
                );
            } else if !legacy {
                let stored = svm.get_account(&wallet).unwrap();
                let len = u32::from_le_bytes(stored.data[108..112].try_into().unwrap()) as usize;
                let entries: &[state_types::WalletBookingEntry] =
                    anchor_lang::bytemuck::cast_slice(&stored.data[112..112 + len * 20]);
                assert_eq!(
                    entries
                        .iter()
                        .find(|entry| entry.provider_policy.get() == slot as u32)
                        .unwrap()
                        .active_bookings
                        .get(),
                    0
                );
            }
            reserve.data = if scoped {
                ix::ReserveListBooking {
                    minimum_wallet_concurrency: None,
                    request,
                    generation: Some(2),
                }
                .data()
            } else {
                ix::ReserveBooking {
                    request,
                    generation: Some(2),
                }
                .data()
            };
            let result = measure_instruction(
                &svm,
                &payer,
                &reserve,
                mode,
                "reserve_receipt_reuse",
                descending,
            );
            assert_eq!(
                post::<state_types::Booking>(&result, booking)
                    .generation
                    .get(),
                2
            );
            let mut close = instruction(
                inference_router_program::ID,
                accounts::CloseBooking {
                    booking,
                    recipient: payer.pubkey(),
                },
                ix::CloseBooking {},
            );
            close.accounts[1].is_signer = true;
            measure_instruction(&svm, &payer, &close, mode, "close_receipt", descending);
        }
        if !descending {
            measure_provider_instructions(&prepared, &payer, &reserve, &baseline);
        }
    }
}

fn measure_provider_instructions(
    prepared: &LiteSVM,
    payer: &Keypair,
    reserve: &solana_instruction::Instruction,
    baseline: &std::ffi::OsStr,
) {
    let [market, model, terms, capacity] =
        std::array::from_fn(|index| reserve.accounts[index].pubkey);
    let mut prepared = prepared.clone();
    let mut stored = prepared.get_account(&terms).unwrap();
    let entry = *anchor_lang::bytemuck::from_bytes::<state_types::TermsEntry>(
        &stored.data[48 + 31 * 136..48 + 32 * 136],
    );
    let provider = entry.provider;
    let market_state: state_types::Market = account(&prepared, market);
    let payout = create_token_account(&mut prepared, payer, market_state.mint, provider);
    let (profile, bump) = Address::find_program_address(
        &[b"provider", market.as_ref(), provider.as_ref()],
        &inference_router_program::ID,
    );
    let mut profile_state = state_types::Provider::zeroed();
    profile_state.market = market;
    profile_state.authority = provider;
    profile_state.bump = bump;
    profile_state.policy_registered = 1;
    store_account(&mut prepared, profile, &profile_state);
    stored.data[44..48].copy_from_slice(&31u32.to_le_bytes());
    stored.data[48 + 31 * 136..].fill(0);
    prepared.set_account(terms, stored).unwrap();
    let mut stored = prepared.get_account(&capacity).unwrap();
    stored.data[44..48].copy_from_slice(&31u32.to_le_bytes());
    prepared.set_account(capacity, stored).unwrap();
    let offer = args::Offer {
        slot: 31,
        input_rate: entry.input_rate.get(),
        cached_rate: entry.cached_rate.get(),
        output_rate: entry.output_rate.get(),
        speed: entry.speed.get(),
        privacy: true,
        active: true,
        capacity: 10,
        checkpoint: entry.checkpoint,
        input_modalities: entry.input_modalities,
        output_modalities: entry.output_modalities,
        context_window: entry.context_window.get(),
        max_input: entry.max_input.get(),
        max_output: entry.max_output.get(),
    };
    measure_scoped_provider(&prepared, payer, reserve, provider, payout, offer);
    for legacy in [true, false] {
        let mode = if legacy { "baseline" } else { "quota_dense" };
        let mut svm = prepared.clone();
        if !legacy {
            svm.add_program_from_file(
                inference_router_program::ID,
                std::env::var_os("FOSSA_PRE_QUOTA_SBF")
                    .expect("FOSSA_PRE_QUOTA_SBF must name the historical fixture program"),
            )
            .unwrap();
        }
        let mut register = instruction(
            inference_router_program::ID,
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
        if legacy {
            svm.add_program_from_file(inference_router_program::ID, baseline)
                .unwrap();
            legacy_terms(&mut svm, terms);
            profile_state.policy_registered = 0;
            store_account(&mut svm, profile, &profile_state);
        }
        if !legacy {
            register
                .accounts
                .push(solana_instruction::AccountMeta::new_readonly(
                    Address::find_program_address(
                        &[b"booking_policies", market.as_ref()],
                        &inference_router_program::ID,
                    )
                    .0,
                    false,
                ));
        }
        let result =
            measure_instruction(&svm, payer, &register, mode, "register_offer_slot31", false);
        let terms_post = &result
            .post_accounts
            .iter()
            .find(|(key, _)| *key == terms)
            .unwrap()
            .1;
        assert_eq!(
            u32::from_le_bytes(terms_post.data()[44..48].try_into().unwrap()),
            32
        );
        commit(&mut svm, payer, &register);
        let update = instruction(
            inference_router_program::ID,
            accounts::UpdateOffer {
                market,
                model,
                terms,
                signer: provider,
            },
            ix::UpdateOffer { offer },
        );
        measure_instruction(&svm, payer, &update, mode, "update_offer_slot31", false);
        let update = instruction(
            inference_router_program::ID,
            accounts::UpdateProfile {
                market,
                profile,
                signer: provider,
            },
            ix::UpdateProfile {
                metadata: args::Metadata {
                    document_hash: [9; 32],
                    uri: "https://example.com/provider.json".into(),
                },
            },
        );
        let result = measure_instruction(&svm, payer, &update, mode, "update_profile", false);
        assert_eq!(
            post::<state_types::Provider>(&result, profile).policy_registered,
            u8::from(!legacy)
        );
    }
}

fn measure_scoped_provider(
    prepared: &LiteSVM,
    payer: &Keypair,
    reserve: &solana_instruction::Instruction,
    provider: Address,
    payout: Address,
    offer: args::Offer,
) {
    let mut svm = prepared.clone();
    let scoped = super::scoped_bench::convert(&mut svm, payer, reserve, true, true, 0);
    let market = scoped.accounts[0].pubkey;
    let model = scoped.accounts[1].pubkey;
    let terms = scoped.accounts[2].pubkey;
    let capacity = scoped.accounts[3].pubkey;
    let list = scoped.accounts[8].pubkey;
    let routing = scoped.accounts[9].pubkey;
    let mut header: state_types::ListTerms = account(&svm, terms);
    header.limit = 32.into();
    header.next_order = 31.into();
    let data = svm.get_account(&terms).unwrap();
    let entries: &[state_types::ListOffer] = anchor_lang::bytemuck::cast_slice(&data.data[120..]);
    store_slab(&mut svm, terms, &header, entries, 32);
    let mut header: state_types::ListCapacity = account(&svm, capacity);
    header.limit = 32.into();
    let data = svm.get_account(&capacity).unwrap();
    let entries: &[state_types::ListCapacityEntry] =
        anchor_lang::bytemuck::cast_slice(&data.data[116..]);
    store_slab(&mut svm, capacity, &header, entries, 32);
    let header: state_types::ListRouting = account(&svm, routing);
    let data = svm.get_account(&routing).unwrap();
    let mut entries: Vec<state_types::PolicyMirror> =
        anchor_lang::bytemuck::cast_slice(&data.data[84..]).to_vec();
    entries.push(state_types::PolicyMirror {
        provider,
        wallet_quota_window_seconds: 60.into(),
        wallet_quota: 10.into(),
        wallet_concurrency: 10.into(),
        epoch: 1.into(),
        active: 1,
        reserved: [0; 3],
    });
    entries.sort_by_key(|entry| entry.provider);
    store_slab(&mut svm, routing, &header, &entries, 32);
    let (budget, bump) = Address::find_program_address(
        &[b"provider_capacity", model.as_ref(), provider.as_ref()],
        &inference_router_program::ID,
    );
    let mut state = state_types::ProviderModelCapacity::zeroed();
    state.model = model;
    state.provider = provider;
    state.total = 20.into();
    state.legacy_slot = u32::MAX.into();
    state.bump = bump;
    store_account(&mut svm, budget, &state);
    let set = instruction(
        inference_router_program::ID,
        accounts::SetListOffer {
            market,
            model,
            provider,
            identity: Address::find_program_address(
                &[b"provider_limits", market.as_ref(), provider.as_ref()],
                &inference_router_program::ID,
            )
            .0,
            list,
            routing,
            terms,
            capacity,
            payout,
        },
        ix::SetListOffer { offer },
    );
    let result = measure_instruction(
        &svm,
        payer,
        &set,
        "scoped_dense",
        "register_offer_slot31",
        false,
    );
    assert_eq!(
        post::<state_types::ProviderModelCapacity>(&result, budget)
            .allocated
            .get(),
        10
    );
    commit(&mut svm, payer, &set);
    measure_instruction(
        &svm,
        payer,
        &set,
        "scoped_dense",
        "update_offer_slot31",
        false,
    );
}
