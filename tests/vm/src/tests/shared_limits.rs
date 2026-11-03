use super::*;
use solana_clock::Clock;
use solana_instruction::{AccountMeta, Instruction};

fn rows<H: SlabSchema + Discriminator + Pod + Zeroable, T: Pod>(
    svm: &LiteSVM,
    key: Address,
) -> Vec<T> {
    let a = svm.get_account(&key).unwrap();
    let start = Slab::<H, T>::space_for(0);
    let length_at = H::DISCRIMINATOR.len() + std::mem::size_of::<H>();
    let count = u32::from_le_bytes(a.data[length_at..length_at + 4].try_into().unwrap()) as usize;
    anchor_lang::bytemuck::cast_slice(&a.data[start..start + count * std::mem::size_of::<T>()])
        .to_vec()
}

fn shared(svm: &mut LiteSVM, reserve: &mut Instruction, enabled: bool, unrelated: usize) {
    let market = reserve.accounts[0].pubkey;
    let mut offers =
        rows::<state_types::ListTerms, state_types::ListOffer>(svm, reserve.accounts[2].pubkey);
    let mut terms_header: state_types::ListTerms = account(svm, reserve.accounts[2].pubkey);
    terms_header.limits_version = 1;
    let mut market_state: state_types::Market = account(svm, market);
    market_state.reserved[0] = 2;
    store_account(svm, market, &market_state);
    for (index, offer) in offers.iter_mut().enumerate() {
        offer.terms.provider_policy = ((index as u32) << 5).into();
        offer.terms.version = state_types::LIST_OFFER_VERSION;
        let (identity, bump) = identity(market, offer.terms.provider);
        let mut state = state_types::ProviderLimitIndex::zeroed();
        state.market = market;
        state.provider = offer.terms.provider;
        state.index = ((index as u32) << 5).into();
        state.bump = bump;
        store_account(svm, identity, &state);
    }
    store_slab(
        svm,
        reserve.accounts[2].pubkey,
        &terms_header,
        &offers,
        offers.len() as u32,
    );
    let (key, bump) = Address::find_program_address(
        &[b"provider_limit_page", market.as_ref(), &[0]],
        &inference_router_program::ID,
    );
    let mut header = state_types::ProviderLimitPage::zeroed();
    header.market = market;
    header.bump = bump;
    let mut entries: Vec<_> = offers
        .iter()
        .map(|_| state_types::ProviderLimitEntry {
            concurrency_limit: 10.into(),
            quota_window_seconds: 60.into(),
            wallet_concurrency_limit: (if enabled { 10 } else { 0 }).into(),
            wallet_quota_limit: (if enabled { 10 } else { 0 }).into(),
            wallet_quota_window_seconds: 60.into(),
            ..state_types::ProviderLimitEntry::zeroed()
        })
        .collect();
    for _ in 0..unrelated {
        entries.push(state_types::ProviderLimitEntry {
            quota_window_seconds: 60.into(),
            wallet_quota_window_seconds: 60.into(),
            ..state_types::ProviderLimitEntry::zeroed()
        });
    }
    store_slab(svm, key, &header, &entries, entries.len().max(1) as u32);
    reserve.accounts[3].pubkey = key;
}

fn modes() -> impl Iterator<Item = bool> {
    [false, true]
        .into_iter()
        .filter(|changed| *changed || std::env::var_os("FOSSA_CU_BASELINE").is_some())
}

fn vm(changed: bool) -> (LiteSVM, Keypair) {
    let mut budget = solana_compute_budget::compute_budget::ComputeBudget::new_with_defaults(true);
    budget.compute_unit_limit = 1_400_000;
    let mut svm = LiteSVM::new()
        .with_sigverify(false)
        .with_compute_budget(budget);
    let path = if changed {
        program_path("inference_router_program.so")
    } else {
        std::env::var_os("FOSSA_CU_BASELINE")
            .expect("FOSSA_CU_BASELINE must name an archived SBF")
            .into()
    };
    svm.add_program_from_file(inference_router_program::ID, path)
        .unwrap();
    let payer = Keypair::new_from_array([71; 32]);
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    let mut clock = svm.get_sysvar::<Clock>();
    clock.unix_timestamp = 100;
    svm.set_sysvar(&clock);
    (svm, payer)
}

fn prepare(
    svm: &mut LiteSVM,
    payer: &Keypair,
    changed: bool,
    count: usize,
    descending: bool,
    enabled: bool,
    warm: bool,
    sparse: usize,
    unrelated: usize,
) -> (Instruction, Address) {
    let (old, booking) = fixture(svm, payer, count, descending, Case::Compatible, false);
    let mut reserve = scoped_bench::convert(svm, payer, &old, enabled, warm, sparse);
    if changed {
        shared(svm, &mut reserve, enabled, unrelated);
    }
    (reserve, booking)
}

fn tx(svm: &LiteSVM, payer: &Keypair, ix: &Instruction) -> Transaction {
    let mut transaction = Transaction::new_unsigned(Message::new(
        std::slice::from_ref(ix),
        Some(&payer.pubkey()),
    ));
    transaction.message.recent_blockhash = svm.latest_blockhash();
    transaction
}

fn apply(svm: &mut LiteSVM, payer: &Keypair, ix: &Instruction) {
    svm.expire_blockhash();
    svm.send_transaction(tx(svm, payer, ix))
        .unwrap_or_else(|e| panic!("apply: {e:?}"));
}

fn measure(
    svm: &LiteSVM,
    payer: &Keypair,
    ix: &Instruction,
    changed: bool,
    scenario: &str,
    success: bool,
) -> Option<litesvm::types::SimulatedTransactionInfo> {
    let transaction = tx(svm, payer, ix);
    let mut reference = None;
    let mut result = None;
    let mut loaded = 0;
    for _ in 0..3 {
        let (cu, actual) = match svm.simulate_transaction(transaction.clone()) {
            Ok(value) => {
                loaded = value.meta.loaded_accounts_data_size;
                let cu = value.meta.compute_units_consumed;
                result = Some(value);
                (cu, true)
            }
            Err(error) => {
                let expected = if scenario.contains("exhausted") || scenario.contains("rate_reject")
                {
                    6005
                } else {
                    6006
                };
                assert!(
                    format!("{:?}", error.err).contains(&format!("Custom({expected})")),
                    "{scenario}: {error:?}"
                );
                loaded = error.meta.loaded_accounts_data_size;
                (error.meta.compute_units_consumed, false)
            }
        };
        assert_eq!(
            actual,
            success,
            "{scenario}, changed={changed}, result={:?}",
            svm.simulate_transaction(transaction.clone())
        );
        assert!(reference.is_none_or(|value| value == cu));
        reference = Some(cu);
    }
    let rent: u64 = transaction
        .message
        .account_keys
        .iter()
        .filter_map(|key| svm.get_account(key))
        .filter(|a| a.owner == inference_router_program::ID)
        .map(|a| a.lamports)
        .sum();
    eprintln!("LIMITS_CU mode={} scenario={scenario} cu={} success={success} loaded={loaded} packet={} accounts={} rent={rent} samples=3", if changed {"changed"} else {"baseline"}, reference.unwrap(), anchor_lang::wincode::serialize(&transaction).unwrap().len(), transaction.message.account_keys.len());
    result
}

