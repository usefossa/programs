use super::*;

#[test]
fn hundred_provider_limits_cu() {
    const COUNT: usize = 100;
    for pages in [1, 4, 16, 32] {
        for scenario in [
            "ordinary_all",
            "ordinary_winner_only",
            "ordinary_provider_quota_disabled",
            "provider_concurrency_full",
            "provider_concurrency_last",
            "wallet_concurrency_full",
            "wallet_quota_full",
            "provider_quota_full",
            "strict_descending_all",
            "strict_descending_winner_only",
        ] {
            let mut baseline_winner = None;
            for changed in modes() {
                let (mut svm, payer) = vm(changed);
                let (mut reserve, booking_key) =
                    prepare(&mut svm, &payer, changed, COUNT, false, true, true, 0, 0);
                let keys = if changed {
                    distribute(&mut svm, &mut reserve, pages)
                } else {
                    Vec::new()
                };
                let terms = reserve.accounts[2].pubkey;
                let header: state_types::ListTerms = account(&svm, terms);
                let mut offers =
                    rows::<state_types::ListTerms, state_types::ListOffer>(&svm, terms);
                assert_eq!(offers.len(), COUNT);
                assert_eq!(
                    rows::<state_types::ListRouting, state_types::PolicyMirror>(
                        &svm,
                        reserve.accounts[9].pubkey
                    )
                    .len(),
                    COUNT
                );
                if scenario.starts_with("strict_descending") {
                    for (index, row) in offers.iter_mut().enumerate() {
                        let rate = ((COUNT - index) as u32) * 1_000_000;
                        row.terms.input_rate = rate.into();
                        row.terms.cached_rate = rate.into();
                        row.terms.output_rate = rate.into();
                    }
                    store_slab(&mut svm, terms, &header, &offers, COUNT as u32);
                }
                if scenario == "wallet_concurrency_full" {
                    for row in &mut offers {
                        row.terms.capacity = 100.into();
                    }
                    store_slab(&mut svm, terms, &header, &offers, COUNT as u32);
                }
                if changed {
                    for (page, key) in keys.iter().enumerate() {
                        let header: state_types::ProviderLimitPage = account(&svm, *key);
                        let mut entries = rows::<
                            state_types::ProviderLimitPage,
                            state_types::ProviderLimitEntry,
                        >(&svm, *key);
                        for row in &mut entries {
                            row.quota_limit = match scenario {
                                "ordinary_provider_quota_disabled" => 0,
                                "wallet_quota_full" => 100,
                                _ => 10,
                            }
                            .into();
                            row.quota_used = (if scenario == "provider_quota_full"
                                || scenario == "wallet_quota_full"
                            {
                                10
                            } else {
                                1
                            })
                            .into();
                            row.quota_window_end = 160.into();
                            if scenario == "wallet_concurrency_full" {
                                row.concurrency_limit = 100.into();
                            }
                            if scenario.starts_with("provider_concurrency_")
                                || scenario == "wallet_concurrency_full"
                            {
                                row.active_bookings = 10.into();
                            }
                        }
                        if scenario == "provider_concurrency_last" && page == (COUNT - 1) % pages {
                            entries.last_mut().unwrap().active_bookings = 0.into();
                        }
                        store_slab(&mut svm, *key, &header, &entries, entries.len() as u32);
                    }
                } else if scenario.starts_with("provider_concurrency_")
                    || scenario == "wallet_concurrency_full"
                {
                    let key = reserve.accounts[3].pubkey;
                    let header: state_types::ListCapacity = account(&svm, key);
                    let mut entries = rows::<
                        state_types::ListCapacity,
                        state_types::ListCapacityEntry,
                    >(&svm, key);
                    for row in &mut entries {
                        row.active = 10.into();
                    }
                    if scenario == "provider_concurrency_last" {
                        entries.last_mut().unwrap().active = 0.into();
                    }
                    store_slab(&mut svm, key, &header, &entries, COUNT as u32);
                }
                {
                    let key = reserve.accounts[10].pubkey;
                    let header: state_types::ProviderWallet = account(&svm, key);
                    let mut entries =
                        rows::<state_types::ProviderWallet, state_types::ProviderUsage>(&svm, key);
                    for row in &mut entries {
                        row.quota_window_end = 160.into();
                        if scenario == "wallet_concurrency_full" {
                            row.active_bookings = 10.into();
                        } else if scenario == "wallet_quota_full" {
                            row.quota_used = 10.into();
                        }
                    }
                    store_slab(&mut svm, key, &header, &entries, COUNT as u32);
                }
                let expected = if scenario == "provider_concurrency_last" {
                    offers.last().unwrap()
                } else {
                    offers
                        .iter()
                        .min_by_key(|row| (row.terms.input_rate.get(), row.order.get()))
                        .unwrap()
                };
                if changed && scenario.ends_with("winner_only") {
                    let page = (expected.terms.provider_policy.get() & 31) as usize;
                    include(&mut reserve, &keys, page, 1 << page, 1 << page);
                }
                let error = if [
                    "provider_concurrency_full",
                    "wallet_concurrency_full",
                    "wallet_quota_full",
                ]
                .contains(&scenario)
                    || changed && scenario == "provider_quota_full"
                {
                    Some(6005)
                } else {
                    None
                };
                let name = format!("hundred_{scenario}_p{pages}");
                if let Some(result) = sample(&mut svm, &payer, &reserve, changed, &name, error) {
                    let post = &result
                        .post_accounts
                        .iter()
                        .find(|(key, _)| *key == booking_key)
                        .unwrap()
                        .1;
                    let booking = state_types::Booking::try_deserialize(&mut post.data()).unwrap();
                    assert_eq!(booking.provider, expected.terms.provider, "{name}");
                    if changed && baseline_winner.is_some() {
                        assert_eq!(Some(booking.provider), baseline_winner, "{name}");
                    } else {
                        baseline_winner = Some(booking.provider);
                    }
                    eprintln!(
                        "LIMITS_WINNER mode={} scenario={name} provider={} expected={} matched=true",
                        if changed { "changed" } else { "baseline" },
                        booking.provider,
                        expected.terms.provider
                    );
                }
            }
        }
    }
}
