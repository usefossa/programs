use crate::{
    args::ReserveRequest,
    helpers::{fee, require},
    state_types::{CapacityEntry, TermsEntry, MAX_ENTRIES},
    CLAIM_SECONDS,
};
use anchor_lang::prelude::{Address, ProgramError};

impl ReserveRequest {
    pub(crate) fn validate(&self, free: u64) -> Result<Self, ProgramError> {
        let mut request = *self;
        require(
            request.max_input > 0
                && request.max_output > 0
                && request.estimated_input <= request.max_input
                && request.estimated_output <= request.max_output
                && request.estimated_cached <= request.estimated_input
                && (request.estimated_cached == 0
                    || request.cache_provider != anchor_lang::prelude::Address::default()),
            crate::RouterError::InvalidArgument,
        )?;
        request.spend = free.min(request.spend);
        Ok(request)
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
    terms: &[TermsEntry],
    capacity: &[CapacityEntry],
    providers: &[Address],
    r: &ReserveRequest,
) -> Result<Match, ProgramError> {
    let count = terms.len();
    require(
        count <= MAX_ENTRIES && count == capacity.len(),
        crate::RouterError::InvalidAccount,
    )?;
    // ceil(output / speed) must fit the claim window; compute required speed once.
    let min_speed =
        u32::from(r.min_speed).max(r.max_output.div_ceil((86_400 - CLAIM_SECONDS) as u32));
    let cache_member = r.estimated_cached > 0 && providers.binary_search(&r.cache_provider).is_ok();
    let mut best = Match {
        slot: 0,
        reserve: 0,
        score: u64::MAX,
        cache: false,
    };
    for (slot, (t, c)) in terms.iter().zip(capacity).enumerate() {
        let input = t.input_rate.get();
        let cached_rate = t.cached_rate.get();
        let output = t.output_rate.get();
        if t.active != 1
            || (r.private && t.privacy != 1)
            || u32::from(t.speed.get()) < min_speed
            || c.active.get() >= t.capacity.get()
            || input > r.input_rate_cap
            || cached_rate > r.cached_rate_cap
            || output > r.output_rate_cap
        {
            continue;
        }
        let cached = r.estimated_cached > 0 && t.provider == r.cache_provider;
        let score = if cached {
            fee(r.estimated_input - r.estimated_cached, input)
                + fee(r.estimated_cached, cached_rate)
        } else {
            fee(r.estimated_input, input)
        } + fee(r.estimated_output, output);
        // scores fit u64; ascending slots preserve lower-slot ties and permit safe score pruning.
        if score >= best.score {
            continue;
        }
        if !(if cached {
            cache_member
        } else {
            providers.binary_search(&t.provider).is_ok()
        }) {
            continue;
        }
        let reserve = fee(r.max_input, input) + fee(r.max_output, output) + 1;
        if reserve > r.spend {
            continue;
        }
        best = Match {
            slot: slot as u32,
            reserve,
            score,
            cache: false,
        };
        if score == 0 {
            break;
        }
    }
    require(best.score != u64::MAX, crate::RouterError::Unavailable)?;
    best.cache = terms[best.slot as usize].provider == r.cache_provider;
    Ok(best)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terms(provider: Address, input_rate: u32) -> TermsEntry {
        TermsEntry {
            provider,
            payout: Address::new_from_array([9; 32]),
            reserved: [0; 32],
            input_rate: input_rate.into(),
            cached_rate: input_rate.into(),
            output_rate: input_rate.into(),
            speed: 100.into(),
            privacy: 1,
            active: 1,
            reserved2: [0; 4],
            capacity: 2.into(),
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
        }
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
}
