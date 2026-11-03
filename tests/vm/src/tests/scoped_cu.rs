use super::*;

pub(super) fn convert(
    svm: &mut LiteSVM,
    payer: &Keypair,
    reserve: &solana_instruction::Instruction,
    enabled: bool,
    populated: bool,
    unrelated: usize,
) -> solana_instruction::Instruction {
    let market = reserve.accounts[0].pubkey;
    let model = reserve.accounts[1].pubkey;
    let list = reserve.accounts[8].pubkey;
    let original = svm.get_account(&reserve.accounts[2].pubkey).unwrap();
    let original_offers: &[state_types::TermsEntry] =
        anchor_lang::bytemuck::cast_slice(&original.data[48..]);
    let listed = svm.get_account(&list).unwrap();
    let listed_count = u32::from_le_bytes(listed.data[84..88].try_into().unwrap()) as usize;
    let providers: &[Address] =
        anchor_lang::bytemuck::cast_slice(&listed.data[88..88 + listed_count * 32]);
    let mut offers: Vec<_> = original_offers
        .iter()
        .enumerate()
        .filter(|(_, offer)| providers.binary_search(&offer.provider).is_ok())
        .map(|(index, offer)| state_types::ListOffer {
            terms: *offer,
            order: (index as u32).into(),
        })
        .collect();
    offers.sort_by_key(|offer| offer.terms.provider);
    let derive =
        |seeds: &[&[u8]]| Address::find_program_address(seeds, &inference_router_program::ID);
    let (terms, terms_bump) = derive(&[b"list_terms", list.as_ref(), model.as_ref()]);
    let (capacity, capacity_bump) = derive(&[b"list_capacity", list.as_ref(), model.as_ref()]);
    let (routing, routing_bump) = derive(&[b"list_routing", list.as_ref()]);
    let (wallet, wallet_bump) =
        derive(&[b"provider_wallet", market.as_ref(), payer.pubkey().as_ref()]);
    let count = offers.len().max(1) as u32;
    let mut header = state_types::ListTerms::zeroed();
    header.model = model;
    header.list = list;
    header.capacity = capacity;
    header.limit = count.into();
    header.next_order = (original_offers.len() as u32).into();
    header.bump = terms_bump;
    store_slab(svm, terms, &header, &offers, count);
    let mut header = state_types::ListCapacity::zeroed();
    header.model = model;
    header.list = list;
    header.terms = terms;
    header.limit = count.into();
    header.bump = capacity_bump;
    let entries: Vec<_> = offers
        .iter()
        .map(|offer| state_types::ListCapacityEntry {
            provider: offer.terms.provider,
            active: 0.into(),
        })
        .collect();
    store_slab(svm, capacity, &header, &entries, count);
    let mut header = state_types::ListRouting::zeroed();
    header.market = market;
    header.list = list;
    header.book_count = 1.into();
    header.bump = routing_bump;
    let mirrors: Vec<_> = offers
        .iter()
        .map(|offer| state_types::PolicyMirror {
            provider: offer.terms.provider,
            wallet_quota_window_seconds: 60.into(),
            wallet_quota: (if enabled { 10 } else { 0 }).into(),
            wallet_concurrency: (if enabled { 10 } else { 0 }).into(),
            epoch: 1.into(),
            active: 1,
            reserved: [0; 3],
        })
        .collect();
    store_slab(svm, routing, &header, &mirrors, count);
    let mut header = state_types::ProviderWallet::zeroed();
    header.market = market;
    header.owner = payer.pubkey();
    header.rent_payer = payer.pubkey();
    header.bump = wallet_bump;
    let mut usage: Vec<_> = offers
        .iter()
        .filter(|_| populated)
        .map(|offer| state_types::ProviderUsage {
            provider: offer.terms.provider,
            quota_window_end: i64::MAX.into(),
            quota_used: 1.into(),
            active_bookings: 0.into(),
        })
        .collect();
    for index in 0..unrelated {
        let mut key = offers[index % offers.len()].terms.provider.to_bytes();
        key[31] = 34;
        usage.push(state_types::ProviderUsage {
            provider: Address::new_from_array(key),
            quota_window_end: i64::MAX.into(),
            quota_used: 1.into(),
            active_bookings: 0.into(),
        });
    }
    usage.sort_by_key(|entry| entry.provider);
    store_slab(svm, wallet, &header, &usage, (usage.len() as u32).max(8));
    let mut state: state_types::Market = account(svm, market);
    state.reserved[0] = 1;
    store_account(svm, market, &state);
    let mut listed = svm.get_account(&list).unwrap();
    listed.data[80] = 1;
    svm.set_account(list, listed).unwrap();
    let request: args::ReserveRequest =
        anchor_lang::wincode::deserialize(&reserve.data[8..]).unwrap();
    instruction(
        inference_router_program::ID,
        accounts::ReserveListBooking {
            market,
            model,
            terms,
            limits_page: capacity,
            user: reserve.accounts[4].pubkey,
            owner: payer.pubkey(),
            booking: reserve.accounts[6].pubkey,
            rent_payer: reserve.accounts[7].pubkey,
            list,
            routing,
            wallet,
            group: None,
        },
        ix::ReserveListBooking {
            minimum_wallet_concurrency: None,
            request,
            generation: None,
        },
    )
}

