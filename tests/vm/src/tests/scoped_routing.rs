use super::*;

struct ScopedFixture {
    f: FilterFixture,
    routing: Address,
    terms: Address,
    capacity: Address,
    wallet: Address,
    policies: [Address; 2],
    budgets: [Address; 2],
}

fn pda(seeds: &[&[u8]]) -> Address {
    Address::find_program_address(seeds, &inference_router_program::ID).0
}

impl ScopedFixture {
    fn new() -> Self {
        Self::from_legacy(FilterFixture::new())
    }

    fn from_legacy(mut f: FilterFixture) -> Self {
        let routing = pda(&[b"list_routing", f.list.as_ref()]);
        let terms = pda(&[b"list_terms", f.list.as_ref(), f.model.as_ref()]);
        let capacity = pda(&[b"list_capacity", f.list.as_ref(), f.model.as_ref()]);
        let wallet = pda(&[
            b"provider_wallet",
            f.market.as_ref(),
            f.payer.pubkey().as_ref(),
        ]);
        let policies = f.providers.each_ref().map(|provider| {
            pda(&[
                b"provider_policy",
                f.market.as_ref(),
                provider.pubkey().as_ref(),
            ])
        });
        let budgets = f.providers.each_ref().map(|provider| {
            pda(&[
                b"provider_capacity",
                f.model.as_ref(),
                provider.pubkey().as_ref(),
            ])
        });
        let system_program = solana_sdk_ids::system_program::ID;
        let activate = instruction(
            inference_router_program::ID,
            accounts::ActivateListRouting {
                market: f.market,
                council: f.payer.pubkey(),
            },
            ix::ActivateListRouting {},
        );
        send(&mut f.svm, &[activate], &f.payer, &[]).unwrap();
        for index in 0..2 {
            let provider = f.providers[index].pubkey();
            let profile = pda(&[b"provider", f.market.as_ref(), provider.as_ref()]);
            let initialize = [
                instruction(
                    inference_router_program::ID,
                    accounts::InitializeProviderPolicy {
                        market: f.market,
                        provider,
                        payer: f.payer.pubkey(),
                        profile,
                        policy: policies[index],
                        system_program,
                    },
                    ix::InitializeProviderPolicy {
                        limits: args::BookingLimits {
                            wallet_window_seconds: 60,
                            wallet_bookings_per_window: 0,
                            wallet_booking_capacity: 0,
                        },
                    },
                ),
                instruction(
                    inference_router_program::ID,
                    accounts::InitializeModelCapacity {
                        market: f.market,
                        model: f.model,
                        provider,
                        payer: f.payer.pubkey(),
                        profile,
                        legacy_terms: f.terms,
                        legacy_capacity: f.capacity,
                        budget: budgets[index],
                        system_program,
                    },
                    ix::InitializeModelCapacity { total: 20 },
                ),
            ];
            send(&mut f.svm, &initialize, &f.payer, &[&f.providers[index]]).unwrap();
        }
        let initialize = [
            instruction(
                inference_router_program::ID,
                accounts::InitializeListRouting {
                    market: f.market,
                    list: f.list,
                    authority: f.payer.pubkey(),
                    payer: f.payer.pubkey(),
                    routing,
                    system_program,
                },
                ix::InitializeListRouting { limit: 2 },
            ),
            instruction(
                inference_router_program::ID,
                accounts::InitializeListBooks {
                    market: f.market,
                    model: f.model,
                    list: f.list,
                    authority: f.payer.pubkey(),
                    routing,
                    payer: f.payer.pubkey(),
                    terms,
                    capacity,
                    system_program,
                },
                ix::InitializeListBooks { limit: 2 },
            ),
            instruction(
                inference_router_program::ID,
                accounts::InitializeProviderWallet {
                    market: f.market,
                    owner: f.payer.pubkey(),
                    payer: f.payer.pubkey(),
                    wallet,
                    system_program,
                },
                ix::InitializeProviderWallet { limit: 8 },
            ),
        ];
        send(&mut f.svm, &initialize, &f.payer, &[]).unwrap();
        let mut scoped = Self {
            f,
            routing,
            terms,
            capacity,
            wallet,
            policies,
            budgets,
        };
        for index in 0..2 {
            let enable = scoped.enable(index);
            scoped.provider_send(enable, index).unwrap();
            let set = scoped.set(index, offer(index as u32));
            scoped.provider_send(set, index).unwrap();
        }
        scoped
    }

    fn provider_send(
        &mut self,
        ix: solana_instruction::Instruction,
        provider: usize,
    ) -> TransactionResult {
        send(
            &mut self.f.svm,
            &[ix],
            &self.f.payer,
            &[&self.f.providers[provider]],
        )
    }

    fn send(&mut self, ix: solana_instruction::Instruction) -> TransactionResult {
        send(&mut self.f.svm, &[ix], &self.f.payer, &[])
    }

    fn enable(&self, index: usize) -> solana_instruction::Instruction {
        instruction(
            inference_router_program::ID,
            accounts::EnablePolicyMirror {
                provider: self.f.providers[index].pubkey(),
                policy: self.policies[index],
                list: self.f.list,
                routing: self.routing,
            },
            ix::EnablePolicyMirror {},
        )
    }

    fn freeze(&self, index: usize) -> solana_instruction::Instruction {
        instruction(
            inference_router_program::ID,
            accounts::FreezePolicyMirror {
                signer: self.f.providers[index].pubkey(),
                policy: self.policies[index],
                list: self.f.list,
                routing: self.routing,
            },
            ix::FreezePolicyMirror {},
        )
    }

