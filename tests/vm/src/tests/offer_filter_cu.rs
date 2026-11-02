use super::*;
use anchor_lang::{
    accounts::{Slab, SlabSchema},
    bytemuck::{Pod, Zeroable},
    Discriminator, Owner,
};
use inference_router_program::{accounts, args, instruction as ix, state_types};
use solana_account::ReadableAccount;

#[path = "booking_lifecycle_cu.rs"]
mod lifecycle;
#[path = "scoped_cu.rs"]
mod scoped_bench;

#[derive(Clone, Copy, Debug, PartialEq)]
enum Case {
    Compatible,
    Checkpoint,
    InputModality,
    OutputModality,
    Context,
    InputCap,
    OutputCap,
    NoMatch,
    UnlistedCheapest,
    UnlistedPadded,
    OnlyExpensiveListed,
    NoneListed,
    UnlistedSeveral,
    PaddedExpensiveOnly,
    PaddedNoneListed,
    ShuffledCompatible,
}

fn support(entry: &mut state_types::TermsEntry, case: Case, compatible: bool) {
    entry.checkpoint = [7; 32];
    entry.input_modalities = state_types::MODALITY_TEXT | state_types::MODALITY_IMAGE;
    entry.output_modalities = state_types::MODALITY_TEXT | state_types::MODALITY_AUDIO;
    entry.version = state_types::OFFER_VERSION;
    entry.context_window = 8192.into();
    entry.max_input = 4096.into();
    entry.max_output = 4096.into();
    if !compatible {
        match case {
            Case::Checkpoint | Case::NoMatch => entry.checkpoint = [8; 32],
            Case::InputModality => entry.input_modalities = state_types::MODALITY_IMAGE,
            Case::OutputModality => entry.output_modalities = state_types::MODALITY_AUDIO,
            Case::Context => entry.context_window = 1535.into(),
            Case::InputCap => entry.max_input = 1023.into(),
            Case::OutputCap => entry.max_output = 511.into(),
            Case::Compatible
            | Case::UnlistedCheapest
            | Case::UnlistedPadded
            | Case::OnlyExpensiveListed
            | Case::NoneListed
            | Case::UnlistedSeveral
            | Case::PaddedExpensiveOnly
            | Case::PaddedNoneListed
            | Case::ShuffledCompatible => {}
        }
    }
}

