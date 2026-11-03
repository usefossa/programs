use super::*;
use solana_account::ReadableAccount;
use solana_address_lookup_table_interface::state::{AddressLookupTable, LookupTableMeta};
use solana_message::{v0, AddressLookupTableAccount, VersionedMessage};
use solana_transaction::versioned::VersionedTransaction;
use std::borrow::Cow;

#[path = "hundred_limits.rs"]
mod hundred_limits;

fn page_key(market: Address, page: u8) -> (Address, u8) {
    Address::find_program_address(
        &[b"provider_limit_page", market.as_ref(), &[page]],
        &inference_router_program::ID,
    )
}

fn distribute(svm: &mut LiteSVM, reserve: &mut Instruction, count: usize) -> Vec<Address> {
    let market = reserve.accounts[0].pubkey;
    let entries = rows::<state_types::ProviderLimitPage, state_types::ProviderLimitEntry>(
        svm,
        reserve.accounts[3].pubkey,
    );
    let terms = reserve.accounts[2].pubkey;
    let header: state_types::ListTerms = account(svm, terms);
    let mut offers = rows::<state_types::ListTerms, state_types::ListOffer>(svm, terms);
    let mut pages = vec![Vec::new(); count];
    for (index, offer) in offers.iter_mut().enumerate() {
        let page = index % count;
        let id = ((pages[page].len() as u32) << 5) | page as u32;
        pages[page].push(entries[index]);
        offer.terms.provider_policy = id.into();
        let key = identity(market, offer.terms.provider).0;
        let mut state: state_types::ProviderLimitIndex = account(svm, key);
        state.index = id.into();
        store_account(svm, key, &state);
    }
    store_slab(svm, terms, &header, &offers, offers.len() as u32);
    let mut keys = Vec::new();
    for (page, entries) in pages.into_iter().enumerate() {
        let (key, bump) = page_key(market, page as u8);
        let mut header = state_types::ProviderLimitPage::zeroed();
        header.market = market;
        header.bump = bump;
        header.page = page as u8;
        store_slab(svm, key, &header, &entries, entries.len().max(1) as u32);
        keys.push(key);
    }
    include(reserve, &keys, 0, u32::MAX, u32::MAX);
    keys
}

fn include(ix: &mut Instruction, pages: &[Address], primary: usize, supplied: u32, writable: u32) {
    ix.accounts.truncate(12);
    ix.accounts[3] = AccountMeta::new(pages[primary], false);
    for (page, key) in pages.iter().enumerate() {
        if page != primary && supplied & (1 << page) != 0 {
            ix.accounts.push(if writable & (1 << page) != 0 {
                AccountMeta::new(*key, false)
            } else {
                AccountMeta::new_readonly(*key, false)
            });
        }
    }
}

fn versioned(svm: &mut LiteSVM, payer: &Keypair, ix: &Instruction) -> VersionedTransaction {
    versioned_many(svm, payer, std::slice::from_ref(ix))
}

