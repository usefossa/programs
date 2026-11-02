use crate::{
    args::{valid_modalities, ReserveRequest},
    helpers::{fee, require},
    state_types::{
        CapacityEntry, LegacyTermsEntry as TermsEntry, MAX_ENTRIES, MODALITIES, OFFER_VERSION,
    },
    CLAIM_SECONDS,
};
use anchor_lang::prelude::{Address, ProgramError};

impl ReserveRequest {
    pub(crate) fn validate(&self, free: u64) -> Result<u64, ProgramError> {
        require(
            self.checkpoint != [0; 32]
                && valid_modalities(self.input_modalities)
                && valid_modalities(self.output_modalities)
                && (self.min_context == 0 || self.limits_configured)
                && self.max_input > 0
                && self.max_output > 0
                && self.max_input.checked_add(self.max_output).is_some()
                && self.estimated_input <= self.max_input
                && self.estimated_output <= self.max_output
                && self.estimated_cached <= self.estimated_input
                && (self.estimated_cached == 0
                    || self.cache_provider != anchor_lang::prelude::Address::default()),
            crate::RouterError::InvalidArgument,
        )?;
        Ok(free.min(self.spend))
    }
}
#[derive(Debug, PartialEq)]
pub(crate) struct Match {
    pub(crate) slot: u32,
    pub(crate) reserve: u64,
    pub(crate) score: u64,
    pub(crate) cache: bool,
}
pub(crate) fn select(
    offers: &[TermsEntry],
    capacities: &[CapacityEntry],
    listed_providers: &[Address],
    request: &ReserveRequest,
    spend: u64,
) -> Result<Match, ProgramError> {
    let offer_count = offers.len();
    require(
        offer_count <= MAX_ENTRIES && offer_count == capacities.len(),
        crate::RouterError::InvalidAccount,
    )?;
    let context_tokens = request
        .max_input
        .checked_add(request.max_output)
        .ok_or(crate::RouterError::InvalidArgument)?
        .max(request.min_context);
    require(
        !listed_providers.is_empty(),
        crate::RouterError::Unavailable,
    )?;
    // ceil(output / speed) must fit the claim window; compute required speed once.
    let min_speed = u32::from(request.min_speed)
        .max(request.max_output.div_ceil((86_400 - CLAIM_SECONDS) as u32));
    let cache_member = request.estimated_cached > 0
        && listed_providers
            .binary_search(&request.cache_provider)
            .is_ok();
    let mut best = Match {
        slot: 0,
        reserve: 0,
        score: u64::MAX,
        cache: false,
    };
    let mut candidates = [(0u64, 0u32); 16];
    // sparse lists reject unlisted offers before buffering candidates.
    let sparse_list = listed_providers.len() < offer_count / candidates.len();
    let mut batch_start = 0;
    // seed the incumbent immediately, then check bounded batches without rescanning the book.
    let mut batch_width = 1;
    while batch_start < offer_count {
        let batch_end = (batch_start + batch_width).min(offer_count);
        let mut candidate_count = 0;
        let mut candidate_index = 0;
        let mut lowest_score = u64::MAX;
        for slot in batch_start..batch_end {
            let offer = &offers[slot];
            let capacity_entry = &capacities[slot];
            let input = offer.input_rate.get();
            let cached_rate = offer.cached_rate.get();
            let output = offer.output_rate.get();
            if offer.active != 1
                || (request.private && offer.privacy != 1)
                || u32::from(offer.speed.get()) < min_speed
                || capacity_entry.active.get() >= offer.capacity.get()
                || input > request.input_rate_cap
                || cached_rate > request.cached_rate_cap
                || output > request.output_rate_cap
            {
                continue;
            }
            let cached = request.estimated_cached > 0 && offer.provider == request.cache_provider;
            let score = if cached {
                fee(request.estimated_input - request.estimated_cached, input)
                    + fee(request.estimated_cached, cached_rate)
            } else {
                fee(request.estimated_input, input)
            } + fee(request.estimated_output, output);
            // only compatible offers become best; non-improving scores cannot win, including ties.
            if score >= best.score
                || offer.version != OFFER_VERSION
                || offer.reserved != 0
                || offer.checkpoint != request.checkpoint
                || (offer.input_modalities | offer.output_modalities) & !MODALITIES != 0
                || offer.input_modalities & request.input_modalities != request.input_modalities
                || offer.output_modalities & request.output_modalities != request.output_modalities
                || context_tokens > offer.context_window.get()
                || request.max_input > offer.max_input.get()
                || request.max_output > offer.max_output.get()
            {
                continue;
            }
            if sparse_list
                && !(if cached {
                    cache_member
                } else {
                    listed_providers.binary_search(&offer.provider).is_ok()
                })
            {
                continue;
            }
            if score < lowest_score {
                lowest_score = score;
                candidate_index = candidate_count;
            }
            candidates[candidate_count] = (score, slot as u32);
            candidate_count += 1;
        }
        // failed batches resume immediate checks until a listed, affordable offer seeds the next batch.
        batch_width = if candidate_count > 0 {
            1
        } else {
            candidates.len()
        };
        while candidate_count > 0 {
            let (score, slot) = candidates[candidate_index];
            let offer = &offers[slot as usize];
            let cache = offer.provider == request.cache_provider;
            let listed = sparse_list
                || (if cache && request.estimated_cached > 0 {
                    cache_member
                } else {
                    listed_providers.binary_search(&offer.provider).is_ok()
                });
            if listed {
                let reserve = fee(request.max_input, offer.input_rate.get())
                    + fee(request.max_output, offer.output_rate.get())
                    + 1;
                if reserve <= spend {
                    // all remaining candidates are worse, including higher-slot ties.
                    best = Match {
                        slot,
                        reserve,
                        score,
                        cache,
                    };
                    batch_width = candidates.len();
                    break;
                }
            }
            candidate_count -= 1;
            candidates[candidate_index] = candidates[candidate_count];
            candidate_index = 0;
            for index in 1..candidate_count {
                if candidates[index] < candidates[candidate_index] {
                    candidate_index = index;
                }
            }
        }
        if best.score == 0 {
            break;
        }
        batch_start = batch_end;
    }
    require(best.score != u64::MAX, crate::RouterError::Unavailable)?;
    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_types::{MODALITY_AUDIO, MODALITY_IMAGE, MODALITY_TEXT, MODALITY_VIDEO};

    fn select(
        terms: &[TermsEntry],
        capacity: &[CapacityEntry],
        providers: &[Address],
        r: &ReserveRequest,
    ) -> Result<Match, ProgramError> {
        super::select(terms, capacity, providers, r, r.spend)
    }

    fn terms(provider: Address, input_rate: u32) -> TermsEntry {
        TermsEntry {
            provider,
            payout: Address::new_from_array([9; 32]),
            checkpoint: [7; 32],
            input_rate: input_rate.into(),
            cached_rate: input_rate.into(),
            output_rate: input_rate.into(),
            speed: 100.into(),
            privacy: 1,
            active: 1,
            input_modalities: MODALITY_TEXT | MODALITY_IMAGE,
            output_modalities: MODALITY_TEXT,
            version: OFFER_VERSION,
            reserved: 0,
            capacity: 2.into(),
            context_window: 200.into(),
            max_input: 100.into(),
            max_output: 100.into(),
        }
    }

    fn capacity(active: u32) -> CapacityEntry {
        CapacityEntry {
            reserved: [0; 16],
            active: active.into(),
            reserved2: [0; 12],
        }
    }

    fn request(cache_provider: Address) -> ReserveRequest {
        ReserveRequest {
            model_id: Address::new_from_array([3; 32]),
            input_rate_cap: 2_000_000,
            cached_rate_cap: 2_000_000,
            output_rate_cap: 2_000_000,
            min_speed: 1,
            max_input: 100,
            max_output: 100,
            private: false,
            spend: 1_000,
            estimated_input: 50,
            estimated_cached: 0,
            estimated_output: 50,
            cache_provider,
            checkpoint: [7; 32],
            input_modalities: MODALITY_TEXT,
            output_modalities: MODALITY_TEXT,
            min_context: 0,
            limits_configured: false,
        }
    }

    #[test]
    fn validated_spending_is_clamped_and_applied_without_copying_requirements() {
        let provider = Address::new_from_array([1; 32]);
        let r = request(provider);
        assert_eq!(r.validate(0).unwrap(), 0);
        assert_eq!(r.validate(500).unwrap(), 500);
        assert_eq!(r.validate(u64::MAX).unwrap(), r.spend);
        let offer = terms(provider, 1_000_000);
        let capacity = capacity(0);
        assert!(super::select(&[offer], &[capacity], &[provider], &r, 200).is_err());
        assert_eq!(
            super::select(&[offer], &[capacity], &[provider], &r, 201)
                .unwrap()
                .reserve,
            201
        );
    }

    #[test]
    fn selects_the_lowest_score_eligible_provider() {
        let first = Address::new_from_array([1; 32]);
        let second = Address::new_from_array([2; 32]);
        let terms = [terms(first, 1_000_000), terms(second, 500_000)];
        let capacity = [capacity(0), capacity(0)];
        let result = select(&terms, &capacity, &[first, second], &request(first)).unwrap();
        assert_eq!(result.slot, 1);
    }

    #[test]
    fn rejects_full_or_unlisted_providers() {
        let first = Address::new_from_array([1; 32]);
        let second = Address::new_from_array([2; 32]);
        let terms = [terms(first, 1_000_000), terms(second, 500_000)];
        let capacity = [capacity(2), capacity(0)];
        assert!(select(&terms, &capacity, &[first], &request(first)).is_err());
    }

    #[test]
    fn filters_serving_requirements_before_cheapest_selection() {
        let first = Address::new_from_array([1; 32]);
        let second = Address::new_from_array([2; 32]);
        let compatible = terms(second, 1_000_000);
        let cheaper = terms(first, 500_000);
        let changes: &[fn(&mut TermsEntry)] = &[
            |t| t.checkpoint = [8; 32],
            |t| t.checkpoint = [0; 32],
            |t| t.input_modalities = MODALITY_IMAGE,
            |t| t.output_modalities = MODALITY_AUDIO,
            |t| t.context_window = 199.into(),
            |t| t.max_input = 99.into(),
            |t| t.max_output = 99.into(),
            |t| t.version = 0,
            |t| t.version = OFFER_VERSION + 1,
            |t| t.reserved = 1,
            |t| t.input_modalities = 0,
            |t| t.output_modalities = 0,
            |t| t.input_modalities |= 128,
            |t| t.output_modalities |= 128,
        ];
        for change in changes {
            let mut incompatible = cheaper;
            change(&mut incompatible);
            let selected = select(
                &[incompatible, compatible],
                &[capacity(0), capacity(0)],
                &[first, second],
                &request(first),
            )
            .unwrap();
            assert_eq!(selected.slot, 1);
            assert!(select(&[incompatible], &[capacity(0)], &[first], &request(first)).is_err());
        }
    }

    #[test]
    fn modality_requirements_are_subsets_and_context_counts_full_input() {
        let provider = Address::new_from_array([1; 32]);
        let offer = terms(provider, 1_000_000);
        let mut r = request(provider);
        r.input_modalities = MODALITY_TEXT | MODALITY_IMAGE;
        r.estimated_cached = r.estimated_input;
        assert!(select(&[offer], &[capacity(0)], &[provider], &r).is_ok());
        r.input_modalities |= MODALITY_VIDEO;
        assert!(select(&[offer], &[capacity(0)], &[provider], &r).is_err());
        r.input_modalities = MODALITY_TEXT;
        r.max_input = 101;
        let mut offer = offer;
        offer.max_input = 200.into();
        assert!(select(&[offer], &[capacity(0)], &[provider], &r).is_err());
    }

    #[test]
    fn minimum_context_requires_explicit_limits_and_filters_offers() {
        let provider = Address::new_from_array([1; 32]);
        let mut offer = terms(provider, 1_000_000);
        let mut r = request(provider);
        r.min_context = 201;
        assert!(r.validate(r.spend).is_err());
        r.limits_configured = true;
        assert!(r.validate(r.spend).is_ok());
        assert!(select(&[offer], &[capacity(0)], &[provider], &r).is_err());
        offer.context_window = 201.into();
        assert!(select(&[offer], &[capacity(0)], &[provider], &r).is_ok());
        r.min_context = 0;
        assert!(r.validate(r.spend).is_ok());
    }

    #[test]
    fn incompatible_candidates_never_replace_incumbents_or_prune_valid_ties() {
        let providers = [1, 2, 3].map(|byte| Address::new_from_array([byte; 32]));
        let mut offers = [
            terms(providers[0], 1_000_000),
            terms(providers[1], 100_000),
            terms(providers[2], 500_000),
        ];
        offers[1].checkpoint = [8; 32];
        let capacities = [capacity(0); 3];
        let mut r = request(Address::default());
        assert_eq!(
            select(&offers, &capacities, &providers, &r).unwrap().slot,
            2
        );
        offers[0].input_modalities = MODALITY_AUDIO;
        r.estimated_input = 0;
        r.estimated_output = 0;
        assert_eq!(
            select(&offers, &capacities, &providers, &r).unwrap().slot,
            2
        );
        offers[1].checkpoint = r.checkpoint;
        assert_eq!(
            select(&offers, &capacities, &providers, &r).unwrap().slot,
            1
        );
    }

    #[test]
    fn unlisted_candidates_preserve_cache_and_lower_slot_ties() {
        let providers = [1, 2, 3].map(|byte| Address::new_from_array([byte; 32]));
        let mut offers = [
            terms(providers[0], 500_000),
            terms(providers[1], 250_000),
            terms(providers[2], 1_000_000),
        ];
        offers[1].cached_rate = 0.into();
        let capacities = [capacity(0); 3];
        let mut r = request(providers[1]);
        r.estimated_cached = r.estimated_input;
        let selected = select(&offers, &capacities, &[providers[0], providers[2]], &r).unwrap();
        assert_eq!(selected.slot, 0);
        assert!(!selected.cache);
        let unrelated = Address::new_from_array([9; 32]);
        let selected = select(
            &offers,
            &capacities,
            &[providers[0], providers[2], unrelated],
            &r,
        )
        .unwrap();
        assert_eq!(selected.slot, 0);
        assert!(!selected.cache);
        let selected = select(&offers, &capacities, &[providers[2]], &r).unwrap();
        assert_eq!(selected.slot, 2);
        r.estimated_input = 0;
        r.estimated_output = 0;
        r.estimated_cached = 0;
        let selected = select(
            &offers,
            &capacities,
            &[providers[1], providers[2], unrelated],
            &r,
        )
        .unwrap();
        assert_eq!(
            (selected.slot, selected.score, selected.cache),
            (1, 0, true)
        );
        assert!(select(
            &offers,
            &capacities,
            &[Address::new_from_array([9; 32])],
            &r
        )
        .is_err());
        assert!(select(&offers, &capacities, &[], &r).is_err());
        let unrelated = [9, 10, 11].map(|byte| Address::new_from_array([byte; 32]));
        assert!(select(&offers, &capacities, &unrelated, &r).is_err());
    }

    #[test]
    fn batches_retain_candidates_after_membership_or_budget_rejection() {
        let providers: [Address; 32] =
            core::array::from_fn(|index| Address::new_from_array([index as u8 + 1; 32]));
        let mut offers = providers.map(|provider| terms(provider, 1_000_000));
        for (index, offer) in offers.iter_mut().enumerate() {
            offer.output_rate = ((32 - index as u32) * 1_000_000).into();
        }
        let capacities = [capacity(0); 32];
        let mut r = request(Address::default());
        r.output_rate_cap = u32::MAX;
        r.input_rate_cap = u32::MAX;
        r.estimated_input = 0;
        r.estimated_output = 1;
        let selected = select(&offers, &capacities, &[providers[24], providers[27]], &r).unwrap();
        assert_eq!(
            (selected.slot, selected.score, selected.reserve),
            (27, 5, 601)
        );
        offers[31].input_rate = u32::MAX.into();
        let listed = [providers[24], providers[27], providers[31]];
        assert_eq!(select(&offers, &capacities, &listed, &r).unwrap().slot, 27);
        offers[31].input_rate = 1_000_000.into();
        assert_eq!(select(&offers, &capacities, &listed, &r).unwrap().slot, 31);
        offers[31].output_rate = 5_000_000.into();
        assert_eq!(select(&offers, &capacities, &listed, &r).unwrap().slot, 27);
        r.cache_provider = providers[27];
        r.estimated_input = 1;
        r.estimated_cached = 1;
        let cached = select(&offers, &capacities, &[providers[27]], &r).unwrap();
        assert_eq!((cached.slot, cached.score, cached.cache), (27, 6, true));
        let uncached = select(&offers, &capacities, &[providers[31]], &r).unwrap();
        assert_eq!(
            (uncached.slot, uncached.score, uncached.cache),
            (31, 6, false)
        );
        assert!(select(
            &offers,
            &capacities,
            &[Address::new_from_array([90; 32])],
            &r,
        )
        .is_err());
    }

    #[test]
    fn request_validation_rejects_unspecified_unknown_and_overflowing_requirements() {
        let r = request(Address::default());
        let changes: &[fn(&mut ReserveRequest)] = &[
            |r| r.checkpoint = [0; 32],
            |r| r.min_context = 1,
            |r| r.input_modalities = 0,
            |r| r.output_modalities = 0,
            |r| r.input_modalities |= 16,
            |r| r.output_modalities |= 128,
            |r| r.max_input = u32::MAX,
            |r| r.max_output = u32::MAX,
        ];
        for change in changes {
            let mut invalid = r;
            change(&mut invalid);
            assert!(invalid.validate(1_000).is_err());
        }
        let mut boundary = r;
        boundary.max_input = u32::MAX - boundary.max_output;
        assert!(boundary.validate(1_000).is_ok());
        boundary.max_input += 1;
        assert!(select(&[], &[], &[], &boundary).is_err());
    }

    #[test]
    fn selection_matches_independent_candidates_across_rounding_cache_and_limits() {
        let providers: [Address; 32] =
            core::array::from_fn(|index| Address::new_from_array([index as u8 + 1; 32]));
        let mut seed = 7u32;
        let mut next = || {
            seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            seed
        };
        for case in 0..1024 {
            let mut r = request(providers[next() as usize % providers.len()]);
            r.max_input = if case % 4 == 0 {
                u32::MAX - 1
            } else {
                1 + next() % 100
            };
            r.max_output = if case % 4 == 0 { 1 } else { 1 + next() % 100 };
            r.estimated_input = if case % 3 == 0 {
                r.max_input
            } else {
                next() % r.max_input
            };
            r.estimated_output = if case % 3 == 0 {
                r.max_output
            } else {
                next() % r.max_output
            };
            r.estimated_cached = if case % 2 == 0 {
                r.estimated_input / 2
            } else {
                r.estimated_input
            };
            r.private = case % 5 == 0;
            r.input_rate_cap = u32::MAX;
            r.cached_rate_cap = u32::MAX;
            r.output_rate_cap = u32::MAX;
            r.spend = if case % 7 == 0 { 10 } else { u64::MAX };
            r.validate(u64::MAX).unwrap();
            let mut offers = providers.map(|provider| terms(provider, 1));
            let mut capacities = [capacity(0); 32];
            let mut listed = alloc::vec::Vec::new();
            for (index, offer) in offers.iter_mut().enumerate() {
                let input_rate = if case % 4 == 0 {
                    u32::MAX
                } else if case % 3 == 0 {
                    next() % 10 + 1
                } else {
                    next().max(1)
                };
                offer.input_rate = input_rate.into();
                offer.cached_rate = (next() % input_rate).into();
                offer.output_rate = if case % 4 == 0 {
                    u32::MAX
                } else if case % 3 == 0 {
                    next() % 10 + 1
                } else {
                    next().max(1)
                }
                .into();
                offer.context_window = (r.max_input + r.max_output).into();
                offer.max_input = r.max_input.into();
                offer.max_output = r.max_output.into();
                if next() % 7 == 0 {
                    offer.checkpoint = [8; 32];
                }
                if next() % 5 == 0 {
                    offer.privacy = 0;
                }
                if next() % 5 == 0 {
                    capacities[index].active = 2.into();
                }
                if next() % 3 != 0 {
                    listed.push(offer.provider);
                }
            }
            if case % 2 == 0 {
                while listed.len() < providers.len() {
                    listed.push(Address::new_from_array([64 + listed.len() as u8; 32]));
                }
            }
            let expected = offers
                .iter()
                .zip(&capacities)
                .enumerate()
                .filter_map(|(slot, (offer, capacity))| {
                    select(
                        core::slice::from_ref(offer),
                        core::slice::from_ref(capacity),
                        &listed,
                        &r,
                    )
                    .ok()
                    .map(|result| (result.score, slot, result))
                })
                .min_by_key(|(score, slot, _)| (*score, *slot));
            let selected = select(&offers, &capacities, &listed, &r);
            match expected {
                Some((_, slot, candidate)) => {
                    let selected = selected.unwrap();
                    assert_eq!(selected.slot as usize, slot, "case {case}");
                    assert_eq!(
                        (selected.score, selected.reserve, selected.cache),
                        (candidate.score, candidate.reserve, candidate.cache)
                    );
                }
                None => assert!(selected.is_err(), "case {case}"),
            }
        }
    }
}