fn release(reserve: &Instruction, provider: Address, generation: Option<u64>) -> Instruction {
    instruction(
        inference_router_program::ID,
        accounts::ReleaseListBooking {
            base: accounts::ReleaseBooking {
                market: reserve.accounts[0].pubkey,
                model: reserve.accounts[1].pubkey,
                capacity: reserve.accounts[3].pubkey,
                user: reserve.accounts[4].pubkey,
                booking: reserve.accounts[6].pubkey,
                provider,
            },
            wallet_bookings: reserve.accounts[10].pubkey,
        },
        ix::ReleaseListBooking { generation },
    )
}

fn set_request(reserve: &mut Instruction, generation: Option<u64>, minimum: Option<u32>) {
    let request: args::ReserveRequest =
        anchor_lang::wincode::deserialize(&reserve.data[8..]).unwrap();
    reserve.data = ix::ReserveListBooking {
        request,
        generation,
        minimum_wallet_concurrency: minimum,
    }
    .data();
}

#[test]
fn shared_limits_routing_cu() {
    for changed in modes() {
        for count in [2, 32, 4096] {
            for descending in [false, true] {
                for scenario in [
                    "disabled_cold",
                    "enabled_cold",
                    "enabled_warm",
                    "quota_exhausted",
                    "quota_last",
                    "concurrency_exhausted",
                    "concurrency_last",
                    "sparse",
                    "window_rollover",
                ] {
                    let (mut svm, payer) = vm(changed);
                    let enabled = scenario != "disabled_cold";
                    let warm = !scenario.ends_with("cold");
                    let (reserve, _) = prepare(
                        &mut svm,
                        &payer,
                        changed,
                        count,
                        descending,
                        enabled,
                        warm,
                        if scenario == "sparse" { count } else { 0 },
                        0,
                    );
                    if scenario.starts_with("quota_") || scenario == "window_rollover" {
                        let key = reserve.accounts[10].pubkey;
                        let header: state_types::ProviderWallet = account(&svm, key);
                        let mut entries = rows::<
                            state_types::ProviderWallet,
                            state_types::ProviderUsage,
                        >(&svm, key);
                        for row in &mut entries {
                            row.quota_used = 10.into();
                            if scenario == "window_rollover" {
                                row.quota_window_end = 100.into();
                            }
                        }
                        if scenario == "quota_last" {
                            entries.last_mut().unwrap().quota_used = 0.into();
                        }
                        store_slab(
                            &mut svm,
                            key,
                            &header,
                            &entries,
                            entries.len().max(8) as u32,
                        );
                    }
                    if scenario.starts_with("concurrency_") {
                        let key = reserve.accounts[3].pubkey;
                        if changed {
                            let header: state_types::ProviderLimitPage = account(&svm, key);
                            let mut entries = rows::<
                                state_types::ProviderLimitPage,
                                state_types::ProviderLimitEntry,
                            >(&svm, key);
                            for row in &mut entries {
                                row.active_bookings = 10.into();
                            }
                            if scenario == "concurrency_last" {
                                entries.last_mut().unwrap().active_bookings = 0.into();
                            }
                            store_slab(&mut svm, key, &header, &entries, count as u32);
                        } else {
                            let header: state_types::ListCapacity = account(&svm, key);
                            let mut entries = rows::<
                                state_types::ListCapacity,
                                state_types::ListCapacityEntry,
                            >(&svm, key);
                            for row in &mut entries {
                                row.active = 10.into();
                            }
                            if scenario == "concurrency_last" {
                                entries.last_mut().unwrap().active = 0.into();
                            }
                            store_slab(&mut svm, key, &header, &entries, count as u32);
                        }
                    }
                    let result = measure(
                        &svm,
                        &payer,
                        &reserve,
                        changed,
                        &format!(
                            "reserve_{scenario}_n{count}_{}",
                            if descending { "last" } else { "first" }
                        ),
                        !scenario.ends_with("exhausted"),
                    );
                    if let Some(result) = result {
                        let booking: state_types::Booking =
                            lifecycle::post(&result, reserve.accounts[6].pubkey);
                        if scenario.ends_with("last") {
                            let offers = rows::<state_types::ListTerms, state_types::ListOffer>(
                                &svm,
                                reserve.accounts[2].pubkey,
                            );
                            assert_eq!(booking.provider, offers.last().unwrap().terms.provider);
                        }
                    }
                }
            }
        }
        for unrelated in [0, 4094, 100_000] {
            let (mut svm, payer) = vm(changed);
            let (reserve, _) = prepare(
                &mut svm, &payer, changed, 2, false, true, true, 0, unrelated,
            );
            measure(
                &svm,
                &payer,
                &reserve,
                changed,
                &format!("unrelated_global_{unrelated}"),
                true,
            );
        }
        for history in [0, 100_000] {
            let (mut svm, payer) = vm(changed);
            let (reserve, _) = prepare(&mut svm, &payer, changed, 2, false, true, true, 0, 0);
            let key = reserve.accounts[10].pubkey;
            let header: state_types::ProviderWallet = account(&svm, key);
            let mut entries =
                rows::<state_types::ProviderWallet, state_types::ProviderUsage>(&svm, key);
            for index in 0..history {
                let mut bytes = [0; 32];
                bytes[28..].copy_from_slice(&(index as u32).to_be_bytes());
                entries.push(state_types::ProviderUsage {
                    provider: Address::new_from_array(bytes),
                    quota_window_end: i64::MAX.into(),
                    quota_used: 1.into(),
                    active_bookings: 0.into(),
                });
            }
            entries.sort_by_key(|row| row.provider);
            store_slab(&mut svm, key, &header, &entries, entries.len() as u32);
            measure(
                &svm,
                &payer,
                &reserve,
                changed,
                &format!("wallet_history_{history}"),
                true,
            );
        }
    }
}