    fn set(&self, index: usize, offer: args::Offer) -> solana_instruction::Instruction {
        instruction(
            inference_router_program::ID,
            accounts::SetListOffer {
                market: self.f.market,
                model: self.f.model,
                provider: self.f.providers[index].pubkey(),
                budget: self.budgets[index],
                list: self.f.list,
                routing: self.routing,
                terms: self.terms,
                capacity: self.capacity,
                payout: self.f.payouts[index],
            },
            ix::SetListOffer { offer },
        )
    }

    fn reserve(&self, generation: u64) -> solana_instruction::Instruction {
        instruction(
            inference_router_program::ID,
            accounts::ReserveListBooking {
                market: self.f.market,
                model: self.f.model,
                terms: self.terms,
                capacity: self.capacity,
                user: self.f.user,
                owner: self.f.payer.pubkey(),
                booking: self.f.booking,
                rent_payer: self.f.payer.pubkey(),
                list: self.f.list,
                routing: self.routing,
                wallet: self.wallet,
                group: None,
            },
            ix::ReserveListBooking {
                minimum_wallet_capacity: None,
                request: self.f.request(),
                generation: Some(generation),
            },
        )
    }

    fn reserve_next(&mut self, generation: u64) {
        let ix = self.reserve(generation);
        self.send(ix).unwrap();
    }

    fn release(&self, provider: usize, generation: u64) -> solana_instruction::Instruction {
        instruction(
            inference_router_program::ID,
            accounts::ReleaseListBooking {
                base: accounts::ReleaseBooking {
                    market: self.f.market,
                    model: self.f.model,
                    capacity: self.capacity,
                    user: self.f.user,
                    booking: self.f.booking,
                    provider: self.f.providers[provider].pubkey(),
                },
                wallet_bookings: self.wallet,
            },
            ix::ReleaseListBooking {
                generation: Some(generation),
            },
        )
    }

    fn release_next(&mut self, provider: usize, generation: u64) {
        let ix = self.release(provider, generation);
        self.provider_send(ix, provider).unwrap();
    }

    fn policy(&mut self, index: usize, seconds: u32, rate: u32, cap: u32) {
        let provider = self.f.providers[index].pubkey();
        let policy = self.policies[index];
        let updates = [
            instruction(
                inference_router_program::ID,
                accounts::ConfigureProviderPolicy { policy, provider },
                ix::BeginProviderPolicyUpdate {},
            ),
            self.freeze(index),
            instruction(
                inference_router_program::ID,
                accounts::ConfigureProviderPolicy { policy, provider },
                ix::CommitProviderPolicyUpdate {
                    limits: args::BookingLimits {
                        wallet_window_seconds: seconds,
                        wallet_bookings_per_window: rate,
                        wallet_booking_capacity: cap,
                    },
                },
            ),
            self.enable(index),
        ];
        send(
            &mut self.f.svm,
            &updates,
            &self.f.payer,
            &[&self.f.providers[index]],
        )
        .unwrap();
    }

    fn usage(&self, index: usize) -> state_types::ProviderUsage {
        self.f
            .entries::<state_types::ProviderUsage>(self.wallet, 112)
            .into_iter()
            .find(|entry| entry.provider == self.f.providers[index].pubkey())
            .unwrap()
    }

    fn membership(&self, index: usize, enabled: bool) -> solana_instruction::Instruction {
        instruction(
            inference_router_program::ID,
            accounts::UpdateRoutingList {
                list: self.f.list,
                authority: self.f.payer.pubkey(),
                routing: self.routing,
                policy: Some(self.policies[index]),
            },
            ix::UpdateRoutingList {
                provider: self.f.providers[index].pubkey(),
                enabled,
            },
        )
    }
}

#[test]
fn scoped_quota_history_fallback_and_owner_lanes() {
    let mut s = ScopedFixture::new();
    s.f.time(1_000);
    s.reserve_next(1);
    assert_eq!(s.f.booked().wallet_booking_counted, 2);
    s.policy(0, 1, 1, 1);
    s.f.new_lane();
    s.reserve_next(1);
    assert_eq!(s.f.booked().provider, s.f.providers[1].pubkey());
    assert_eq!(s.usage(0).outstanding.get(), 1);
    assert_eq!(s.usage(1).outstanding.get(), 1);
    s.release_next(1, 1);
    s.policy(0, 1, 1, 2);
    s.f.time(1_060);
    s.reserve_next(2);
    assert_eq!(s.f.booked().provider, s.f.providers[0].pubkey());
    assert_eq!(s.usage(0).window_end.get(), 1_061);
    s.release_next(0, 2);
    assert_eq!(s.usage(0).bookings.get(), 1);
    s.reserve_next(3);
    assert_eq!(s.f.booked().provider, s.f.providers[1].pubkey());
}

