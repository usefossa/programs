use crate::{
    abi::{CAPACITY_LEN, HEADER_LEN, MAX_ENTRIES, TERMS_LEN},
    helpers::{collateral, err, fee, rd16, rd32, rd64, require},
    CLAIM_SECONDS,
};
use pinocchio::error::ProgramError;

pub(crate) struct Request {
    caps: [u32; 3],
    min_speed: u16,
    private: bool,
    cache_provider: [u8; 32],
    pub(crate) max_input: u32,
    pub(crate) max_output: u32,
    estimated_input: u32,
    estimated_cached: u32,
    estimated_output: u32,
    pub(crate) spend: u64,
}
impl Request {
    pub(crate) fn parse(p: &[u8], free: u64) -> Result<Self, ProgramError> {
        require(p.len() == 107 && p[54] <= 1, 3)?;
        let r = Self {
            caps: [rd32(p, 32), rd32(p, 36), rd32(p, 40)],
            min_speed: rd16(p, 44),
            private: p[54] == 1,
            cache_provider: p[75..107].try_into().unwrap(),
            max_input: rd32(p, 46),
            max_output: rd32(p, 50),
            estimated_input: rd32(p, 63),
            estimated_cached: rd32(p, 67),
            estimated_output: rd32(p, 71),
            spend: free.min(rd64(p, 55)),
        };
        require(
            r.max_input > 0
                && r.max_output > 0
                && r.estimated_input <= r.max_input
                && r.estimated_output <= r.max_output
                && r.estimated_cached <= r.estimated_input
                && (r.estimated_cached == 0 || p[75..107] != [0; 32]),
            3,
        )?;
        Ok(r)
    }
}
#[derive(Debug, PartialEq)]
pub(crate) struct Match {
    pub(crate) slot: u32,
    pub(crate) reserve: u64,
    pub(crate) score: u64,
    pub(crate) exposure: u64,
    pub(crate) cache: bool,
}
pub(crate) fn select(terms: &[u8], capacity: &[u8], r: &Request) -> Result<Match, ProgramError> {
    let count = rd32(terms, 44) as usize;
    require(
        count <= MAX_ENTRIES && count == rd32(capacity, 44) as usize,
        2,
    )?;
    let terms = terms
        .get(HEADER_LEN..HEADER_LEN + count * TERMS_LEN)
        .ok_or(err(2))?;
    let capacity = capacity
        .get(HEADER_LEN..HEADER_LEN + count * CAPACITY_LEN)
        .ok_or(err(2))?;
    // ceil(output / speed) must fit the claim window; compute required speed once.
    let min_speed =
        u32::from(r.min_speed).max(r.max_output.div_ceil((86_400 - CLAIM_SECONDS) as u32));
    let mut best = Match {
        slot: 0,
        reserve: 0,
        score: u64::MAX,
        exposure: 0,
        cache: false,
    };
    for (slot, (t, c)) in terms
        .chunks_exact(TERMS_LEN)
        .zip(capacity.chunks_exact(CAPACITY_LEN))
        .enumerate()
    {
        if t[111] != 1
            || t[112] != 1
            || (r.private && t[110] != 1)
            || u32::from(rd16(t, 108)) < min_speed
            || rd32(c, 16) >= rd32(t, 116)
            || rd32(c, 20) == u32::MAX
        {
            continue;
        }
        let input = rd32(t, 96);
        let cached = rd32(t, 100);
        let output = rd32(t, 104);
        if input > r.caps[0] || cached > r.caps[1] || output > r.caps[2] {
            continue;
        }
        let cached_tokens = if r.estimated_cached > 0 && t[..32] == r.cache_provider {
            r.estimated_cached
        } else {
            0
        };
        let score = fee(r.estimated_input - cached_tokens, input)
            + fee(cached_tokens, cached)
            + fee(r.estimated_output, output);
        // scores fit u64; ascending slots preserve lower-slot ties and permit safe score pruning.
        if score >= best.score {
            continue;
        }
        let reserve = fee(r.max_input, input) + fee(r.max_output, output) + 1;
        if reserve > r.spend {
            continue;
        }
        let Some(exposure) = rd64(c, 8).checked_add(reserve) else {
            continue;
        };
        if !collateral(exposure).is_ok_and(|needed| rd64(c, 0) >= needed) {
            continue;
        }
        best = Match {
            slot: slot as u32,
            reserve,
            score,
            exposure,
            cache: false,
        };
        if score == 0 {
            break;
        }
    }
    require(best.score != u64::MAX, 6)?;
    let at = best.slot as usize * TERMS_LEN;
    best.cache = terms[at..at + 32] == r.cache_provider;
    Ok(best)
}