#[test]
fn shared_limits_lifecycle_cu() {
    for changed in modes() {
        for count in [1, 32] {
            for descending in [false, true] {
                for generation in [None, Some(1)] {
                    let (mut svm, payer) = vm(changed);
                    let (mut reserve, booking) = prepare(
                        &mut svm, &payer, changed, count, descending, true, true, 0, 0,
                    );
                    set_request(&mut reserve, generation, None);
                    let mint = create_mint(&mut svm, &payer, payer.pubkey(), 6);
                    let fee_destination =
                        create_token_account(&mut svm, &payer, mint, payer.pubkey());
                    let user = reserve.accounts[4].pubkey;
                    let (authority, bump) = Address::find_program_address(
                        &[b"vault", user.as_ref()],
                        &inference_router_program::ID,
                    );
                    let vault = create_token_account(&mut svm, &payer, mint, authority);
                    mint_to(&mut svm, &payer, mint, vault, 1_000_000_000);
                    let market = reserve.accounts[0].pubkey;
                    let mut state: state_types::Market = account(&svm, market);
                    state.mint = mint;
                    store_account(&mut svm, market, &state);
                    let mut state: state_types::User = account(&svm, user);
                    state.vault = vault;
                    state.bump = bump;
                    store_account(&mut svm, user, &state);
                    let terms = reserve.accounts[2].pubkey;
                    let header: state_types::ListTerms = account(&svm, terms);
                    let mut offers =
                        rows::<state_types::ListTerms, state_types::ListOffer>(&svm, terms);
                    let provider = offers
                        .iter()
                        .min_by_key(|row| row.terms.input_rate.get())
                        .unwrap()
                        .terms
                        .provider;
                    let payout = create_token_account(&mut svm, &payer, mint, provider);
                    for row in &mut offers {
                        if row.terms.provider == provider {
                            row.terms.payout = payout;
                        }
                    }
                    store_slab(&mut svm, terms, &header, &offers, count as u32);
                    let suffix = format!(
                        "n{count}_{}_{}",
                        if descending { "last" } else { "first" },
                        if generation.is_some() {
                            "reusable"
                        } else {
                            "oneshot"
                        }
                    );
                    measure(
                        &svm,
                        &payer,
                        &reserve,
                        changed,
                        &format!("reserve_{suffix}"),
                        true,
                    );
                    apply(&mut svm, &payer, &reserve);
                    let booked: state_types::Booking = account(&svm, booking);
                    let release = release(&reserve, provider, generation);
                    measure(
                        &svm,
                        &payer,
                        &release,
                        changed,
                        &format!("release_{suffix}"),
                        true,
                    );
                    let expiry = instruction(
                        inference_router_program::ID,
                        accounts::ExpireListBooking {
                            base: accounts::ExpireBooking {
                                market,
                                model: reserve.accounts[1].pubkey,
                                capacity: reserve.accounts[3].pubkey,
                                user,
                                booking,
                            },
                            wallet_bookings: reserve.accounts[10].pubkey,
                        },
                        ix::ExpireListBooking { generation },
                    );
                    measure(
                        &svm,
                        &payer,
                        &expiry,
                        changed,
                        &format!("expiry_early_reject_{suffix}"),
                        false,
                    );
                    let mut expired = svm.clone();
                    let mut clock = expired.get_sysvar::<Clock>();
                    clock.unix_timestamp = booked.claim_end.get() + 1;
                    expired.set_sysvar(&clock);
                    measure(
                        &expired,
                        &payer,
                        &expiry,
                        changed,
                        &format!("expiry_{suffix}"),
                        true,
                    );
                    let mut claim = instruction(
                        inference_router_program::ID,
                        accounts::ClaimListBooking {
                            base: accounts::ClaimBooking {
                                market,
                                model: reserve.accounts[1].pubkey,
                                capacity: reserve.accounts[3].pubkey,
                                user,
                                booking,
                                provider,
                                vault,
                                payout,
                                mint,
                                authority,
                                token: anchor_spl::token::ID,
                            },
                            wallet_bookings: reserve.accounts[10].pubkey,
                        },
                        ix::ClaimListBooking {
                            claim: args::Claim {
                                uncached: 512,
                                cached: 0,
                                output: 256,
                                receipt: generation.map(|generation| args::ReceiptInput {
                                    owner: payer.pubkey(),
                                    user,
                                    booking,
                                    generation,
                                    charge: 776,
                                    claim_end: booked.claim_end.get(),
                                }),
                            },
                        },
                    );
                    claim.accounts.extend([
                        AccountMeta::new(fee_destination, false),
                        AccountMeta::new(fee_destination, false),
                    ]);
                    let result = measure(
                        &svm,
                        &payer,
                        &claim,
                        changed,
                        &format!("claim_{suffix}"),
                        true,
                    )
                    .unwrap();
                    let claimed: state_types::Booking = lifecycle::post(&result, booking);
                    assert_eq!(claimed.charge.get(), 776);
                    let mut released = svm.clone();
                    apply(&mut released, &payer, &release);
                    measure(
                        &released,
                        &payer,
                        &release,
                        changed,
                        &format!("double_release_reject_{suffix}"),
                        false,
                    );
                    if generation.is_some() {
                        set_request(&mut reserve, Some(2), None);
                        measure(
                            &released,
                            &payer,
                            &reserve,
                            changed,
                            &format!("reuse_after_release_{suffix}"),
                            true,
                        );
                        apply(&mut svm, &payer, &claim);
                        measure(
                            &svm,
                            &payer,
                            &reserve,
                            changed,
                            &format!("reuse_after_claim_{suffix}"),
                            true,
                        );
                    }
                    let close = instruction(
                        inference_router_program::ID,
                        accounts::CloseBooking {
                            booking,
                            recipient: payer.pubkey(),
                        },
                        ix::CloseBooking {},
                    );
                    measure(
                        &released,
                        &payer,
                        &close,
                        changed,
                        &format!("close_{suffix}"),
                        true,
                    );
                }
            }
        }
    }
}

fn limits(concurrency: u32, quota: u32, quota_window_seconds: u32) -> args::RequestLimits {
    args::RequestLimits {
        concurrency,
        quota,
        quota_window_seconds,
    }
}

fn identity(market: Address, provider: Address) -> (Address, u8) {
    Address::find_program_address(
        &[b"provider_limits", market.as_ref(), provider.as_ref()],
        &inference_router_program::ID,
    )
}

fn configure(
    svm: &mut LiteSVM,
    reserve: &Instruction,
    provider: Address,
    global: args::RequestLimits,
    wallet: args::RequestLimits,
) -> Instruction {
    let (profile, bump) = Address::find_program_address(
        &[
            b"provider",
            reserve.accounts[0].pubkey.as_ref(),
            provider.as_ref(),
        ],
        &inference_router_program::ID,
    );
    let mut state = state_types::Provider::zeroed();
    state.market = reserve.accounts[0].pubkey;
    state.authority = provider;
    state.bump = bump;
    store_account(svm, profile, &state);
    let identity = identity(reserve.accounts[0].pubkey, provider).0;
    if svm.get_account(&identity).is_none() {
        instruction(
            inference_router_program::ID,
            accounts::InitializeProviderLimits {
                provider,
                payer: reserve.accounts[7].pubkey,
                profile,
                page: reserve.accounts[3].pubkey,
                identity,
                system_program: anchor_lang::system_program::ID,
            },
            ix::InitializeProviderLimits {
                limits: args::ProviderLimits { global, wallet },
            },
        )
    } else {
        instruction(
            inference_router_program::ID,
            accounts::ConfigureProviderLimits {
                provider,
                identity,
                profile,
                page: reserve.accounts[3].pubkey,
            },
            ix::ConfigureProviderLimits {
                limits: args::ProviderLimits { global, wallet },
            },
        )
    }
}