#[test]
fn scoped_removal_freezes_immediately_and_recovers_allocations() {
    let mut s = ScopedFixture::new();
    s.reserve_next(1);
    let remove = s.membership(0, false);
    s.send(remove).unwrap();
    assert_eq!(
        account::<state_types::ProviderPolicy>(&s.f.svm, s.policies[0])
            .active_mirrors
            .get(),
        0
    );
    let mut zero = offer(0);
    zero.capacity = 0;
    let set = s.set(0, zero);
    assert!(s.provider_send(set, 0).is_err());
    s.release_next(0, 1);
    let set = s.set(0, zero);
    s.provider_send(set, 0).unwrap();
    assert_eq!(
        account::<state_types::ProviderModelCapacity>(&s.f.svm, s.budgets[0])
            .allocated
            .get(),
        0
    );
    let add = s.membership(0, true);
    s.send(add).unwrap();
    let set = s.set(0, offer(0));
    assert!(s.provider_send(set, 0).is_err());
    s.reserve_next(2);
    assert_eq!(s.f.booked().provider, s.f.providers[1].pubkey());
    s.release_next(1, 2);
    let enable = s.enable(0);
    s.provider_send(enable, 0).unwrap();
    let set = s.set(0, offer(0));
    s.provider_send(set, 0).unwrap();
    s.reserve_next(3);
    assert_eq!(s.f.booked().provider, s.f.providers[0].pubkey());
}

#[test]
fn scoped_activation_blocks_old_reservations_but_preserves_settlement() {
    let mut legacy = FilterFixture::new();
    if let Ok(path) = std::env::var("FOSSA_LEGACY_SBF") {
        legacy
            .svm
            .add_program_from_file(inference_router_program::ID, path)
            .unwrap();
    }
    legacy.reserve_next(1);
    legacy
        .svm
        .add_program_from_file(
            inference_router_program::ID,
            program_path("inference_router_program.so"),
        )
        .unwrap();
    let mut s = ScopedFixture::from_legacy(legacy);
    assert_eq!(
        account::<state_types::ProviderModelCapacity>(&s.f.svm, s.budgets[0])
            .legacy_active
            .get(),
        1
    );
    let old_release = s.f.release(0, 1);
    s.provider_send(old_release, 0).unwrap();
    let old_reserve = s.f.reserve(s.f.request(), 2);
    assert!(s.send(old_reserve).is_err());
    let sync = instruction(
        inference_router_program::ID,
        accounts::SyncLegacyCapacity {
            budget: s.budgets[0],
            model: s.f.model,
            legacy_capacity: s.f.capacity,
        },
        ix::SyncLegacyCapacity {},
    );
    s.send(sync.clone()).unwrap();
    s.send(sync).unwrap();
    assert_eq!(
        account::<state_types::ProviderModelCapacity>(&s.f.svm, s.budgets[0])
            .legacy_active
            .get(),
        0
    );
    s.reserve_next(2);
    let activate = instruction(
        inference_router_program::ID,
        accounts::ActivateListRouting {
            market: s.f.market,
            council: s.f.payer.pubkey(),
        },
        ix::ActivateListRouting {},
    );
    assert!(s.send(activate).is_err());
}

#[test]
fn scoped_policy_update_barrier_and_foreign_mirrors() {
    let mut s = ScopedFixture::new();
    let policy = s.policies[0];
    let provider = s.f.providers[0].pubkey();
    let begin = instruction(
        inference_router_program::ID,
        accounts::ConfigureProviderPolicy { policy, provider },
        ix::BeginProviderPolicyUpdate {},
    );
    s.provider_send(begin, 0).unwrap();
    let commit = instruction(
        inference_router_program::ID,
        accounts::ConfigureProviderPolicy { policy, provider },
        ix::CommitProviderPolicyUpdate {
            limits: args::BookingLimits {
                wallet_window_seconds: 60,
                wallet_bookings_per_window: 1,
                wallet_booking_capacity: 1,
            },
        },
    );
    assert!(s.provider_send(commit.clone(), 0).is_err());
    let freeze = s.freeze(0);
    s.provider_send(freeze.clone(), 0).unwrap();
    assert!(s.provider_send(freeze, 0).is_err());
    let enable = s.enable(0);
    assert!(s.provider_send(enable, 0).is_err());
    s.reserve_next(1);
    assert_eq!(s.f.booked().provider, s.f.providers[1].pubkey());
    s.release_next(1, 1);
    s.provider_send(commit, 0).unwrap();
    let mut foreign = s.enable(0);
    foreign.accounts[0].pubkey = s.f.providers[1].pubkey();
    assert!(s.provider_send(foreign, 1).is_err());
    let enable = s.enable(0);
    s.provider_send(enable, 0).unwrap();
    s.reserve_next(2);
    assert_eq!(s.f.booked().provider, s.f.providers[0].pubkey());
}

#[test]
fn scoped_claim_expiry_and_invalid_account_bindings() {
    let mut s = ScopedFixture::new();
    for (index, replacement) in [
        (2, s.f.terms),
        (3, s.f.capacity),
        (9, Address::new_unique()),
        (10, Address::new_unique()),
    ] {
        let mut reserve = s.reserve(1);
        reserve.accounts[index].pubkey = replacement;
        assert!(s.send(reserve).is_err());
    }
    s.reserve_next(1);
    let booked = s.f.booked();
    let mut claim = instruction(
        inference_router_program::ID,
        accounts::ClaimListBooking {
            base: accounts::ClaimBooking {
                market: s.f.market,
                model: s.f.model,
                capacity: s.capacity,
                user: s.f.user,
                booking: s.f.booking,
                provider: s.f.providers[0].pubkey(),
                vault: s.f.vault,
                payout: s.f.payouts[0],
                mint: s.f.mint,
                authority: s.f.authority,
                token: anchor_spl::token::ID,
            },
            wallet_bookings: s.wallet,
        },
        ix::ClaimListBooking {
            claim: args::Claim {
                uncached: 100,
                cached: 0,
                output: 100,
                receipt: Some(args::ReceiptInput {
                    owner: s.f.payer.pubkey(),
                    user: s.f.user,
                    booking: s.f.booking,
                    generation: 1,
                    charge: 101,
                    claim_end: booked.claim_end.get(),
                }),
            },
        },
    );
    claim.accounts.extend(
        s.f.fee_destinations
            .map(|key| solana_instruction::AccountMeta::new(key, false)),
    );
    s.provider_send(claim.clone(), 0).unwrap();
    assert!(s.provider_send(claim, 0).is_err());
    assert_eq!(s.usage(0).outstanding.get(), 0);
    assert_eq!(token_account(&s.f.svm, s.f.payouts[0]).amount, 100);
    s.reserve_next(2);
    let expire = instruction(
        inference_router_program::ID,
        accounts::ExpireListBooking {
            base: accounts::ExpireBooking {
                market: s.f.market,
                model: s.f.model,
                capacity: s.capacity,
                user: s.f.user,
                booking: s.f.booking,
            },
            wallet_bookings: s.wallet,
        },
        ix::ExpireListBooking {
            generation: Some(2),
        },
    );
    assert!(s.send(expire.clone()).is_err());
    s.f.time(s.f.booked().claim_end.get() + 1);
    s.send(expire.clone()).unwrap();
    assert!(s.send(expire).is_err());
    assert_eq!(s.usage(0).outstanding.get(), 0);
}