pub(super) fn store_slab<H, T>(
    svm: &mut LiteSVM,
    address: Address,
    header: &H,
    entries: &[T],
    capacity: u32,
) where
    H: Discriminator + Owner + Pod + Zeroable + SlabSchema,
    T: Pod,
{
    assert!(entries.len() <= capacity as usize);
    let mut data = vec![0; Slab::<H, T>::space_for(capacity)];
    let header_at = H::DISCRIMINATOR.len();
    let len_at = header_at + std::mem::size_of::<H>();
    data[..header_at].copy_from_slice(H::DISCRIMINATOR);
    data[header_at..len_at].copy_from_slice(anchor_lang::bytemuck::bytes_of(header));
    data[len_at..len_at + std::mem::size_of::<u32>()]
        .copy_from_slice(&(entries.len() as u32).to_le_bytes());
    let entries_at = Slab::<H, T>::space_for(0);
    let entries_bytes = anchor_lang::bytemuck::cast_slice(entries);
    data[entries_at..entries_at + entries_bytes.len()].copy_from_slice(entries_bytes);
    svm.set_account(
        address,
        solana_account::Account {
            lamports: svm.minimum_balance_for_rent_exemption(data.len()),
            data,
            owner: H::OWNER,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
}

fn cheapest_slot(count: usize, descending: bool, case: Case) -> usize {
    if case == Case::ShuffledCompatible {
        (0..count)
            .find(|index| (index * 73 + 19) % count == 0)
            .unwrap()
    } else if descending {
        count - 1
    } else {
        0
    }
}

fn fixture(
    svm: &mut LiteSVM,
    payer: &Keypair,
    count: usize,
    descending: bool,
    case: Case,
    cached: bool,
) -> (solana_instruction::Instruction, Address) {
    let market = Address::new_from_array([11; 32]);
    let model_id = Address::new_from_array([12; 32]);
    let (model, bump) = Address::find_program_address(
        &[b"model", market.as_ref(), model_id.as_ref()],
        &inference_router_program::ID,
    );
    let terms = Address::new_from_array([13; 32]);
    let capacity = Address::new_from_array([14; 32]);
    let user = Address::new_from_array([15; 32]);
    let list = Address::new_from_array([16; 32]);
    let booking = Address::new_from_array([17; 32]);
    let mut market_state = state_types::Market::zeroed();
    market_state.council = payer.pubkey();
    market_state.mint = Address::new_from_array([18; 32]);
    store_account(svm, market, &market_state);
    let mut model_state = state_types::Model::zeroed();
    model_state.market = market;
    model_state.model_id = model_id;
    model_state.terms = terms;
    model_state.capacity = capacity;
    model_state.limit = (count as u32).into();
    model_state.bump = bump;
    store_account(svm, model, &model_state);
    let mut providers = Vec::with_capacity(count);
    let mut offers = Vec::with_capacity(count);
    for index in 0..count {
        let mut bytes = [33; 32];
        bytes[..4].copy_from_slice(&(index as u32 + 1).to_le_bytes());
        let provider = Address::new_from_array(bytes);
        providers.push(provider);
        let rank = if case == Case::ShuffledCompatible {
            (index * 73 + 19) % count + 1
        } else if descending {
            count - index
        } else {
            index + 1
        };
        let rate = rank as u32 * 1_000_000;
        let mut entry = state_types::TermsEntry {
            provider,
            provider_policy: (index as u32).into(),
            payout: Address::new_from_array([34; 32]),
            input_rate: rate.into(),
            cached_rate: (if cached { rate / 2 } else { rate }).into(),
            output_rate: rate.into(),
            speed: 100.into(),
            privacy: 1,
            active: 1,
            capacity: 10.into(),
            ..state_types::TermsEntry::zeroed()
        };
        support(
            &mut entry,
            case,
            case == Case::Compatible || index + 1 == count && case != Case::NoMatch,
        );
        offers.push(entry);
    }
    let (policies, policy_bump) = Address::find_program_address(
        &[b"booking_policies", market.as_ref()],
        &inference_router_program::ID,
    );
    let mut policy_header = state_types::BookingPolicies::zeroed();
    policy_header.market = market;
    policy_header.bump = policy_bump;
    let policy_entries: Vec<_> = offers
        .iter()
        .map(|offer| state_types::BookingPolicy {
            provider: offer.provider,
            wallet_window_seconds: 60.into(),
            wallet_bookings_per_window: 0.into(),
            wallet_booking_capacity: 0.into(),
        })
        .collect();
    store_slab(svm, policies, &policy_header, &policy_entries, count as u32);
    let (wallet_bookings, wallet_bump) = Address::find_program_address(
        &[b"wallet_bookings", market.as_ref(), payer.pubkey().as_ref()],
        &inference_router_program::ID,
    );
    let mut wallet_header = state_types::WalletBookings::zeroed();
    wallet_header.market = market;
    wallet_header.owner = payer.pubkey();
    wallet_header.rent_payer = payer.pubkey();
    wallet_header.bump = wallet_bump;
    store_slab::<_, state_types::WalletBookingEntry>(svm, wallet_bookings, &wallet_header, &[], 8);
    providers.sort_unstable();
    match case {
        Case::UnlistedCheapest | Case::UnlistedPadded => providers.retain(|provider| {
            *provider != offers[if descending { count - 1 } else { 0 }].provider
        }),
        Case::OnlyExpensiveListed | Case::PaddedExpensiveOnly => providers.retain(|provider| {
            *provider == offers[if descending { 0 } else { count - 1 }].provider
        }),
        Case::NoneListed | Case::PaddedNoneListed => providers.clear(),
        Case::UnlistedSeveral => providers.retain(|provider| {
            let missing = 8.min(count - 1);
            !offers.iter().enumerate().any(|(index, offer)| {
                offer.provider == *provider
                    && (if descending { count - 1 - index } else { index }) < missing
            })
        }),
        _ => {}
    }
    if case == Case::UnlistedPadded {
        providers.push(Address::new_from_array([240; 32]));
        providers.sort_unstable();
    }
    if matches!(
        case,
        Case::UnlistedSeveral | Case::PaddedExpensiveOnly | Case::PaddedNoneListed
    ) {
        while providers.len() < count {
            let mut unrelated = [241; 32];
            unrelated[..4].copy_from_slice(&(50_000 + providers.len() as u32).to_le_bytes());
            providers.push(Address::new_from_array(unrelated));
        }
        providers.sort_unstable();
    }
    let mut header = state_types::Terms::zeroed();
    header.model = model;
    header.limit = (count as u32).into();
    store_slab(svm, terms, &header, &offers, count as u32);
    let mut header = state_types::Capacity::zeroed();
    header.model = model;
    header.limit = (count as u32).into();
    store_slab(
        svm,
        capacity,
        &header,
        &vec![state_types::CapacityEntry::zeroed(); count],
        count as u32,
    );
    let mut header = state_types::List::zeroed();
    header.market = market;
    header.authority = payer.pubkey();
    header.limit = (count as u32).into();
    header.fee_bps = 100.into();
    header.protocol_share = 10_000.into();
    store_slab(svm, list, &header, &providers, count as u32);
    let mut user_state = state_types::User::zeroed();
    user_state.owner = payer.pubkey();
    user_state.market = market;
    user_state.free = 1_000_000_000u64.into();
    store_account(svm, user, &user_state);
    let mut booking_state = state_types::Booking::zeroed();
    booking_state.owner = payer.pubkey();
    booking_state.market = market;
    booking_state.user = user;
    booking_state.rent_payer = payer.pubkey();
    booking_state.status = 5;
    store_account(svm, booking, &booking_state);
    let request = args::ReserveRequest {
        model_id,
        input_rate_cap: u32::MAX,
        cached_rate_cap: u32::MAX,
        output_rate_cap: u32::MAX,
        min_speed: 1,
        max_input: 1024,
        max_output: 512,
        private: true,
        spend: 1_000_000_000,
        estimated_input: 512,
        estimated_cached: if cached { 256 } else { 0 },
        estimated_output: 256,
        cache_provider: if cached {
            offers[cheapest_slot(count, descending, case)].provider
        } else {
            Address::default()
        },
        checkpoint: [7; 32],
        input_modalities: state_types::MODALITY_TEXT,
        output_modalities: state_types::MODALITY_TEXT,
        min_context: 0,
        limits_configured: false,
    };
    (
        {
            let mut ix = instruction(
                inference_router_program::ID,
                accounts::ReserveBooking {
                    market,
                    model,
                    terms,
                    capacity,
                    user,
                    owner: payer.pubkey(),
                    booking,
                    rent_payer: payer.pubkey(),
                    list,
                    group: None,
                },
                ix::ReserveBooking {
                    request,
                    generation: None,
                },
            );
            ix.accounts.insert(
                9,
                solana_instruction::AccountMeta::new_readonly(policies, false),
            );
            ix.accounts.insert(
                10,
                solana_instruction::AccountMeta::new(wallet_bookings, false),
            );
            ix
        },
        booking,
    )
}

fn expected_slot(count: usize, descending: bool, case: Case) -> Option<usize> {
    match case {
        Case::NoMatch | Case::NoneListed | Case::PaddedNoneListed => None,
        Case::Compatible | Case::ShuffledCompatible => Some(cheapest_slot(count, descending, case)),
        Case::UnlistedCheapest | Case::UnlistedPadded => {
            Some(if descending { count - 2 } else { 1 })
        }
        Case::OnlyExpensiveListed | Case::PaddedExpensiveOnly => {
            Some(if descending { 0 } else { count - 1 })
        }
        Case::UnlistedSeveral => {
            let missing = 8.min(count - 1);
            Some(if descending {
                count - 1 - missing
            } else {
                missing
            })
        }
        _ => Some(count - 1),
    }
}

fn measure(svm: &mut LiteSVM, payer: &Keypair, count: usize, descending: bool, case: Case) {
    measure_inner(svm, payer, count, descending, case, false);
}

fn measure_inner(
    svm: &mut LiteSVM,
    payer: &Keypair,
    count: usize,
    descending: bool,
    case: Case,
    cached: bool,
) {
    let (reserve, booking) = fixture(svm, payer, count, descending, case, cached);
    let transaction = Transaction::new(
        &[payer],
        Message::new(&[reserve], Some(&payer.pubkey())),
        svm.latest_blockhash(),
    );
    let mut samples = Vec::new();
    let mut exhausted = false;
    for _ in 0..3 {
        match svm.simulate_transaction(transaction.clone()) {
            Err(failure)
                if failure.meta.compute_units_consumed == 1_400_000
                    && failure
                        .meta
                        .logs
                        .iter()
                        .any(|line| line.contains("exceeded CUs meter")) =>
            {
                assert!(count == 4096);
                exhausted = true;
                samples.push(failure.meta.compute_units_consumed);
            }
            Err(failure) if expected_slot(count, descending, case).is_none() => {
                let expected = anchor_lang::prelude::ProgramError::from(
                    inference_router_program::RouterError::Unavailable,
                );
                assert!(format!("{:?}", failure.err).contains(&format!("{:?}", expected)));
                samples.push(failure.meta.compute_units_consumed);
            }
            Err(error) => panic!("case={case:?} count={count} descending={descending}: {error:?}"),
            Ok(result) => {
                assert!(
                    expected_slot(count, descending, case).is_some(),
                    "no eligible provider must reject"
                );
                let post = &result
                    .post_accounts
                    .iter()
                    .find(|(address, _)| *address == booking)
                    .unwrap()
                    .1;
                let booking = state_types::Booking::try_deserialize(&mut post.data()).unwrap();
                assert_eq!(
                    booking.slot.get() as usize,
                    expected_slot(count, descending, case).unwrap()
                );
                assert_eq!(
                    booking.cache_allowed,
                    u8::from(
                        cached
                            && expected_slot(count, descending, case)
                                == Some(cheapest_slot(count, descending, case))
                    )
                );
                samples.push(result.meta.compute_units_consumed);
            }
        }
    }
    assert!(
        samples.iter().all(|value| *value == samples[0]),
        "deterministic CU samples changed"
    );
    eprintln!(
        "CU_BENCH case={case:?} offers={count} order={} cu={} samples=3 status={} cache={cached}",
        if descending { "last" } else { "first" },
        samples[0],
        if exhausted { "exhausted" } else { "complete" }
    );
}

#[test]
#[ignore = "explicit SBF CU comparison"]
fn offer_filter_cu_benchmark() {
    let mut svm = LiteSVM::new();
    let mut budget = solana_compute_budget::compute_budget::ComputeBudget::new_with_defaults(true);
    budget.compute_unit_limit = 1_400_000;
    svm = svm.with_compute_budget(budget);
    svm.add_program_from_file(
        inference_router_program::ID,
        std::env::var_os("FOSSA_CU_SBF")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| program_path("inference_router_program.so")),
    )
    .unwrap();
    let payer = Keypair::new_from_array([71; 32]);
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    for count in [1, 3, 8, 32, 128, 512, 1024, 2048, 4096] {
        for descending in [false, true] {
            measure(&mut svm, &payer, count, descending, Case::Compatible);
        }
    }
    for count in [32, 512, 4096] {
        for case in [
            Case::Checkpoint,
            Case::InputModality,
            Case::OutputModality,
            Case::Context,
            Case::InputCap,
            Case::OutputCap,
            Case::NoMatch,
        ] {
            measure(&mut svm, &payer, count, false, case);
        }
    }
    for count in [3, 32, 512] {
        for descending in [false, true] {
            measure_inner(&mut svm, &payer, count, descending, Case::Compatible, true);
        }
    }
    for count in [3, 32, 512, 4096] {
        for descending in [false, true] {
            for case in [
                Case::UnlistedCheapest,
                Case::UnlistedPadded,
                Case::OnlyExpensiveListed,
                Case::NoneListed,
            ] {
                measure(&mut svm, &payer, count, descending, case);
            }
        }
    }
    for count in [3, 32, 512, 4096] {
        for descending in [false, true] {
            for case in [
                Case::UnlistedSeveral,
                Case::PaddedExpensiveOnly,
                Case::PaddedNoneListed,
            ] {
                measure(&mut svm, &payer, count, descending, case);
            }
            measure_inner(
                &mut svm,
                &payer,
                count,
                descending,
                Case::UnlistedPadded,
                true,
            );
        }
        for cached in [false, true] {
            measure_inner(
                &mut svm,
                &payer,
                count,
                false,
                Case::ShuffledCompatible,
                cached,
            );
        }
    }
    eprintln!(
        "CU_LAYOUT terms_entry={} booking_account={}",
        std::mem::size_of::<state_types::TermsEntry>(),
        state_types::Booking::DISCRIMINATOR.len() + std::mem::size_of::<state_types::Booking>()
    );
}

#[test]
#[ignore = "explicit booking policy SBF CU comparison"]
fn wallet_booking_cu_benchmark() {
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
                "enabled_empty",
                "enabled_dense",
                "enabled_sparse",
                "skip_cheapest",
                "all_exhausted",
                "all_exhausted_sparse",
                "last_eligible",
                "last_eligible_sparse",
            ] {
                let (reserve, booking) =
                    fixture(&mut svm, &payer, count, descending, Case::Compatible, false);
                let policies = reserve.accounts[9].pubkey;
                let wallet = reserve.accounts[10].pubkey;
                let mut policy_account = svm.get_account(&policies).unwrap();
                policy_account.data[40..44].copy_from_slice(&(count as u32).to_le_bytes());
                let policy_entries: &mut [state_types::BookingPolicy] =
                    anchor_lang::bytemuck::cast_slice_mut(&mut policy_account.data[52..]);
                for entry in policy_entries {
                    entry.wallet_bookings_per_window = 10.into();
                    entry.wallet_booking_capacity = 10.into();
                }
                let offset = if scenario.ends_with("sparse") {
                    count
                } else {
                    0
                };
                if offset != 0 {
                    let entries =
                        anchor_lang::bytemuck::cast_slice::<_, state_types::BookingPolicy>(
                            &policy_account.data[52..],
                        )
                        .to_vec();
                    let mut all_entries = entries.clone();
                    for (index, entry) in all_entries.iter_mut().enumerate() {
                        let mut bytes = [99; 32];
                        bytes[..4].copy_from_slice(&(index as u32).to_le_bytes());
                        entry.provider = Address::new_from_array(bytes);
                    }
                    all_entries.extend(entries);
                    let header = state_types::BookingPolicies::try_deserialize(
                        &mut &policy_account.data[..],
                    )
                    .unwrap();
                    let mut header = header;
                    header.limited_providers = (all_entries.len() as u32).into();
                    store_slab(
                        &mut svm,
                        policies,
                        &header,
                        &all_entries,
                        all_entries.len() as u32,
                    );
                    let terms = reserve.accounts[2].pubkey;
                    let mut account = svm.get_account(&terms).unwrap();
                    for (index, entry) in anchor_lang::bytemuck::cast_slice_mut::<
                        _,
                        state_types::TermsEntry,
                    >(&mut account.data[48..])
                    .iter_mut()
                    .enumerate()
                    {
                        entry.provider_policy = ((index + offset) as u32).into();
                    }
                    svm.set_account(terms, account).unwrap();
                } else {
                    svm.set_account(policies, policy_account).unwrap();
                }
                let mut header = state_types::WalletBookings::zeroed();
                header.market = reserve.accounts[0].pubkey;
                header.owner = payer.pubkey();
                header.rent_payer = payer.pubkey();
                header.bump = Address::find_program_address(
                    &[
                        b"wallet_bookings",
                        header.market.as_ref(),
                        header.owner.as_ref(),
                    ],
                    &inference_router_program::ID,
                )
                .1;
                let cheapest = if descending { count - 1 } else { 0 };
                let entries: Vec<_> = (0..if scenario == "enabled_empty" {
                    0
                } else {
                    count
                })
                    .map(|index| {
                        let exhausted = scenario.starts_with("all_exhausted")
                            || scenario.starts_with("last_eligible") && index + 1 < count
                            || scenario == "skip_cheapest" && index == cheapest;
                        state_types::WalletBookingEntry {
                            provider_policy: ((index + offset) as u32).into(),
                            window_end: i64::MAX.into(),
                            bookings: (if exhausted { 10 } else { 1 }).into(),
                            outstanding: 0.into(),
                        }
                    })
                    .collect();
                store_slab(
                    &mut svm,
                    wallet,
                    &header,
                    &entries,
                    if entries.is_empty() { 8 } else { count as u32 },
                );
                let transaction = Transaction::new(
                    &[&payer],
                    Message::new(&[reserve], Some(&payer.pubkey())),
                    svm.latest_blockhash(),
                );
                let mut samples = vec![];
                let mut loaded = 0;
                let mut exhausted_cu = false;
                for _ in 0..3 {
                    match svm.simulate_transaction(transaction.clone()) {
                        Ok(result) => {
                            assert!(!scenario.starts_with("all_exhausted"));
                            let post = &result
                                .post_accounts
                                .iter()
                                .find(|(address, _)| *address == booking)
                                .unwrap()
                                .1;
                            let booked =
                                state_types::Booking::try_deserialize(&mut post.data()).unwrap();
                            assert_eq!(
                                booked.slot.get() as usize,
                                if scenario.starts_with("last_eligible") {
                                    count - 1
                                } else if scenario == "skip_cheapest" {
                                    if descending {
                                        count - 2
                                    } else {
                                        1
                                    }
                                } else {
                                    cheapest
                                }
                            );
                            loaded = result.meta.loaded_accounts_data_size;
                            samples.push(result.meta.compute_units_consumed);
                        }
                        Err(error) => {
                            exhausted_cu = error.meta.compute_units_consumed == 1_400_000;
                            assert!(
                                exhausted_cu || scenario.starts_with("all_exhausted"),
                                "{scenario}: {error:?}"
                            );
                            if !exhausted_cu {
                                assert!(format!("{:?}", error.err).contains("Custom(6005)"));
                            }
                            loaded = error.meta.loaded_accounts_data_size;
                            samples.push(error.meta.compute_units_consumed);
                        }
                    }
                }
                assert!(samples.iter().all(|sample| *sample == samples[0]));
                eprintln!("WALLET_CU scenario={scenario} offers={count} order={} cu={} samples=3 loaded_bytes={loaded} status={}", if descending {"last"} else {"first"}, samples[0], if exhausted_cu {"exhausted"} else {"complete"});
            }
        }
    }
}