fn fresh_booking(svm: &mut LiteSVM, reserve: &Instruction, seed: u8) -> Instruction {
    let mut result = reserve.clone();
    let key = Address::new_from_array([seed; 32]);
    let mut booking = state_types::Booking::zeroed();
    booking.owner = result.accounts[5].pubkey;
    booking.market = result.accounts[0].pubkey;
    booking.user = result.accounts[4].pubkey;
    booking.rent_payer = result.accounts[7].pubkey;
    booking.status = 5;
    store_account(svm, key, &booking);
    result.accounts[6].pubkey = key;
    result
}

fn another_scope(svm: &mut LiteSVM, reserve: &Instruction, seed: u8) -> Instruction {
    let mut result = fresh_booking(svm, reserve, seed);
    let model_id = Address::new_from_array([seed + 1; 32]);
    let (model, bump) = Address::find_program_address(
        &[
            b"model",
            result.accounts[0].pubkey.as_ref(),
            model_id.as_ref(),
        ],
        &inference_router_program::ID,
    );
    let mut model_state: state_types::Model = account(svm, result.accounts[1].pubkey);
    model_state.model_id = model_id;
    model_state.bump = bump;
    store_account(svm, model, &model_state);
    let list = Address::new_from_array([seed + 2; 32]);
    let list_state: state_types::List = account(svm, result.accounts[8].pubkey);
    let members = rows::<state_types::List, Address>(svm, result.accounts[8].pubkey);
    store_slab(svm, list, &list_state, &members, members.len() as u32);
    let (terms, bump) = Address::find_program_address(
        &[b"list_terms", list.as_ref(), model.as_ref()],
        &inference_router_program::ID,
    );
    let mut header: state_types::ListTerms = account(svm, result.accounts[2].pubkey);
    header.model = model;
    header.list = list;
    header.bump = bump;
    let offers =
        rows::<state_types::ListTerms, state_types::ListOffer>(svm, result.accounts[2].pubkey);
    store_slab(svm, terms, &header, &offers, offers.len() as u32);
    let (routing, bump) = Address::find_program_address(
        &[b"list_routing", list.as_ref()],
        &inference_router_program::ID,
    );
    let mut header: state_types::ListRouting = account(svm, result.accounts[9].pubkey);
    header.list = list;
    header.bump = bump;
    let mirrors =
        rows::<state_types::ListRouting, state_types::PolicyMirror>(svm, result.accounts[9].pubkey);
    store_slab(svm, routing, &header, &mirrors, mirrors.len() as u32);
    result.accounts[1].pubkey = model;
    result.accounts[2].pubkey = terms;
    result.accounts[8].pubkey = list;
    result.accounts[9].pubkey = routing;
    let mut request: args::ReserveRequest =
        anchor_lang::wincode::deserialize(&result.data[8..]).unwrap();
    request.model_id = model_id;
    result.data = ix::ReserveListBooking {
        request,
        generation: None,
        minimum_wallet_concurrency: None,
    }
    .data();
    result
}

fn other_wallet(svm: &mut LiteSVM, reserve: &Instruction, seed: u8) -> Instruction {
    let mut result = reserve.clone();
    let owner = Keypair::new_from_array([seed; 32]).pubkey();
    let user = Address::new_from_array([seed; 32]);
    let mut state: state_types::User = account(svm, result.accounts[4].pubkey);
    state.owner = owner;
    store_account(svm, user, &state);
    let (wallet, bump) = Address::find_program_address(
        &[
            b"provider_wallet",
            result.accounts[0].pubkey.as_ref(),
            owner.as_ref(),
        ],
        &inference_router_program::ID,
    );
    let mut header = state_types::ProviderWallet::zeroed();
    header.market = result.accounts[0].pubkey;
    header.owner = owner;
    header.rent_payer = result.accounts[7].pubkey;
    header.bump = bump;
    store_slab::<_, state_types::ProviderUsage>(svm, wallet, &header, &[], 8);
    result.accounts[4].pubkey = user;
    result.accounts[5].pubkey = owner;
    result.accounts[10].pubkey = wallet;
    fresh_booking(svm, &result, seed + 1)
}

fn rejected(svm: &LiteSVM, payer: &Keypair, ix: &Instruction, code: u32) {
    let error = svm.simulate_transaction(tx(svm, payer, ix)).unwrap_err();
    assert!(
        format!("{:?}", error.err).contains(&format!("Custom({code})")),
        "{error:?}"
    );
}

#[test]
fn shared_limits_behavior() {
    let (mut svm, payer) = vm(true);
    let (reserve, _) = prepare(&mut svm, &payer, true, 1, false, true, false, 0, 0);
    let provider =
        rows::<state_types::ListTerms, state_types::ListOffer>(&svm, reserve.accounts[2].pubkey)[0]
            .terms
            .provider;
    let configure_limits = configure(
        &mut svm,
        &reserve,
        provider,
        limits(2, 3, 60),
        limits(2, 3, 60),
    );
    apply(&mut svm, &payer, &configure_limits);
    let second = another_scope(&mut svm, &reserve, 81);
    let third = another_scope(&mut svm, &reserve, 91);
    apply(&mut svm, &payer, &reserve);
    apply(&mut svm, &payer, &second);
    rejected(&svm, &payer, &third, 6005);
    let entries = rows::<state_types::ProviderLimitPage, state_types::ProviderLimitEntry>(
        &svm,
        reserve.accounts[3].pubkey,
    );
    assert_eq!(
        (
            entries[0].active_bookings.get(),
            entries[0].quota_used.get()
        ),
        (2, 2)
    );
    let wallet = rows::<state_types::ProviderWallet, state_types::ProviderUsage>(
        &svm,
        reserve.accounts[10].pubkey,
    );
    assert_eq!(
        (wallet[0].active_bookings.get(), wallet[0].quota_used.get()),
        (2, 2)
    );
    // lowering cannot erase live counts or refund window consumption.
    let lower = configure(
        &mut svm,
        &reserve,
        provider,
        limits(1, 1, 1),
        limits(1, 1, 1),
    );
    apply(&mut svm, &payer, &lower);
    rejected(&svm, &payer, &third, 6005);
    apply(&mut svm, &payer, &release(&second, provider, None));
    rejected(&svm, &payer, &third, 6005);
    apply(&mut svm, &payer, &release(&reserve, provider, None));
    rejected(&svm, &payer, &third, 6005);
    let entry = rows::<state_types::ProviderLimitPage, state_types::ProviderLimitEntry>(
        &svm,
        reserve.accounts[3].pubkey,
    )[0];
    assert_eq!(
        (
            entry.active_bookings.get(),
            entry.quota_used.get(),
            entry.quota_window_end.get()
        ),
        (0, 2, 160)
    );
    let mut clock = svm.get_sysvar::<Clock>();
    clock.unix_timestamp = 160;
    svm.set_sysvar(&clock);
    apply(&mut svm, &payer, &third);
    let entry = rows::<state_types::ProviderLimitPage, state_types::ProviderLimitEntry>(
        &svm,
        reserve.accounts[3].pubkey,
    )[0];
    assert_eq!(
        (
            entry.active_bookings.get(),
            entry.quota_used.get(),
            entry.quota_window_end.get()
        ),
        (1, 1, 161)
    );
    // duration update applies at rollover; active quota_used remain counted across time.
    clock.unix_timestamp = 1000;
    svm.set_sysvar(&clock);
    let fourth = fresh_booking(&mut svm, &third, 101);
    rejected(&svm, &payer, &fourth, 6005);
    apply(&mut svm, &payer, &release(&third, provider, None));
    apply(&mut svm, &payer, &fourth);
}