impl ScopedFixture {
    fn scope(&self) -> [Address; 4] {
        [self.f.list, self.routing, self.terms, self.capacity]
    }
    fn use_scope(&mut self, scope: [Address; 4]) {
        [self.f.list, self.routing, self.terms, self.capacity] = scope;
    }

    fn another_list(&mut self) -> [Address; 4] {
        let original = self.scope();
        let list_key = Keypair::new();
        let payment =
            litesvm_token::CreateAccount::new(&mut self.f.svm, &self.f.payer, &self.f.mint)
                .send()
                .unwrap();
        mint_to(&mut self.f.svm, &self.f.payer, self.f.mint, payment, 1);
        let create = list(
            self.f.market,
            self.f.payer.pubkey(),
            list_key.pubkey(),
            10,
            Some([payment, self.f.fee_destinations[0], self.f.mint]),
        );
        send(&mut self.f.svm, &[create], &self.f.payer, &[&list_key]).unwrap();
        self.f.list = list_key.pubkey();
        self.routing = pda(&[b"list_routing", self.f.list.as_ref()]);
        self.terms = pda(&[b"list_terms", self.f.list.as_ref(), self.f.model.as_ref()]);
        self.capacity = pda(&[
            b"list_capacity",
            self.f.list.as_ref(),
            self.f.model.as_ref(),
        ]);
        let system_program = solana_sdk_ids::system_program::ID;
        let setup = [
            instruction(
                inference_router_program::ID,
                accounts::InitializeListRouting {
                    market: self.f.market,
                    list: self.f.list,
                    authority: self.f.payer.pubkey(),
                    payer: self.f.payer.pubkey(),
                    routing: self.routing,
                    system_program,
                },
                ix::InitializeListRouting { limit: 2 },
            ),
            instruction(
                inference_router_program::ID,
                accounts::InitializeListBooks {
                    market: self.f.market,
                    model: self.f.model,
                    list: self.f.list,
                    authority: self.f.payer.pubkey(),
                    routing: self.routing,
                    payer: self.f.payer.pubkey(),
                    terms: self.terms,
                    capacity: self.capacity,
                    system_program,
                },
                ix::InitializeListBooks { limit: 2 },
            ),
        ];
        send(&mut self.f.svm, &setup, &self.f.payer, &[]).unwrap();
        for index in 0..2 {
            let prior = account::<state_types::ProviderPolicy>(&self.f.svm, self.policies[index])
                .active_mirrors;
            let add = self.membership(index, true);
            self.send(add).unwrap();
            assert_eq!(
                account::<state_types::ProviderPolicy>(&self.f.svm, self.policies[index])
                    .active_mirrors,
                prior
            );
            let enable = self.enable(index);
            let mut forged = enable.clone();
            forged.accounts[0].pubkey = self.f.payer.pubkey();
            assert!(self.send(forged).is_err());
            self.provider_send(enable, index).unwrap();
            let set = self.set(index, offer(index as u32));
            self.provider_send(set, index).unwrap();
        }
        original
    }
}