#[test]
#[ignore = "explicit scoped routing SBF CU and unlisted spam comparison"]
fn scoped_routing_cu_benchmark() {
    let mut svm = LiteSVM::new();
    let mut budget = solana_compute_budget::compute_budget::ComputeBudget::new_with_defaults(true);
    budget.compute_unit_limit = 1_400_000;
    svm = svm.with_compute_budget(budget);
    svm.add_program_from_file(
        inference_router_program::ID,
        program_path("inference_router_program.so"),
    )
    .unwrap();
    let payer = Keypair::new_from_array([71; 32]);
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    for count in [32usize, 4096] {
        for descending in [false, true] {
            for scenario in [
                "enabled_dense",
                "enabled_sparse",
                "all_exhausted",
                "all_exhausted_sparse",
                "last_eligible",
                "last_eligible_sparse",
            ] {
                let (old, booking) =
                    fixture(&mut svm, &payer, count, descending, Case::Compatible, false);
                let reserve = convert(
                    &mut svm,
                    &payer,
                    &old,
                    true,
                    true,
                    if scenario.ends_with("sparse") {
                        count
                    } else {
                        0
                    },
                );
                let mut stored = svm.get_account(&reserve.accounts[10].pubkey).unwrap();
                let entries: &mut [state_types::ProviderUsage] =
                    anchor_lang::bytemuck::cast_slice_mut(&mut stored.data[112..]);
                let mut last_provider = None;
                if scenario.starts_with("all_exhausted") || scenario.starts_with("last_eligible") {
                    for entry in entries.iter_mut() {
                        entry.quota_used = 10.into();
                    }
                    if scenario.starts_with("last_eligible") {
                        let last = entries
                            .iter_mut()
                            .rev()
                            .find(|entry| entry.provider.as_ref()[31] == 33)
                            .unwrap();
                        last.quota_used = 1.into();
                        last_provider = Some(last.provider);
                    }
                }
                svm.set_account(reserve.accounts[10].pubkey, stored)
                    .unwrap();
                let tx = Transaction::new(
                    &[&payer],
                    Message::new(&[reserve], Some(&payer.pubkey())),
                    svm.latest_blockhash(),
                );
                let mut samples = vec![];
                let mut loaded = 0;
                for _ in 0..3 {
                    match svm.simulate_transaction(tx.clone()) {
                        Ok(result) => {
                            assert!(!scenario.starts_with("all_exhausted"));
                            let post = &result
                                .post_accounts
                                .iter()
                                .find(|(key, _)| *key == booking)
                                .unwrap()
                                .1;
                            let booked =
                                state_types::Booking::try_deserialize(&mut post.data()).unwrap();
                            if let Some(provider) = last_provider {
                                assert_eq!(booked.provider, provider);
                            }
                            loaded = result.meta.loaded_accounts_data_size;
                            samples.push(result.meta.compute_units_consumed);
                        }
                        Err(error) => {
                            assert!(
                                scenario.starts_with("all_exhausted")
                                    && format!("{:?}", error.err).contains("Custom(6005)"),
                                "{scenario}: {error:?}"
                            );
                            loaded = error.meta.loaded_accounts_data_size;
                            samples.push(error.meta.compute_units_consumed);
                        }
                    }
                }
                assert!(samples.iter().all(|cu| *cu == samples[0]));
                eprintln!("SCOPED_CU scenario={scenario} offers={count} order={} cu={} loaded_bytes={loaded} samples=3",if descending {"last"}else{"first"},samples[0]);
            }
        }
    }
    let mut reference = None;
    for spam in [0usize, 4094] {
        let (old, _) = fixture(&mut svm, &payer, spam + 2, false, Case::Compatible, false);
        let original = svm.get_account(&old.accounts[2].pubkey).unwrap();
        let offers: &[state_types::TermsEntry] =
            anchor_lang::bytemuck::cast_slice(&original.data[48..]);
        let mut providers = vec![offers[0].provider, offers[1].provider];
        providers.sort();
        let mut list: state_types::List = account(&svm, old.accounts[8].pubkey);
        list.limit = 2.into();
        store_slab(&mut svm, old.accounts[8].pubkey, &list, &providers, 2);
        let reserve = convert(&mut svm, &payer, &old, true, true, 0);
        let tx = Transaction::new(
            &[&payer],
            Message::new(&[reserve], Some(&payer.pubkey())),
            svm.latest_blockhash(),
        );
        for _ in 0..3 {
            let result = svm.simulate_transaction(tx.clone()).unwrap();
            let value = (
                result.meta.compute_units_consumed,
                result.meta.loaded_accounts_data_size,
            );
            if let Some(reference) = reference {
                assert_eq!(value, reference);
            } else {
                reference = Some(value);
            }
        }
        let (cu, loaded) = reference.unwrap();
        eprintln!("SCOPED_SPAM unlisted={spam} admitted=2 cu={cu} loaded_bytes={loaded} samples=3");
    }
}