#[test]
fn shared_limits_wallet_and_provider_isolation() {
    let (mut svm, payer) = vm(true);
    let (reserve, _) = prepare(&mut svm, &payer, true, 1, false, true, false, 0, 0);
    let provider =
        rows::<state_types::ListTerms, state_types::ListOffer>(&svm, reserve.accounts[2].pubkey)[0]
            .terms
            .provider;
    let config = configure(
        &mut svm,
        &reserve,
        provider,
        limits(2, 2, 60),
        limits(1, 1, 60),
    );
    apply(&mut svm, &payer, &config);
    let same = another_scope(&mut svm, &reserve, 81);
    let other = other_wallet(&mut svm, &same, 102);
    apply(&mut svm, &payer, &reserve);
    let lane = Address::new_from_array([233; 32]);
    let user_state: state_types::User = account(&svm, reserve.accounts[4].pubkey);
    store_account(&mut svm, lane, &user_state);
    let mut same_owner_lane = reserve.clone();
    same_owner_lane.accounts[4].pubkey = lane;
    let same_owner_lane = fresh_booking(&mut svm, &same_owner_lane, 234);
    rejected(&svm, &payer, &same_owner_lane, 6005);
    rejected(&svm, &payer, &same, 6005);
    apply(&mut svm, &payer, &other);
    let third_wallet = other_wallet(&mut svm, &same, 104);
    rejected(&svm, &payer, &third_wallet, 6005);
    apply(&mut svm, &payer, &release(&reserve, provider, None));
    rejected(&svm, &payer, &same, 6005);
    rejected(&svm, &payer, &third_wallet, 6005);
    let mut clock = svm.get_sysvar::<Clock>();
    clock.unix_timestamp = 160;
    svm.set_sysvar(&clock);
    apply(&mut svm, &payer, &third_wallet);
    let (mut svm, payer) = vm(true);
    let (reserve, _) = prepare(&mut svm, &payer, true, 2, false, true, false, 0, 0);
    let providers =
        rows::<state_types::ListTerms, state_types::ListOffer>(&svm, reserve.accounts[2].pubkey);
    for row in &providers {
        let config = configure(
            &mut svm,
            &reserve,
            row.terms.provider,
            limits(1, 1, 60),
            limits(1, 1, 60),
        );
        apply(&mut svm, &payer, &config);
    }
    apply(&mut svm, &payer, &reserve);
    let second = another_scope(&mut svm, &reserve, 81);
    apply(&mut svm, &payer, &second);
    assert_ne!(
        account::<state_types::Booking>(&svm, reserve.accounts[6].pubkey).provider,
        account::<state_types::Booking>(&svm, second.accounts[6].pubkey).provider
    );
    let rows = rows::<state_types::ProviderLimitPage, state_types::ProviderLimitEntry>(
        &svm,
        reserve.accounts[3].pubkey,
    );
    assert!(rows
        .iter()
        .all(|row| row.active_bookings.get() == 1 && row.quota_used.get() == 1));
}

#[test]
fn shared_limits_no_fragmentation_and_bypass_rejections() {
    let (mut svm, payer) = vm(true);
    let (mut reserve, _) = prepare(&mut svm, &payer, true, 1, false, true, false, 0, 0);
    let provider =
        rows::<state_types::ListTerms, state_types::ListOffer>(&svm, reserve.accounts[2].pubkey)[0]
            .terms
            .provider;
    let config = configure(
        &mut svm,
        &reserve,
        provider,
        limits(100, 0, 60),
        limits(100, 0, 60),
    );
    apply(&mut svm, &payer, &config);
    let terms = reserve.accounts[2].pubkey;
    let header: state_types::ListTerms = account(&svm, terms);
    let mut offers = rows::<state_types::ListTerms, state_types::ListOffer>(&svm, terms);
    offers[0].terms.capacity = 1.into();
    store_slab(&mut svm, terms, &header, &offers, 1);
    set_request(&mut reserve, None, Some(100));
    for index in 0..100 {
        let request = fresh_booking(&mut svm, &reserve, 120 + index);
        apply(&mut svm, &payer, &request);
    }
    let entry = rows::<state_types::ProviderLimitPage, state_types::ProviderLimitEntry>(
        &svm,
        reserve.accounts[3].pubkey,
    )[0];
    assert_eq!(entry.active_bookings.get(), 100);
    let exhausted = fresh_booking(&mut svm, &reserve, 220);
    let tracked = [
        reserve.accounts[3].pubkey,
        reserve.accounts[4].pubkey,
        reserve.accounts[10].pubkey,
        exhausted.accounts[6].pubkey,
    ];
    let before: Vec<_> = tracked
        .iter()
        .map(|key| svm.get_account(key).unwrap())
        .collect();
    rejected(&svm, &payer, &exhausted, 6005);
    assert!(svm.send_transaction(tx(&svm, &payer, &exhausted)).is_err());
    for (key, before) in tracked.into_iter().zip(before) {
        assert_eq!(svm.get_account(&key).unwrap(), before);
    }
    let mut invalid = config.clone();
    invalid.accounts[0].pubkey = payer.pubkey();
    rejected(&svm, &payer, &invalid, 6003);
    let mut invalid = reserve.clone();
    invalid.accounts[3].pubkey = reserve.accounts[10].pubkey;
    assert!(svm
        .simulate_transaction(tx(&svm, &payer, &invalid))
        .is_err());
    let foreign = Address::new_from_array([230; 32]);
    let mut header: state_types::ProviderLimitPage = account(&svm, reserve.accounts[3].pubkey);
    header.market = foreign;
    store_slab(&mut svm, foreign, &header, &[entry], 1);
    invalid.accounts[3].pubkey = foreign;
    rejected(&svm, &payer, &invalid, 6001);
    let mut pending = reserve.clone();
    pending.accounts[6].pubkey = Address::new_from_array([120; 32]);
    let mut foreign_release = release(&pending, provider, None);
    foreign_release.accounts[2].pubkey = foreign;
    rejected(&svm, &payer, &foreign_release, 6001);
    let before = svm.get_account(&reserve.accounts[10].pubkey).unwrap();
    assert!(svm
        .send_transaction(tx(&svm, &payer, &foreign_release))
        .is_err());
    assert_eq!(
        svm.get_account(&reserve.accounts[10].pubkey).unwrap(),
        before
    );
    let mut legacy = reserve.clone();
    legacy.data[..8].copy_from_slice(ix::ReserveBooking::DISCRIMINATOR);
    assert!(svm.simulate_transaction(tx(&svm, &payer, &legacy)).is_err());
    let wallet = reserve.accounts[10].pubkey;
    let close = instruction(
        inference_router_program::ID,
        accounts::CloseProviderWallet {
            owner: payer.pubkey(),
            wallet,
            recipient: payer.pubkey(),
        },
        ix::CloseProviderWallet {},
    );
    rejected(&svm, &payer, &close, 6006);
    let prune = instruction(
        inference_router_program::ID,
        accounts::PruneProviderWallet {
            owner: payer.pubkey(),
            wallet,
        },
        ix::PruneProviderWallet {},
    );
    apply(&mut svm, &payer, &prune);
    assert_eq!(
        rows::<state_types::ProviderWallet, state_types::ProviderUsage>(&svm, wallet)[0]
            .active_bookings
            .get(),
        100
    );
}