#[test]
fn scoped_cross_list_capacity_quota_and_policy_barrier() {
    let mut s = ScopedFixture::new();
    s.policy(0, 60, 2, 1);
    s.f.time(1000);
    let first = s.another_list();
    let second = s.scope();
    let mut bigger = offer(0);
    bigger.capacity = 11;
    let set = s.set(0, bigger);
    assert!(s.provider_send(set, 0).is_err());
    assert_eq!(
        account::<state_types::ProviderModelCapacity>(&s.f.svm, s.budgets[0])
            .allocated
            .get(),
        20
    );
    s.use_scope(first);
    s.reserve_next(1);
    let original_release = s.release(0, 1);
    let original_user = s.f.user;
    let original_booking = s.f.booking;
    s.f.new_lane();
    s.use_scope(second);
    s.reserve_next(1);
    assert_eq!(s.f.booked().provider, s.f.providers[1].pubkey());
    s.release_next(1, 1);
    s.provider_send(original_release, 0).unwrap();
    s.reserve_next(2);
    assert_eq!(s.f.booked().provider, s.f.providers[0].pubkey());
    s.release_next(0, 2);
    s.use_scope(first);
    s.reserve_next(3);
    assert_eq!(s.f.booked().provider, s.f.providers[1].pubkey());
    s.release_next(1, 3);
    let policy = s.policies[0];
    let provider = s.f.providers[0].pubkey();
    let begin = instruction(
        inference_router_program::ID,
        accounts::ConfigureProviderPolicy { policy, provider },
        ix::BeginProviderPolicyUpdate {},
    );
    s.provider_send(begin, 0).unwrap();
    let freeze = s.freeze(0);
    s.provider_send(freeze, 0).unwrap();
    let commit = instruction(
        inference_router_program::ID,
        accounts::ConfigureProviderPolicy { policy, provider },
        ix::CommitProviderPolicyUpdate {
            limits: args::BookingLimits {
                wallet_window_seconds: 1,
                wallet_bookings_per_window: 1,
                wallet_booking_capacity: 1,
            },
        },
    );
    assert!(s.provider_send(commit.clone(), 0).is_err());
    s.use_scope(second);
    let remove = s.membership(0, false);
    s.send(remove).unwrap();
    s.provider_send(commit, 0).unwrap();
    let prune = instruction(
        inference_router_program::ID,
        accounts::PruneListOffer {
            list: s.f.list,
            terms: s.terms,
            capacity: s.capacity,
            budget: s.budgets[0],
        },
        ix::PruneListOffer {},
    );
    s.send(prune.clone()).unwrap();
    assert!(s.send(prune).is_err());
    assert_eq!(
        account::<state_types::ProviderModelCapacity>(&s.f.svm, s.budgets[0])
            .allocated
            .get(),
        10
    );
    assert_eq!(s.usage(0).bookings.get(), 2);
    assert_eq!(s.usage(0).window_end.get(), 1060);
    assert_eq!(
        account::<state_types::User>(&s.f.svm, original_user)
            .free
            .get(),
        100_000
    );
    assert_eq!(
        account::<state_types::Booking>(&s.f.svm, original_booking).status,
        5
    );
}

#[test]
fn scoped_churn_preserves_ties_pending_capacity_and_wallet_hints() {
    let mut s = ScopedFixture::new();
    let larger = usize::from(s.f.providers[0].pubkey() < s.f.providers[1].pubkey());
    let smaller = 1 - larger;
    for index in 0..2 {
        let mut zero = offer(index as u32);
        zero.capacity = 0;
        let set = s.set(index, zero);
        s.provider_send(set, index).unwrap();
    }
    let same = offer(0);
    let set = s.set(larger, same);
    s.provider_send(set, larger).unwrap();
    s.reserve_next(1);
    assert_eq!(s.f.booked().slot.get(), 0);
    let pending = s.release(larger, 1);
    let set = s.set(smaller, same);
    s.provider_send(set, smaller).unwrap();
    let freeze = s.freeze(larger);
    s.provider_send(freeze, larger).unwrap();
    s.f.new_lane();
    s.reserve_next(1);
    assert_eq!(s.f.booked().provider, s.f.providers[smaller].pubkey());
    s.provider_send(pending, larger).unwrap();
    assert_eq!(s.usage(smaller).outstanding.get(), 1);
    assert_eq!(s.usage(larger).outstanding.get(), 0);
    s.release_next(smaller, 1);
    let enable = s.enable(larger);
    s.provider_send(enable, larger).unwrap();
    s.reserve_next(2);
    assert_eq!(s.f.booked().provider, s.f.providers[larger].pubkey());
    s.release_next(larger, 2);
    let mut request = s.f.request();
    request.estimated_input = 0;
    request.estimated_output = 0;
    let mut reserve = s.reserve(3);
    reserve.data = ix::ReserveListBooking {
        minimum_wallet_capacity: None,
        request,
        generation: Some(3),
    }
    .data();
    s.send(reserve).unwrap();
    assert_eq!(s.f.booked().provider, s.f.providers[larger].pubkey());
    s.release_next(larger, 3);
    let freeze = s.freeze(larger);
    s.provider_send(freeze, larger).unwrap();
    s.reserve_next(4);
    assert_eq!(s.f.booked().provider, s.f.providers[smaller].pubkey());
    assert_eq!(s.usage(smaller).outstanding.get(), 1);
    assert_eq!(s.usage(larger).outstanding.get(), 0);
}

#[test]
fn scoped_wallet_history_cannot_be_pruned_or_closed_early() {
    let mut s = ScopedFixture::new();
    s.f.time(1000);
    s.policy(0, 60, 1, 1);
    s.reserve_next(1);
    s.release_next(0, 1);
    let close = instruction(
        inference_router_program::ID,
        accounts::CloseProviderWallet {
            owner: s.f.payer.pubkey(),
            wallet: s.wallet,
            recipient: s.f.payer.pubkey(),
        },
        ix::CloseProviderWallet {},
    );
    assert!(s.send(close.clone()).is_err());
    let prune = instruction(
        inference_router_program::ID,
        accounts::PruneProviderWallet {
            owner: s.f.payer.pubkey(),
            wallet: s.wallet,
        },
        ix::PruneProviderWallet {},
    );
    s.send(prune.clone()).unwrap();
    assert_eq!(s.usage(0).bookings.get(), 1);
    s.reserve_next(2);
    assert_eq!(s.f.booked().provider, s.f.providers[1].pubkey());
    s.release_next(1, 2);
    s.f.time(1060);
    s.send(prune).unwrap();
    s.send(close).unwrap();
    let init = instruction(
        inference_router_program::ID,
        accounts::InitializeProviderWallet {
            market: s.f.market,
            owner: s.f.payer.pubkey(),
            payer: s.f.payer.pubkey(),
            wallet: s.wallet,
            system_program: solana_sdk_ids::system_program::ID,
        },
        ix::InitializeProviderWallet { limit: 8 },
    );
    s.send(init).unwrap();
    s.reserve_next(3);
    assert_eq!(s.f.booked().provider, s.f.providers[0].pubkey());
}