fn versioned_many(
    svm: &mut LiteSVM,
    payer: &Keypair,
    instructions: &[Instruction],
) -> VersionedTransaction {
    let addresses: Vec<_> = instructions
        .iter()
        .flat_map(|ix| &ix.accounts)
        .filter(|meta| !meta.is_signer && meta.pubkey != inference_router_program::ID)
        .map(|meta| meta.pubkey)
        .collect();
    let key = Address::new_from_array([241; 32]);
    let data = AddressLookupTable {
        meta: LookupTableMeta::default(),
        addresses: Cow::Borrowed(&addresses),
    }
    .serialize_for_tests()
    .unwrap();
    svm.set_account(
        key,
        solana_account::Account {
            lamports: svm.minimum_balance_for_rent_exemption(data.len()),
            data,
            owner: solana_sdk_ids::address_lookup_table::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
    let clock = svm.get_sysvar::<Clock>();
    if clock.slot == 0 {
        svm.warp_to_slot(1);
    }
    let message = v0::Message::try_compile(
        &payer.pubkey(),
        instructions,
        &[AddressLookupTableAccount { key, addresses }],
        svm.latest_blockhash(),
    )
    .unwrap();
    VersionedTransaction::try_new(VersionedMessage::V0(message), &[payer]).unwrap()
}

fn sample(
    svm: &mut LiteSVM,
    payer: &Keypair,
    ix: &Instruction,
    changed: bool,
    scenario: &str,
    error: Option<u32>,
) -> Option<litesvm::types::SimulatedTransactionInfo> {
    let legacy = tx(svm, payer, ix);
    let legacy_bytes = anchor_lang::wincode::serialize(&legacy).unwrap().len();
    let transaction = if legacy_bytes > 1232 {
        versioned(svm, payer, ix)
    } else {
        legacy.into()
    };
    let packet = anchor_lang::wincode::serialize(&transaction).unwrap().len();
    assert!(packet <= 1232);
    let account_count = ix
        .accounts
        .iter()
        .map(|m| m.pubkey)
        .chain([payer.pubkey(), inference_router_program::ID])
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    assert!(account_count <= 64);
    let mut reference = None;
    let mut result = None;
    let mut loaded = 0;
    for _ in 0..3 {
        let cu = match svm.simulate_transaction(transaction.clone()) {
            Ok(value) => {
                assert!(error.is_none(), "{scenario}: expected {error:?}");
                loaded = value.meta.loaded_accounts_data_size;
                let cu = value.meta.compute_units_consumed;
                result = Some(value);
                cu
            }
            Err(value) => {
                let expected = error.unwrap_or_else(|| panic!("{scenario}: {value:?}"));
                assert!(
                    format!("{:?}", value.err).contains(&format!("Custom({expected})")),
                    "{scenario}: {value:?}"
                );
                loaded = value.meta.loaded_accounts_data_size;
                value.meta.compute_units_consumed
            }
        };
        assert!(cu <= 1_400_000);
        assert!(reference.is_none_or(|prior| prior == cu));
        reference = Some(cu);
    }
    let rent: u64 = ix
        .accounts
        .iter()
        .map(|m| m.pubkey)
        .collect::<std::collections::BTreeSet<_>>()
        .iter()
        .filter_map(|key| svm.get_account(key))
        .filter(|account| account.owner == inference_router_program::ID)
        .map(|account| account.lamports)
        .sum();
    let writes = ix
        .accounts
        .iter()
        .filter(|meta| meta.is_writable)
        .map(|meta| meta.pubkey)
        .collect::<std::collections::BTreeSet<_>>()
        .len();
    eprintln!("LIMITS_CU mode={} scenario={scenario} cu={} success={} loaded={loaded} packet={packet} accounts={account_count} rent={rent} samples=3 legacy_packet={legacy_bytes} writable={writes} error={error:?}", if changed {"changed"} else {"baseline"}, reference.unwrap(), error.is_none());
    result
}

#[test]
fn paged_limits_spread_cu() {
    for count in [2, 32, 4096] {
        for pages in [1, 2, 4, 16, 32]
            .into_iter()
            .filter(|pages| *pages <= count)
        {
            for scenario in [
                "all",
                "winner_only",
                "active_full",
                "active_last",
                "quota_full",
                "rate_full",
            ] {
                for changed in modes() {
                    let (mut svm, payer) = vm(changed);
                    let (mut reserve, _) =
                        prepare(&mut svm, &payer, changed, count, false, true, true, 0, 0);
                    let mut keys = Vec::new();
                    if changed {
                        keys = distribute(&mut svm, &mut reserve, pages);
                    }
                    if scenario.starts_with("active_") {
                        if changed {
                            for (page, key) in keys.iter().enumerate() {
                                let header: state_types::ProviderLimitPage = account(&svm, *key);
                                let mut entries = rows::<
                                    state_types::ProviderLimitPage,
                                    state_types::ProviderLimitEntry,
                                >(&svm, *key);
                                for row in &mut entries {
                                    row.active_bookings = 10.into();
                                }
                                if scenario == "active_last" && page == (count - 1) % pages {
                                    entries.last_mut().unwrap().active_bookings = 0.into();
                                }
                                store_slab(&mut svm, *key, &header, &entries, entries.len() as u32);
                            }
                        } else {
                            let key = reserve.accounts[3].pubkey;
                            let header: state_types::ListCapacity = account(&svm, key);
                            let mut entries = rows::<
                                state_types::ListCapacity,
                                state_types::ListCapacityEntry,
                            >(&svm, key);
                            for row in &mut entries {
                                row.active = 10.into();
                            }
                            if scenario == "active_last" {
                                entries.last_mut().unwrap().active = 0.into();
                            }
                            store_slab(&mut svm, key, &header, &entries, entries.len() as u32);
                        }
                    }
                    if scenario == "quota_full" {
                        let key = reserve.accounts[10].pubkey;
                        let header: state_types::ProviderWallet = account(&svm, key);
                        let mut entries = rows::<
                            state_types::ProviderWallet,
                            state_types::ProviderUsage,
                        >(&svm, key);
                        for row in &mut entries {
                            row.quota_used = 10.into();
                        }
                        store_slab(&mut svm, key, &header, &entries, entries.len() as u32);
                    }
                    if changed && scenario == "rate_full" {
                        for key in &keys {
                            let header: state_types::ProviderLimitPage = account(&svm, *key);
                            let mut entries = rows::<
                                state_types::ProviderLimitPage,
                                state_types::ProviderLimitEntry,
                            >(&svm, *key);
                            for row in &mut entries {
                                row.quota_limit = 10.into();
                                row.quota_used = 10.into();
                                row.quota_window_end = 160.into();
                            }
                            store_slab(&mut svm, *key, &header, &entries, entries.len() as u32);
                        }
                    }
                    if changed && scenario == "winner_only" {
                        let offers = rows::<state_types::ListTerms, state_types::ListOffer>(
                            &svm,
                            reserve.accounts[2].pubkey,
                        );
                        let winner = offers
                            .iter()
                            .min_by_key(|row| (row.terms.input_rate.get(), row.order.get()))
                            .unwrap();
                        let page = (winner.terms.provider_policy.get() & 31) as usize;
                        include(&mut reserve, &keys, page, 1 << page, 1 << page);
                    }
                    let error = if scenario == "active_full"
                        || scenario == "quota_full"
                        || changed && scenario == "rate_full"
                    {
                        Some(6005)
                    } else {
                        None
                    };
                    sample(
                        &mut svm,
                        &payer,
                        &reserve,
                        changed,
                        &format!("paged_{scenario}_n{count}_p{pages}"),
                        error,
                    );
                }
            }
        }
    }
}

#[test]
fn paged_limits_missing_and_readonly_guards() {
    let (mut svm, payer) = vm(true);
    let (mut reserve, _) = prepare(&mut svm, &payer, true, 2, false, true, true, 0, 0);
    let keys = distribute(&mut svm, &mut reserve, 2);
    include(&mut reserve, &keys, 1, 2, 2);
    sample(
        &mut svm,
        &payer,
        &reserve,
        true,
        "paged_missing_cheapest",
        Some(6009),
    );
    let tracked = [
        reserve.accounts[4].pubkey,
        reserve.accounts[6].pubkey,
        reserve.accounts[10].pubkey,
        keys[0],
        keys[1],
    ];
    let before: Vec<_> = tracked
        .iter()
        .map(|key| svm.get_account(key).unwrap())
        .collect();
    assert!(svm.send_transaction(tx(&svm, &payer, &reserve)).is_err());
    for (key, before) in tracked.iter().zip(before) {
        assert_eq!(svm.get_account(key).unwrap(), before);
    }
    include(&mut reserve, &keys, 1, 3, 2);
    sample(
        &mut svm,
        &payer,
        &reserve,
        true,
        "paged_readonly_winner",
        Some(6010),
    );
    include(&mut reserve, &keys, 1, 3, 3);
    sample(
        &mut svm,
        &payer,
        &reserve,
        true,
        "paged_remaining_writable_winner",
        None,
    );
    include(&mut reserve, &keys, 0, 1, 1);
    sample(
        &mut svm,
        &payer,
        &reserve,
        true,
        "paged_expensive_missing_allowed",
        None,
    );
    let terms = reserve.accounts[2].pubkey;
    let header: state_types::ListTerms = account(&svm, terms);
    let mut offers = rows::<state_types::ListTerms, state_types::ListOffer>(&svm, terms);
    offers[1].terms.input_rate = offers[0].terms.input_rate;
    offers[1].terms.output_rate = offers[0].terms.output_rate;
    store_slab(&mut svm, terms, &header, &offers, 2);
    include(&mut reserve, &keys, 1, 2, 2);
    sample(
        &mut svm,
        &payer,
        &reserve,
        true,
        "paged_earlier_tie_missing",
        Some(6009),
    );
    include(&mut reserve, &keys, 0, 1, 1);
    sample(
        &mut svm,
        &payer,
        &reserve,
        true,
        "paged_later_tie_missing_allowed",
        None,
    );
    offers[1].terms.checkpoint = [255; 32];
    store_slab(&mut svm, terms, &header, &offers, 2);
    sample(
        &mut svm,
        &payer,
        &reserve,
        true,
        "paged_incompatible_missing_allowed",
        None,
    );
    let mut duplicate = reserve.clone();
    duplicate
        .accounts
        .push(AccountMeta::new_readonly(keys[0], false));
    assert!(svm
        .simulate_transaction(tx(&svm, &payer, &duplicate))
        .is_err());
}

#[test]
fn paged_limits_unrelated_pages_do_not_load_or_lock() {
    let (mut svm, payer) = vm(true);
    let (mut reserve, _) = prepare(&mut svm, &payer, true, 2, false, true, true, 0, 0);
    let keys = distribute(&mut svm, &mut reserve, 2);
    include(&mut reserve, &keys, 0, 1, 1);
    let first = sample(&mut svm, &payer, &reserve, true, "paged_unrelated_0", None).unwrap();
    let header: state_types::ProviderLimitPage = account(&svm, keys[1]);
    let row =
        rows::<state_types::ProviderLimitPage, state_types::ProviderLimitEntry>(&svm, keys[1])[0];
    store_slab(&mut svm, keys[1], &header, &vec![row; 100_000], 100_000);
    let second = sample(
        &mut svm,
        &payer,
        &reserve,
        true,
        "paged_unrelated_100000",
        None,
    )
    .unwrap();
    assert_eq!(
        first.meta.compute_units_consumed,
        second.meta.compute_units_consumed
    );
    assert_eq!(
        first.meta.loaded_accounts_data_size,
        second.meta.loaded_accounts_data_size
    );
    assert!(!reserve.accounts.iter().any(|meta| meta.pubkey == keys[1]));
}

#[test]
fn paged_limits_cross_scope_release_and_provider_isolation() {
    let (mut svm, payer) = vm(true);
    let (mut reserve, _) = prepare(&mut svm, &payer, true, 2, false, true, false, 0, 0);
    let keys = distribute(&mut svm, &mut reserve, 2);
    let offers =
        rows::<state_types::ListTerms, state_types::ListOffer>(&svm, reserve.accounts[2].pubkey);
    for (page, offer) in offers.iter().enumerate() {
        let mut context = reserve.clone();
        context.accounts[3].pubkey = keys[page];
        let config = configure(
            &mut svm,
            &context,
            offer.terms.provider,
            limits(1, 2, 60),
            limits(1, 2, 60),
        );
        apply(&mut svm, &payer, &config);
    }
    include(&mut reserve, &keys, 0, 1, 1);
    apply(&mut svm, &payer, &reserve);
    let mut other = another_scope(&mut svm, &reserve, 81);
    rejected(&svm, &payer, &other, 6009);
    include(&mut other, &keys, 1, 3, 3);
    apply(&mut svm, &payer, &other);
    assert_eq!(
        account::<state_types::Booking>(&svm, other.accounts[6].pubkey).provider,
        offers[1].terms.provider
    );
    let mut wrong = release(&other, offers[1].terms.provider, None);
    wrong.accounts[2].pubkey = keys[0];
    rejected(&svm, &payer, &wrong, 6001);
    apply(
        &mut svm,
        &payer,
        &release(&other, offers[1].terms.provider, None),
    );
    assert_eq!(
        rows::<state_types::ProviderLimitPage, state_types::ProviderLimitEntry>(&svm, keys[0])[0]
            .active_bookings
            .get(),
        1
    );
    assert_eq!(
        rows::<state_types::ProviderLimitPage, state_types::ProviderLimitEntry>(&svm, keys[1])[0]
            .active_bookings
            .get(),
        0
    );
    apply(
        &mut svm,
        &payer,
        &release(&reserve, offers[0].terms.provider, None),
    );
    let mut retry = fresh_booking(&mut svm, &reserve, 102);
    include(&mut retry, &keys, 0, 1, 1);
    apply(&mut svm, &payer, &retry);
    let disjoint: Vec<_> = [reserve, other]
        .iter()
        .map(|ix| {
            ix.accounts
                .iter()
                .filter(|meta| meta.is_writable && keys.contains(&meta.pubkey))
                .map(|meta| meta.pubkey)
                .collect::<std::collections::BTreeSet<_>>()
        })
        .collect();
    assert!(disjoint[0].contains(&keys[0]));
    assert!(disjoint[1].contains(&keys[1]));
}

#[test]
fn paged_limits_sparse_model_and_registration_order_cu() {
    for count in [2, 32] {
        for changed in modes() {
            let (mut svm, payer) = vm(changed);
            let (mut reserve, _) =
                prepare(&mut svm, &payer, changed, 4096, false, true, true, 0, 0);
            if changed {
                distribute(&mut svm, &mut reserve, 32);
            }
            let terms = reserve.accounts[2].pubkey;
            let header: state_types::ListTerms = account(&svm, terms);
            let all = rows::<state_types::ListTerms, state_types::ListOffer>(&svm, terms);
            let positions: Vec<_> = (0..count).map(|index| (index * 173 + 91) % 4096).collect();
            let mut selected: Vec<_> = positions.iter().map(|index| all[*index]).collect();
            selected.sort_by_key(|row| row.terms.provider);
            store_slab(&mut svm, terms, &header, &selected, count as u32);
            if !changed {
                let key = reserve.accounts[3].pubkey;
                let header: state_types::ListCapacity = account(&svm, key);
                let all =
                    rows::<state_types::ListCapacity, state_types::ListCapacityEntry>(&svm, key);
                let mut selected: Vec<_> = positions.iter().map(|index| all[*index]).collect();
                selected.sort_by_key(|row| row.provider);
                store_slab(&mut svm, key, &header, &selected, count as u32);
            }
            sample(
                &mut svm,
                &payer,
                &reserve,
                changed,
                &format!("paged_model_subset_n{count}_members4096"),
                None,
            );
            for minimal in [false, true] {
                if changed {
                    let keys: Vec<_> = (0..32)
                        .map(|page| page_key(reserve.accounts[0].pubkey, page).0)
                        .collect();
                    let winner = selected
                        .iter()
                        .min_by_key(|row| (row.terms.input_rate.get(), row.order.get()))
                        .unwrap();
                    let primary = (winner.terms.provider_policy.get() & 31) as usize;
                    let mask = if minimal {
                        1 << primary
                    } else {
                        selected.iter().fold(0, |mask, row| {
                            mask | (1 << (row.terms.provider_policy.get() & 31))
                        })
                    };
                    include(&mut reserve, &keys, primary, mask, 1 << primary);
                }
                sample(
                    &mut svm,
                    &payer,
                    &reserve,
                    changed,
                    &format!(
                        "paged_model_subset_n{count}_members4096_{}",
                        if minimal {
                            "winner_only"
                        } else {
                            "relevant_pages"
                        }
                    ),
                    None,
                );
            }
        }
    }
    let (mut svm, payer) = vm(true);
    let (reserve, _) = prepare(&mut svm, &payer, true, 4096, false, true, true, 0, 0);
    let key = reserve.accounts[3].pubkey;
    let header: state_types::ProviderLimitPage = account(&svm, key);
    let mut entries =
        rows::<state_types::ProviderLimitPage, state_types::ProviderLimitEntry>(&svm, key);
    entries.reverse();
    store_slab(&mut svm, key, &header, &entries, 4096);
    let terms = reserve.accounts[2].pubkey;
    let header: state_types::ListTerms = account(&svm, terms);
    let mut offers = rows::<state_types::ListTerms, state_types::ListOffer>(&svm, terms);
    for (index, row) in offers.iter_mut().enumerate() {
        row.terms.provider_policy = (((4095 - index) as u32) << 5).into();
    }
    store_slab(&mut svm, terms, &header, &offers, 4096);
    sample(
        &mut svm,
        &payer,
        &reserve,
        true,
        "paged_reverse_registration_n4096",
        None,
    );
}

#[test]
fn paged_limits_wallet_cold_setup_and_identity_guards() {
    for changed in modes() {
        let (mut svm, payer) = vm(changed);
        let (reserve, _) = prepare(&mut svm, &payer, changed, 1, false, true, false, 0, 0);
        let wallet = reserve.accounts[10].pubkey;
        svm.set_account(wallet, solana_account::Account::default())
            .unwrap();
        let init = instruction(
            inference_router_program::ID,
            accounts::InitializeProviderWallet {
                market: reserve.accounts[0].pubkey,
                owner: payer.pubkey(),
                payer: payer.pubkey(),
                wallet,
                system_program: anchor_lang::system_program::ID,
            },
            ix::InitializeProviderWallet { limit: 8 },
        );
        let result = sample(
            &mut svm,
            &payer,
            &init,
            changed,
            "wallet_initialize_8",
            None,
        )
        .unwrap();
        let created = result
            .post_accounts
            .iter()
            .find(|(key, _)| *key == wallet)
            .unwrap();
        eprintln!(
            "LIMITS_CREATED mode={} scenario=wallet_initialize_8 bytes={} rent={}",
            if changed { "changed" } else { "baseline" },
            created.1.data().len(),
            created.1.lamports()
        );
        let message = Message::new(&[init.clone(), reserve.clone()], Some(&payer.pubkey()));
        let mut transaction = Transaction::new_unsigned(message);
        transaction.message.recent_blockhash = svm.latest_blockhash();
        let mut cu = 0;
        let mut loaded = 0;
        for _ in 0..3 {
            let result = svm.simulate_transaction(transaction.clone()).unwrap();
            assert!(cu == 0 || cu == result.meta.compute_units_consumed);
            cu = result.meta.compute_units_consumed;
            loaded = result.meta.loaded_accounts_data_size;
        }
        eprintln!("LIMITS_CU mode={} scenario=wallet_initialize_and_first_reserve cu={cu} success=true loaded={loaded} packet={} accounts={} rent=0 samples=3",if changed {"changed"} else {"baseline"},anchor_lang::wincode::serialize(&transaction).unwrap().len(),transaction.message.account_keys.len());
        apply(&mut svm, &payer, &init);
        sample(
            &mut svm,
            &payer,
            &reserve,
            changed,
            "wallet_first_provider_reserve",
            None,
        );
        apply(&mut svm, &payer, &reserve);
        let second = fresh_booking(&mut svm, &reserve, 87);
        sample(
            &mut svm,
            &payer,
            &second,
            changed,
            "wallet_existing_provider_reserve",
            None,
        );
    }
    let (mut svm, payer) = vm(true);
    let (mut reserve, _) = prepare(&mut svm, &payer, true, 2, false, true, false, 0, 0);
    let keys = distribute(&mut svm, &mut reserve, 2);
    let provider =
        rows::<state_types::ListTerms, state_types::ListOffer>(&svm, reserve.accounts[2].pubkey)[0]
            .terms
            .provider;
    let config = configure(
        &mut svm,
        &reserve,
        provider,
        limits(0, 0, 60),
        limits(0, 0, 60),
    );
    apply(&mut svm, &payer, &config);
    assert_eq!(
        rows::<state_types::ProviderLimitPage, state_types::ProviderLimitEntry>(&svm, keys[0])[0]
            .concurrency_limit
            .get(),
        u32::MAX
    );
    let profile = Address::find_program_address(
        &[
            b"provider",
            reserve.accounts[0].pubkey.as_ref(),
            provider.as_ref(),
        ],
        &inference_router_program::ID,
    )
    .0;
    let duplicate = instruction(
        inference_router_program::ID,
        accounts::InitializeProviderLimits {
            provider,
            payer: payer.pubkey(),
            profile,
            page: keys[1],
            identity: identity(reserve.accounts[0].pubkey, provider).0,
            system_program: anchor_lang::system_program::ID,
        },
        ix::InitializeProviderLimits {
            limits: args::ProviderLimits {
                global: limits(1, 1, 60),
                wallet: limits(1, 1, 60),
            },
        },
    );
    assert!(svm
        .simulate_transaction(tx(&svm, &payer, &duplicate))
        .is_err());
    let mut wrong = config.clone();
    wrong.accounts[3].pubkey = keys[1];
    rejected(&svm, &payer, &wrong, 6003);
    let second = other_wallet(&mut svm, &reserve, 91);
    let mut list = another_scope(&mut svm, &second, 101);
    let terms = list.accounts[2].pubkey;
    let header: state_types::ListTerms = account(&svm, terms);
    let offers = rows::<state_types::ListTerms, state_types::ListOffer>(&svm, terms);
    store_slab(&mut svm, terms, &header, &offers[1..], 1);
    include(&mut reserve, &keys, 0, 1, 1);
    include(&mut list, &keys, 1, 2, 2);
    let first_pages: std::collections::BTreeSet<_> = reserve
        .accounts
        .iter()
        .filter(|meta| keys.contains(&meta.pubkey))
        .map(|meta| meta.pubkey)
        .collect();
    let second_pages: std::collections::BTreeSet<_> = list
        .accounts
        .iter()
        .filter(|meta| keys.contains(&meta.pubkey))
        .map(|meta| meta.pubkey)
        .collect();
    assert!(first_pages.is_disjoint(&second_pages));
    apply(&mut svm, &payer, &reserve);
    apply(&mut svm, &payer, &list);
}

#[test]
fn paged_limits_sorted_adversarial_prices_cu() {
    for count in [32, 4096] {
        for changed in modes() {
            let (mut svm, payer) = vm(changed);
            let (mut reserve, _) =
                prepare(&mut svm, &payer, changed, count, false, true, true, 0, 0);
            let keys = if changed {
                distribute(&mut svm, &mut reserve, 32)
            } else {
                Vec::new()
            };
            let terms = reserve.accounts[2].pubkey;
            let header: state_types::ListTerms = account(&svm, terms);
            let mut offers = rows::<state_types::ListTerms, state_types::ListOffer>(&svm, terms);
            for (index, row) in offers.iter_mut().enumerate() {
                let rate = ((count - index) as u32) * 1_000_000;
                row.terms.input_rate = rate.into();
                row.terms.cached_rate = rate.into();
                row.terms.output_rate = rate.into();
            }
            store_slab(&mut svm, terms, &header, &offers, count as u32);
            sample(
                &mut svm,
                &payer,
                &reserve,
                changed,
                &format!("paged_adversarial_all_n{count}"),
                None,
            );
            if changed {
                let page = (offers.last().unwrap().terms.provider_policy.get() & 31) as usize;
                include(&mut reserve, &keys, page, 1 << page, 1 << page);
            }
            sample(
                &mut svm,
                &payer,
                &reserve,
                changed,
                &format!("paged_adversarial_winner_only_n{count}"),
                None,
            );
        }
    }
}

#[test]
fn paged_limits_explicit_compute_budget_packets() {
    for changed in modes() {
        for pages in [1, 16, 32] {
            let (mut svm, payer) = vm(changed);
            let (mut reserve, _) =
                prepare(&mut svm, &payer, changed, 4096, false, true, true, 0, 0);
            if changed {
                distribute(&mut svm, &mut reserve, pages);
            }
            let instructions = [
                solana_compute_budget_interface::ComputeBudgetInstruction::set_compute_unit_limit(
                    1_400_000,
                ),
                reserve,
            ];
            let message = Message::new(&instructions, Some(&payer.pubkey()));
            let mut legacy = Transaction::new_unsigned(message);
            legacy.message.recent_blockhash = svm.latest_blockhash();
            let legacy_bytes = anchor_lang::wincode::serialize(&legacy).unwrap().len();
            let transaction: VersionedTransaction = if legacy_bytes <= 1232 {
                legacy.into()
            } else {
                versioned_many(&mut svm, &payer, &instructions)
            };
            let packet = anchor_lang::wincode::serialize(&transaction).unwrap().len();
            assert!(packet <= 1232);
            let mut cu = 0;
            for _ in 0..3 {
                let result = svm.simulate_transaction(transaction.clone()).unwrap();
                assert!(cu == 0 || cu == result.meta.compute_units_consumed);
                cu = result.meta.compute_units_consumed;
                assert!(cu <= 1_400_000);
            }
            eprintln!("LIMITS_PACKET mode={} pages={pages} cu={cu} packet={packet} legacy_packet={legacy_bytes} explicit_limit=1400000 samples=3", if changed { "changed" } else { "baseline" });
        }
    }
}