#[test]
fn shared_limits_rate_cu() {
    for changed in modes() {
        for count in [1, 32, 4096] {
            for scenario in [
                "rate_enabled",
                "rate_reject_all",
                "rate_last",
                "rate_rollover",
            ] {
                let (mut svm, payer) = vm(changed);
                let (reserve, _) =
                    prepare(&mut svm, &payer, changed, count, false, true, true, 0, 0);
                if changed {
                    let key = reserve.accounts[3].pubkey;
                    let header: state_types::ProviderLimitPage = account(&svm, key);
                    let mut entries = rows::<
                        state_types::ProviderLimitPage,
                        state_types::ProviderLimitEntry,
                    >(&svm, key);
                    for row in &mut entries {
                        row.quota_limit = 10.into();
                        row.quota_used = (if scenario == "rate_enabled" { 1 } else { 10 }).into();
                        row.quota_window_end = (if scenario == "rate_rollover" {
                            100i64
                        } else {
                            160
                        })
                        .into();
                    }
                    if scenario == "rate_last" {
                        entries.last_mut().unwrap().quota_used = 0.into();
                    }
                    store_slab(&mut svm, key, &header, &entries, count as u32);
                }
                let result = measure(
                    &svm,
                    &payer,
                    &reserve,
                    changed,
                    &format!("provider_{scenario}_n{count}"),
                    !changed || scenario != "rate_reject_all",
                );
                if let Some(result) = result {
                    if changed && scenario == "rate_last" {
                        let booked: state_types::Booking =
                            lifecycle::post(&result, reserve.accounts[6].pubkey);
                        assert_eq!(
                            booked.provider,
                            rows::<state_types::ListTerms, state_types::ListOffer>(
                                &svm,
                                reserve.accounts[2].pubkey
                            )
                            .last()
                            .unwrap()
                            .terms
                            .provider
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn shared_limits_configure_cu() {
    for changed in modes() {
        let (mut svm, payer) = vm(changed);
        let (reserve, _) = prepare(&mut svm, &payer, changed, 32, false, true, true, 0, 0);
        let provider = rows::<state_types::ListRouting, state_types::PolicyMirror>(
            &svm,
            reserve.accounts[9].pubkey,
        )[0]
        .provider;
        if changed {
            for providers in [1u32, 32, 4096, 100_000] {
                let shared =
                    Slab::<state_types::ProviderLimitPage, state_types::ProviderLimitEntry>::space_for(
                        providers,
                    );
                let local =
                    Slab::<state_types::ListCapacity, state_types::ListCapacityEntry>::space_for(
                        providers,
                    );
                eprintln!("LIMITS_RENT providers={providers} shared_bytes={shared} shared_lamports={} list_bytes={local} list_lamports={}", svm.minimum_balance_for_rent_exemption(shared), svm.minimum_balance_for_rent_exemption(local));
            }
        }
        if changed {
            let config = configure(
                &mut svm,
                &reserve,
                provider,
                limits(10, 10, 60),
                limits(2, 3, 60),
            );
            measure(
                &svm,
                &payer,
                &config,
                true,
                "limits_configure_existing",
                true,
            );
            let result = measure(
                &svm,
                &payer,
                &config,
                true,
                "quota_update_workflow_1_list",
                true,
            )
            .unwrap();
            assert!(result.meta.compute_units_consumed > 0);
            let new_provider = Keypair::new_from_array([42; 32]).pubkey();
            let key = reserve.accounts[3].pubkey;
            let header: state_types::ProviderLimitPage = account(&svm, key);
            let entries =
                rows::<state_types::ProviderLimitPage, state_types::ProviderLimitEntry>(&svm, key);
            store_slab(&mut svm, key, &header, &entries, 33);
            let config = configure(
                &mut svm,
                &reserve,
                new_provider,
                limits(10, 10, 60),
                limits(2, 3, 60),
            );
            let initialized =
                measure(&svm, &payer, &config, true, "limits_configure_new", true).unwrap();
            let identity_key = identity(reserve.accounts[0].pubkey, new_provider).0;
            let created: solana_account::Account = initialized
                .post_accounts
                .iter()
                .find(|(key, _)| *key == identity_key)
                .unwrap()
                .1
                .clone()
                .into();
            eprintln!("LIMITS_CREATED mode=changed scenario=limits_configure_new bytes={} rent={} row_bytes={} row_incremental_rent={}", created.data.len(), created.lamports, std::mem::size_of::<state_types::ProviderLimitEntry>(), svm.minimum_balance_for_rent_exemption(40)-svm.minimum_balance_for_rent_exemption(0));
            let grow = instruction(
                inference_router_program::ID,
                accounts::GrowProviderLimitPage {
                    page: key,
                    payer: payer.pubkey(),
                    system_program: anchor_lang::system_program::ID,
                },
                ix::GrowProviderLimitPage { limit: 64 },
            );
            measure(&svm, &payer, &grow, true, "limits_grow_33_to_64", true);
            let market = Address::new_from_array([222; 32]);
            let state: state_types::Market = account(&svm, reserve.accounts[0].pubkey);
            store_account(&mut svm, market, &state);
            let key = Address::find_program_address(
                &[b"provider_limit_page", market.as_ref(), &[0]],
                &inference_router_program::ID,
            )
            .0;
            let init = instruction(
                inference_router_program::ID,
                accounts::InitializeProviderLimitPage {
                    market,
                    payer: payer.pubkey(),
                    page: key,
                    system_program: anchor_lang::system_program::ID,
                },
                ix::InitializeProviderLimitPage { limit: 32, page: 0 },
            );
            measure(&svm, &payer, &init, true, "limits_initialize_32", true);
        } else {
            let market = reserve.accounts[0].pubkey;
            let (policy, bump) = Address::find_program_address(
                &[b"provider_policy", market.as_ref(), provider.as_ref()],
                &inference_router_program::ID,
            );
            let mut state = state_types::ProviderPolicy::zeroed();
            state.market = market;
            state.provider = provider;
            state.wallet_quota_window_seconds = 60.into();
            state.wallet_quota = 10.into();
            state.wallet_concurrency = 10.into();
            state.epoch = 1.into();
            state.active_mirrors = 1.into();
            state.bump = bump;
            store_account(&mut svm, policy, &state);
            let instructions = [
                instruction(
                    inference_router_program::ID,
                    accounts::ConfigureProviderPolicy { policy, provider },
                    ix::BeginProviderPolicyUpdate {},
                ),
                instruction(
                    inference_router_program::ID,
                    accounts::FreezePolicyMirror {
                        signer: provider,
                        policy,
                        list: reserve.accounts[8].pubkey,
                        routing: reserve.accounts[9].pubkey,
                    },
                    ix::FreezePolicyMirror {},
                ),
                instruction(
                    inference_router_program::ID,
                    accounts::ConfigureProviderPolicy { policy, provider },
                    ix::CommitProviderPolicyUpdate {
                        limits: args::BookingLimits {
                            wallet_quota_window_seconds: 60,
                            wallet_quota: 3,
                            wallet_concurrency: 2,
                        },
                    },
                ),
                instruction(
                    inference_router_program::ID,
                    accounts::EnablePolicyMirror {
                        provider,
                        policy,
                        list: reserve.accounts[8].pubkey,
                        routing: reserve.accounts[9].pubkey,
                    },
                    ix::EnablePolicyMirror {},
                ),
            ];
            let mut total = 0;
            for (name, instruction) in [
                "quota_begin_update",
                "quota_freeze",
                "quota_commit",
                "quota_enable",
            ]
            .into_iter()
            .zip(instructions)
            {
                total += measure(&svm, &payer, &instruction, false, name, true)
                    .unwrap()
                    .meta
                    .compute_units_consumed;
                apply(&mut svm, &payer, &instruction);
            }
            eprintln!("LIMITS_CU mode=baseline scenario=quota_update_workflow_1_list cu={total} success=true loaded=0 packet=0 accounts=0 rent=0 samples=3");
        }
    }
}

#[test]
fn shared_limits_offer_cu() {
    for changed in modes() {
        let (mut svm, payer) = vm(changed);
        let (reserve, _) = prepare(&mut svm, &payer, changed, 32, false, true, true, 0, 0);
        let terms = reserve.accounts[2].pubkey;
        let mut header: state_types::ListTerms = account(&svm, terms);
        let capacity = header.capacity;
        let mut offers = rows::<state_types::ListTerms, state_types::ListOffer>(&svm, terms);
        let row = offers.pop().unwrap();
        header.next_order = 31.into();
        store_slab(&mut svm, terms, &header, &offers, 32);
        let cap_header: state_types::ListCapacity = account(&svm, capacity);
        let mut caps =
            rows::<state_types::ListCapacity, state_types::ListCapacityEntry>(&svm, capacity);
        caps.pop();
        store_slab(&mut svm, capacity, &cap_header, &caps, 32);
        let provider = row.terms.provider;
        let market = reserve.accounts[0].pubkey;
        let model = reserve.accounts[1].pubkey;
        let mint = create_mint(&mut svm, &payer, payer.pubkey(), 6);
        let mut state: state_types::Market = account(&svm, market);
        state.mint = mint;
        store_account(&mut svm, market, &state);
        let payout = create_token_account(&mut svm, &payer, mint, provider);
        let (budget, bump) = Address::find_program_address(
            &[b"provider_capacity", model.as_ref(), provider.as_ref()],
            &inference_router_program::ID,
        );
        let mut state = state_types::ProviderModelCapacity::zeroed();
        state.model = model;
        state.provider = provider;
        state.total = 100.into();
        state.legacy_slot = u32::MAX.into();
        state.bump = bump;
        store_account(&mut svm, budget, &state);
        let offer = args::Offer {
            slot: 0,
            input_rate: row.terms.input_rate.get(),
            cached_rate: row.terms.cached_rate.get(),
            output_rate: row.terms.output_rate.get(),
            speed: row.terms.speed.get(),
            privacy: true,
            active: true,
            capacity: 10,
            checkpoint: row.terms.checkpoint,
            input_modalities: row.terms.input_modalities,
            output_modalities: row.terms.output_modalities,
            context_window: row.terms.context_window.get(),
            max_input: row.terms.max_input.get(),
            max_output: row.terms.max_output.get(),
        };
        let mut set = instruction(
            inference_router_program::ID,
            accounts::SetListOffer {
                market,
                model,
                provider,
                identity: identity(market, provider).0,
                list: reserve.accounts[8].pubkey,
                routing: reserve.accounts[9].pubkey,
                terms,
                capacity,
                payout,
            },
            ix::SetListOffer { offer },
        );
        if !changed {
            set.accounts[3] = AccountMeta::new(budget, false);
        }
        measure(&svm, &payer, &set, changed, "offer_register_slot31", true);
        apply(&mut svm, &payer, &set);
        measure(&svm, &payer, &set, changed, "offer_update_slot31", true);
        let mut removed = svm.clone();
        let mut zero = offer;
        zero.capacity = 0;
        set.data = ix::SetListOffer { offer: zero }.data();
        measure(&removed, &payer, &set, changed, "offer_remove_slot31", true);
        apply(&mut removed, &payer, &set);
        let list = reserve.accounts[8].pubkey;
        let header: state_types::List = account(&svm, list);
        let mut members = rows::<state_types::List, Address>(&svm, list);
        members.retain(|key| *key != provider);
        store_slab(&mut svm, list, &header, &members, 32);
        let mut prune = instruction(
            inference_router_program::ID,
            accounts::PruneListOffer {
                list,
                terms,
                capacity,
                provider,
            },
            ix::PruneListOffer {},
        );
        if !changed {
            prune.accounts[3] = AccountMeta::new(budget, false);
        }
        measure(&svm, &payer, &prune, changed, "offer_prune_slot31", true);
    }
}

#[test]
fn shared_limits_appended_provider_preserves_stable_index() {
    let (mut svm, payer) = vm(true);
    let (reserve, _) = prepare(&mut svm, &payer, true, 1, false, true, false, 0, 0);
    apply(&mut svm, &payer, &reserve);
    let booked: state_types::Booking = account(&svm, reserve.accounts[6].pubkey);
    assert_eq!(booked.slot.get(), 0);
    let key = reserve.accounts[3].pubkey;
    let header: state_types::ProviderLimitPage = account(&svm, key);
    let entries =
        rows::<state_types::ProviderLimitPage, state_types::ProviderLimitEntry>(&svm, key);
    store_slab(&mut svm, key, &header, &entries, 2);
    let provider = Address::new_from_array([0; 32]);
    let config = configure(
        &mut svm,
        &reserve,
        provider,
        limits(1, 1, 60),
        limits(1, 1, 60),
    );
    apply(&mut svm, &payer, &config);
    let entries =
        rows::<state_types::ProviderLimitPage, state_types::ProviderLimitEntry>(&svm, key);
    assert_eq!(entries[0].active_bookings.get(), 1);
    assert_eq!(entries[1].active_bookings.get(), 0);
    apply(&mut svm, &payer, &release(&reserve, booked.provider, None));
    let entries =
        rows::<state_types::ProviderLimitPage, state_types::ProviderLimitEntry>(&svm, key);
    assert!(entries.iter().all(|row| row.active_bookings.get() == 0));
    assert_eq!(entries[0].quota_used.get(), 1);
}

#[test]
fn shared_limits_legacy_drain_cu() {
    if std::env::var_os("FOSSA_CU_BASELINE").is_none() {
        return;
    }
    for changed in modes() {
        for marker in [0u8, 1, 2] {
            let (mut svm, payer) = vm(false);
            let (old, booking) = fixture(&mut svm, &payer, 32, false, Case::Compatible, false);
            let legacy_wallet = old.accounts[10].pubkey;
            let mut reserve = if marker == 2 {
                scoped_bench::convert(&mut svm, &payer, &old, true, true, 0)
            } else {
                let mut instruction = old;
                instruction.accounts.drain(9..11);
                lifecycle::legacy_terms(&mut svm, instruction.accounts[2].pubkey);
                instruction
            };
            let request: args::ReserveRequest =
                anchor_lang::wincode::deserialize(&reserve.data[8..]).unwrap();
            reserve.data = if marker == 2 {
                ix::ReserveListBooking {
                    request,
                    generation: Some(1),
                    minimum_wallet_concurrency: None,
                }
                .data()
            } else {
                ix::ReserveBooking {
                    request,
                    generation: Some(1),
                }
                .data()
            };
            let market = reserve.accounts[0].pubkey;
            let model = reserve.accounts[1].pubkey;
            let terms = reserve.accounts[2].pubkey;
            let capacity = reserve.accounts[3].pubkey;
            let user = reserve.accounts[4].pubkey;
            let mint = create_mint(&mut svm, &payer, payer.pubkey(), 6);
            let fee_destination = create_token_account(&mut svm, &payer, mint, payer.pubkey());
            let mut market_state: state_types::Market = account(&svm, market);
            market_state.mint = mint;
            store_account(&mut svm, market, &market_state);
            let provider = if marker == 2 {
                rows::<state_types::ListTerms, state_types::ListOffer>(&svm, terms)[0]
                    .terms
                    .provider
            } else {
                rows::<state_types::FilteredTerms, state_types::LegacyTermsEntry>(&svm, terms)[0]
                    .provider
            };
            let payout = create_token_account(&mut svm, &payer, mint, provider);
            if marker == 2 {
                let header: state_types::ListTerms = account(&svm, terms);
                let mut offers =
                    rows::<state_types::ListTerms, state_types::ListOffer>(&svm, terms);
                offers[0].terms.payout = payout;
                store_slab(&mut svm, terms, &header, &offers, 32);
            } else {
                let header: state_types::FilteredTerms = account(&svm, terms);
                let mut offers =
                    rows::<state_types::FilteredTerms, state_types::LegacyTermsEntry>(&svm, terms);
                offers[0].payout = payout;
                store_slab(&mut svm, terms, &header, &offers, 32);
            }
            let (authority, bump) = Address::find_program_address(
                &[b"vault", user.as_ref()],
                &inference_router_program::ID,
            );
            let vault = create_token_account(&mut svm, &payer, mint, authority);
            mint_to(&mut svm, &payer, mint, vault, 1_000_000_000);
            let mut user_state: state_types::User = account(&svm, user);
            user_state.vault = vault;
            user_state.bump = bump;
            store_account(&mut svm, user, &user_state);
            if marker == 0 {
                if changed {
                    svm.add_program_from_file(
                        inference_router_program::ID,
                        program_path("inference_router_program.so"),
                    )
                    .unwrap();
                }
                measure(
                    &svm,
                    &payer,
                    &reserve,
                    changed,
                    "legacy_reserve_before_activation",
                    true,
                );
            }
            apply(&mut svm, &payer, &reserve);
            let mut booked: state_types::Booking = account(&svm, booking);
            if marker == 1 {
                booked.wallet_booking_counted = 1;
                booked.provider_policy = 0.into();
                store_account(&mut svm, booking, &booked);
                let header: state_types::WalletBookings = account(&svm, legacy_wallet);
                store_slab(
                    &mut svm,
                    legacy_wallet,
                    &header,
                    &[state_types::WalletBookingEntry {
                        provider_policy: 0.into(),
                        quota_window_end: 160.into(),
                        quota_used: 1.into(),
                        active_bookings: 1.into(),
                    }],
                    8,
                );
            }
            if changed {
                svm.add_program_from_file(
                    inference_router_program::ID,
                    program_path("inference_router_program.so"),
                )
                .unwrap();
            }
            market_state.reserved[0] = 1;
            store_account(&mut svm, market, &market_state);
            if marker < 2 {
                measure(
                    &svm,
                    &payer,
                    &reserve,
                    changed,
                    &format!("legacy_reserve_after_activation_marker{marker}"),
                    false,
                );
            }
            let wallet = if marker == 2 {
                reserve.accounts[10].pubkey
            } else {
                legacy_wallet
            };
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
            let mut expiry = instruction(
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
            if marker == 0 {
                release.accounts.pop();
                release.data[..8].copy_from_slice(ix::ReleaseBooking::DISCRIMINATOR);
                expiry.accounts.pop();
                expiry.data[..8].copy_from_slice(ix::ExpireBooking::DISCRIMINATOR);
                claim.accounts.pop();
                claim.data[..8].copy_from_slice(ix::ClaimBooking::DISCRIMINATOR);
            }
            claim.accounts.extend([
                AccountMeta::new(fee_destination, false),
                AccountMeta::new(fee_destination, false),
            ]);
            measure(
                &svm,
                &payer,
                &release,
                changed,
                &format!("legacy_release_marker{marker}"),
                true,
            );
            measure(
                &svm,
                &payer,
                &claim,
                changed,
                &format!("legacy_claim_marker{marker}"),
                true,
            );
            let mut clock = svm.get_sysvar::<Clock>();
            clock.unix_timestamp = booked.claim_end.get() + 1;
            svm.set_sysvar(&clock);
            measure(
                &svm,
                &payer,
                &expiry,
                changed,
                &format!("legacy_expiry_marker{marker}"),
                true,
            );
        }
    }
}

#[path = "paged_limits.rs"]
mod paged;