#[test]
fn scoped_full_legacy_book_cannot_block_new_admitted_provider() {
    use crate::tests::offer_filter_cu::store_slab;
    let mut f = FilterFixture::new();
    let old = f.terms()[0];
    let entries: Vec<_> = (0..4096u32)
        .map(|index| {
            let mut entry = old;
            let mut key = [0; 32];
            key[28..].copy_from_slice(&index.to_be_bytes());
            entry.provider = Address::new_from_array(key);
            entry
        })
        .collect();
    let mut model: state_types::Model = account(&f.svm, f.model);
    model.limit = 4096.into();
    store_account(&mut f.svm, f.model, &model);
    let mut terms: state_types::FilteredTerms = account(&f.svm, f.terms);
    terms.limit = 4096.into();
    store_slab(&mut f.svm, f.terms, &terms, &entries, 4096);
    let mut capacity: state_types::Capacity = account(&f.svm, f.capacity);
    capacity.limit = 4096.into();
    let capacities = vec![anchor_lang::bytemuck::Zeroable::zeroed(); 4096];
    store_slab::<_, state_types::CapacityEntry>(
        &mut f.svm,
        f.capacity,
        &capacity,
        &capacities,
        4096,
    );
    for provider in &f.providers {
        let profile = pda(&[b"provider", f.market.as_ref(), provider.pubkey().as_ref()]);
        let mut state: state_types::Provider = account(&f.svm, profile);
        state.policy_registered = 0;
        store_account(&mut f.svm, profile, &state);
    }
    let mut s = ScopedFixture::from_legacy(f);
    assert_eq!(
        account::<state_types::ProviderModelCapacity>(&s.f.svm, s.budgets[0])
            .legacy_slot
            .get(),
        u32::MAX
    );
    s.reserve_next(1);
    assert_eq!(s.f.booked().provider, s.f.providers[0].pubkey());
    let mut malicious = s.set(0, offer(0));
    malicious.accounts[2].pubkey = s.f.payer.pubkey();
    assert!(s.send(malicious).is_err());
    let old_register = instruction(
        inference_router_program::ID,
        accounts::RegisterOffer {
            market: s.f.market,
            model: s.f.model,
            terms: s.f.terms,
            capacity: s.f.capacity,
            profile: pda(&[
                b"provider",
                s.f.market.as_ref(),
                s.f.providers[0].pubkey().as_ref(),
            ]),
            provider: s.f.providers[0].pubkey(),
            payout: s.f.payouts[0],
        },
        ix::RegisterOffer { offer: offer(0) },
    );
    assert!(s.provider_send(old_register, 0).is_err());
}

#[test]
fn scoped_wallet_capacity_spans_models_and_rejects_foreign_settlement_books() {
    let mut legacy = FilterFixture::new();
    let first = [legacy.model, legacy.model_id, legacy.terms, legacy.capacity];
    legacy.new_model();
    let mut s = ScopedFixture::from_legacy(legacy);
    s.policy(0, 60, 10, 1);
    s.reserve_next(1);
    let second_release = s.release(0, 1);
    let second_capacity = s.capacity;
    s.f.new_lane();
    [s.f.model, s.f.model_id, s.f.terms, s.f.capacity] = first;
    s.terms = pda(&[b"list_terms", s.f.list.as_ref(), s.f.model.as_ref()]);
    s.capacity = pda(&[b"list_capacity", s.f.list.as_ref(), s.f.model.as_ref()]);
    let system_program = solana_sdk_ids::system_program::ID;
    let books = instruction(
        inference_router_program::ID,
        accounts::InitializeListBooks {
            market: s.f.market,
            model: s.f.model,
            list: s.f.list,
            authority: s.f.payer.pubkey(),
            routing: s.routing,
            payer: s.f.payer.pubkey(),
            terms: s.terms,
            capacity: s.capacity,
            system_program,
        },
        ix::InitializeListBooks { limit: 2 },
    );
    s.send(books).unwrap();
    for index in 0..2 {
        let provider = s.f.providers[index].pubkey();
        s.budgets[index] = pda(&[b"provider_capacity", s.f.model.as_ref(), provider.as_ref()]);
        let init = instruction(
            inference_router_program::ID,
            accounts::InitializeModelCapacity {
                market: s.f.market,
                model: s.f.model,
                provider,
                payer: s.f.payer.pubkey(),
                profile: pda(&[b"provider", s.f.market.as_ref(), provider.as_ref()]),
                legacy_terms: s.f.terms,
                legacy_capacity: s.f.capacity,
                budget: s.budgets[index],
                system_program,
            },
            ix::InitializeModelCapacity { total: 20 },
        );
        s.provider_send(init, index).unwrap();
        let set = s.set(index, offer(index as u32));
        s.provider_send(set, index).unwrap();
    }
    s.reserve_next(1);
    assert_eq!(s.f.booked().provider, s.f.providers[1].pubkey());
    let mut bad = s.release(1, 1);
    bad.accounts[2].pubkey = second_capacity;
    assert!(s.provider_send(bad, 1).is_err());
    assert_eq!(s.usage(1).outstanding.get(), 1);
    s.release_next(1, 1);
    s.provider_send(second_release, 0).unwrap();
    assert_eq!(s.usage(0).outstanding.get(), 0);
}