#[test]
#[ignore = "explicit provider-order worst case and large wallet history SBF CU"]
fn scoped_ordered_and_history_cu_benchmark() {
    let mut budget = solana_compute_budget::compute_budget::ComputeBudget::new_with_defaults(true);
    budget.compute_unit_limit = 1_400_000;
    let mut svm = LiteSVM::new().with_compute_budget(budget);
    svm.add_program_from_file(
        inference_router_program::ID,
        program_path("inference_router_program.so"),
    )
    .unwrap();
    let payer = Keypair::new_from_array([71; 32]);
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    for count in [32usize, 4096] {
        let (old, booking) = fixture(&mut svm, &payer, count, false, Case::Compatible, false);
        let reserve = convert(&mut svm, &payer, &old, true, true, count);
        let mut stored = svm.get_account(&reserve.accounts[2].pubkey).unwrap();
        let offers: &mut [state_types::ListOffer] =
            anchor_lang::bytemuck::cast_slice_mut(&mut stored.data[120..]);
        for (index, row) in offers.iter_mut().enumerate() {
            let rate = ((count - index) as u32) * 1_000_000;
            row.terms.input_rate = rate.into();
            row.terms.cached_rate = rate.into();
            row.terms.output_rate = rate.into();
        }
        let expected = offers.last().unwrap().terms.provider;
        svm.set_account(reserve.accounts[2].pubkey, stored).unwrap();
        let tx = Transaction::new(
            &[&payer],
            Message::new(&[reserve], Some(&payer.pubkey())),
            svm.latest_blockhash(),
        );
        let mut previous = None;
        for _ in 0..3 {
            let result = svm.simulate_transaction(tx.clone()).unwrap();
            let post = &result
                .post_accounts
                .iter()
                .find(|(key, _)| *key == booking)
                .unwrap()
                .1;
            assert_eq!(
                state_types::Booking::try_deserialize(&mut post.data())
                    .unwrap()
                    .provider,
                expected
            );
            let cu = result.meta.compute_units_consumed;
            if let Some(previous) = previous {
                assert_eq!(cu, previous);
            }
            previous = Some(cu);
        }
        eprintln!("SCOPED_ORDERED offers={count} wallet_entries={} descending=true sparse=true cu={} samples=3",count*2,previous.unwrap());
    }
    for history in [0usize, 100_000] {
        let (old, _) = fixture(&mut svm, &payer, 2, false, Case::Compatible, false);
        let reserve = convert(&mut svm, &payer, &old, true, true, 0);
        let key = reserve.accounts[10].pubkey;
        let header: state_types::ProviderWallet = account(&svm, key);
        let data = svm.get_account(&key).unwrap();
        let mut entries: Vec<state_types::ProviderUsage> =
            anchor_lang::bytemuck::cast_slice(&data.data[112..112 + 2 * 48]).to_vec();
        for index in 0..history {
            let mut key = [0; 32];
            key[28..].copy_from_slice(&(index as u32).to_be_bytes());
            entries.push(state_types::ProviderUsage {
                provider: Address::new_from_array(key),
                quota_window_end: i64::MAX.into(),
                quota_used: 1.into(),
                active_bookings: 0.into(),
            });
        }
        entries.sort_by_key(|entry| entry.provider);
        store_slab(&mut svm, key, &header, &entries, entries.len() as u32);
        let tx = Transaction::new(
            &[&payer],
            Message::new(&[reserve], Some(&payer.pubkey())),
            svm.latest_blockhash(),
        );
        let mut previous = None;
        let mut loaded = 0;
        for _ in 0..3 {
            let result = svm.simulate_transaction(tx.clone()).unwrap();
            let cu = result.meta.compute_units_consumed;
            if let Some(previous) = previous {
                assert_eq!(cu, previous);
            }
            previous = Some(cu);
            loaded = result.meta.loaded_accounts_data_size;
        }
        eprintln!(
            "SCOPED_HISTORY unrelated={history} admitted=2 cu={} loaded_bytes={loaded} samples=3",
            previous.unwrap()
        );
    }
}