#[test]
fn scoped_legacy_zero_marker_drains_without_wallet_account() {
    let mut f = FilterFixture::new();
    f.reserve_next(1);
    let mut booked = f.svm.get_account(&f.booking).unwrap();
    booked.data[356] = 0;
    booked.data[140..144].fill(0);
    f.svm.set_account(f.booking, booked).unwrap();
    let mut s = ScopedFixture::from_legacy(f);
    let release = s.f.release(0, 1);
    s.provider_send(release, 0).unwrap();
    assert_eq!(s.f.active(), [0, 0]);
    assert_eq!(s.f.free(), 100_000);
}

#[test]
fn scoped_identity_initialization_and_transaction_rollback_are_enforced() {
    let mut s = ScopedFixture::new();
    let other_owner = s.f.providers[1].pubkey();
    let other_wallet = pda(&[
        b"provider_wallet",
        s.f.market.as_ref(),
        other_owner.as_ref(),
    ]);
    let init = instruction(
        inference_router_program::ID,
        accounts::InitializeProviderWallet {
            market: s.f.market,
            owner: other_owner,
            payer: s.f.payer.pubkey(),
            wallet: other_wallet,
            system_program: solana_sdk_ids::system_program::ID,
        },
        ix::InitializeProviderWallet { limit: 8 },
    );
    s.provider_send(init, 1).unwrap();
    let mut reserve = s.reserve(1);
    reserve.accounts[10].pubkey = other_wallet;
    assert!(s.send(reserve).is_err());
    let alias = Keypair::new();
    let invalid = instruction(
        inference_router_program::ID,
        accounts::InitializeProviderWallet {
            market: s.f.market,
            owner: s.f.payer.pubkey(),
            payer: s.f.payer.pubkey(),
            wallet: alias.pubkey(),
            system_program: solana_sdk_ids::system_program::ID,
        },
        ix::InitializeProviderWallet { limit: 8 },
    );
    assert!(s.send(invalid).is_err());
    let mut foreign = s.set(0, offer(0));
    foreign.accounts[3].pubkey = s.budgets[1];
    assert!(s.provider_send(foreign, 0).is_err());
    let grow = instruction(
        inference_router_program::ID,
        accounts::GrowListBooks {
            list: s.f.list,
            authority: other_owner,
            terms: s.terms,
            capacity: s.capacity,
            payer: s.f.payer.pubkey(),
            system_program: solana_sdk_ids::system_program::ID,
        },
        ix::GrowListBooks { limit: 3 },
    );
    assert!(s.provider_send(grow, 1).is_err());
    let activate = instruction(
        inference_router_program::ID,
        accounts::ActivateListRouting {
            market: s.f.market,
            council: other_owner,
        },
        ix::ActivateListRouting {},
    );
    assert!(s.provider_send(activate, 1).is_err());
    let before = s.f.svm.get_account(&s.wallet).unwrap();
    let capacity = s.f.svm.get_account(&s.capacity).unwrap();
    let free = s.f.free();
    let reserve = s.reserve(1);
    let bad = s.release(0, 999);
    assert!(send(
        &mut s.f.svm,
        &[reserve, bad],
        &s.f.payer,
        &[&s.f.providers[0]]
    )
    .is_err());
    assert_eq!(s.f.svm.get_account(&s.wallet).unwrap().data, before.data);
    assert_eq!(
        s.f.svm.get_account(&s.capacity).unwrap().data,
        capacity.data
    );
    assert_eq!(s.f.free(), free);
    s.reserve_next(1);
    s.release_next(0, 1);
    let mut wallet = s.f.svm.get_account(&s.wallet).unwrap();
    let entries: &mut [state_types::ProviderUsage] =
        anchor_lang::bytemuck::cast_slice_mut(&mut wallet.data[112..]);
    let entry = entries
        .iter_mut()
        .find(|entry| entry.provider == s.f.providers[0].pubkey())
        .unwrap();
    entry.bookings = u32::MAX.into();
    s.f.svm.set_account(s.wallet, wallet).unwrap();
    s.reserve_next(2);
    assert_eq!(s.usage(0).bookings.get(), u32::MAX);
    s.release_next(0, 2);
    assert_eq!(s.usage(0).outstanding.get(), 0);
}

#[test]
fn scoped_book_closure_and_reenrollment_cannot_reset_wallet_rate_budget() {
    let mut s = ScopedFixture::new();
    s.f.time(1000);
    s.policy(0, 60, 1, 1);
    s.reserve_next(1);
    s.release_next(0, 1);
    for index in 0..2 {
        let remove = s.membership(index, false);
        s.send(remove).unwrap();
        let prune = instruction(
            inference_router_program::ID,
            accounts::PruneListOffer {
                list: s.f.list,
                terms: s.terms,
                capacity: s.capacity,
                budget: s.budgets[index],
            },
            ix::PruneListOffer {},
        );
        s.send(prune).unwrap();
    }
    let close_routing = instruction(
        inference_router_program::ID,
        accounts::CloseListRouting {
            list: s.f.list,
            authority: s.f.payer.pubkey(),
            routing: s.routing,
        },
        ix::CloseListRouting {},
    );
    assert!(s.send(close_routing.clone()).is_err());
    let close_books = instruction(
        inference_router_program::ID,
        accounts::CloseListBooks {
            list: s.f.list,
            authority: s.f.payer.pubkey(),
            routing: s.routing,
            terms: s.terms,
            capacity: s.capacity,
        },
        ix::CloseListBooks {},
    );
    s.send(close_books).unwrap();
    s.send(close_routing).unwrap();
    assert_eq!(
        account::<state_types::List>(&s.f.svm, s.f.list).reserved,
        [0; 4]
    );
    let system_program = solana_sdk_ids::system_program::ID;
    let initialize = [
        instruction(
            inference_router_program::ID,
            accounts::InitializeListRouting {
                market: s.f.market,
                list: s.f.list,
                authority: s.f.payer.pubkey(),
                payer: s.f.payer.pubkey(),
                routing: s.routing,
                system_program,
            },
            ix::InitializeListRouting { limit: 1 },
        ),
        instruction(
            inference_router_program::ID,
            accounts::InitializeListBooks {
                market: s.f.market,
                model: s.f.model,
                list: s.f.list,
                authority: s.f.payer.pubkey(),
                routing: s.routing,
                payer: s.f.payer.pubkey(),
                terms: s.terms,
                capacity: s.capacity,
                system_program,
            },
            ix::InitializeListBooks { limit: 1 },
        ),
        instruction(
            inference_router_program::ID,
            accounts::GrowListRouting {
                list: s.f.list,
                authority: s.f.payer.pubkey(),
                routing: s.routing,
                payer: s.f.payer.pubkey(),
                system_program,
            },
            ix::GrowListRouting { limit: 2 },
        ),
        instruction(
            inference_router_program::ID,
            accounts::GrowListBooks {
                list: s.f.list,
                authority: s.f.payer.pubkey(),
                terms: s.terms,
                capacity: s.capacity,
                payer: s.f.payer.pubkey(),
                system_program,
            },
            ix::GrowListBooks { limit: 2 },
        ),
        instruction(
            inference_router_program::ID,
            accounts::GrowProviderWallet {
                owner: s.f.payer.pubkey(),
                wallet: s.wallet,
                payer: s.f.payer.pubkey(),
                system_program,
            },
            ix::GrowProviderWallet { limit: 16 },
        ),
    ];
    send(&mut s.f.svm, &initialize, &s.f.payer, &[]).unwrap();
    for index in 0..2 {
        let add = s.membership(index, true);
        s.send(add).unwrap();
        let enable = s.enable(index);
        s.provider_send(enable, index).unwrap();
        let set = s.set(index, offer(index as u32));
        s.provider_send(set, index).unwrap();
    }
    s.reserve_next(2);
    assert_eq!(s.f.booked().provider, s.f.providers[1].pubkey());
    assert_eq!(s.usage(0).bookings.get(), 1);
}

#[test]
fn scoped_demand_filters_configured_capacity_without_reserving_a_batch() {
    let mut s = ScopedFixture::new();
    s.policy(0, 60, 0, 1);
    s.policy(1, 60, 0, 3);
    let demand = |s: &ScopedFixture, n| {
        let mut reserve = s.reserve(1);
        reserve.data = ix::ReserveListBooking {
            request: s.f.request(),
            generation: Some(1),
            minimum_wallet_capacity: Some(n),
        }
        .data();
        reserve
    };
    assert!(s.send(demand(&s, 0)).is_err());
    assert!(s.send(demand(&s, 11)).is_err());
    for lane in 0..3 {
        if lane > 0 {
            s.f.new_lane();
        }
        s.send(demand(&s, 3)).unwrap();
        assert_eq!(s.f.booked().provider, s.f.providers[1].pubkey());
    }
    s.f.new_lane();
    assert!(s.send(demand(&s, 3)).is_err());
    assert_eq!(s.usage(1).outstanding.get(), 3);
    s.policy(0, 60, 0, 0);
    let mut offer = offer(0);
    offer.capacity = 2;
    let set = s.set(0, offer);
    s.provider_send(set, 0).unwrap();
    assert!(s.send(demand(&s, 3)).is_err());
    s.send(demand(&s, 2)).unwrap();
    assert_eq!(s.f.booked().provider, s.f.providers[0].pubkey());
}

#[test]
fn scoped_empty_books_stage_before_activation_without_interrupting_legacy_routing() {
    let mut f = FilterFixture::new();
    let routing = pda(&[b"list_routing", f.list.as_ref()]);
    let terms = pda(&[b"list_terms", f.list.as_ref(), f.model.as_ref()]);
    let capacity = pda(&[b"list_capacity", f.list.as_ref(), f.model.as_ref()]);
    let system_program = solana_sdk_ids::system_program::ID;
    let stage = [
        instruction(
            inference_router_program::ID,
            accounts::InitializeListRouting {
                market: f.market,
                list: f.list,
                authority: f.payer.pubkey(),
                payer: f.payer.pubkey(),
                routing,
                system_program,
            },
            ix::InitializeListRouting { limit: 2 },
        ),
        instruction(
            inference_router_program::ID,
            accounts::InitializeListBooks {
                market: f.market,
                model: f.model,
                list: f.list,
                authority: f.payer.pubkey(),
                routing,
                payer: f.payer.pubkey(),
                terms,
                capacity,
                system_program,
            },
            ix::InitializeListBooks { limit: 2 },
        ),
    ];
    send(&mut f.svm, &stage, &f.payer, &[]).unwrap();
    assert_eq!(
        account::<state_types::Market>(&f.svm, f.market).reserved[0],
        0
    );
    f.reserve_next(1);
    let release = f.release(0, 1);
    send(&mut f.svm, &[release], &f.payer, &[&f.providers[0]]).unwrap();
    assert_eq!(f.free(), 100_000);
}
