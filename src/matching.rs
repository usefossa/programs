use crate::{
    abi::{CAPACITY_LEN, HEADER_LEN, MAX_ENTRIES, TERMS_LEN},
    helpers::{err, fee, rd16, rd32, rd64, require},
    CLAIM_SECONDS,
};
use anchor_lang::prelude::ProgramError;

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
    pub(crate) cache: bool,
}
pub(crate) fn select(
    terms: &[u8],
    capacity: &[u8],
    providers: &[u8],
    r: &Request,
) -> Result<Match, ProgramError> {
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
    // runtime account data and fixed entries are eight-byte aligned; validate before word reads.
    require(
        (terms.as_ptr() as usize | capacity.as_ptr() as usize | providers.as_ptr() as usize) & 7
            == 0,
        2,
    )?;
    let members = crate::instructions::list::Members::new(providers);
    // ceil(output / speed) must fit the claim window; compute required speed once.
    let min_speed =
        u32::from(r.min_speed).max(r.max_output.div_ceil((86_400 - CLAIM_SECONDS) as u32));
    let cache_words: [u64; 4] = core::array::from_fn(|i| rd64(&r.cache_provider, i * 8));
    let cache_member = r.estimated_cached > 0
        && members.contains(unsafe {
            core::slice::from_raw_parts(cache_words.as_ptr().cast::<u8>(), 32)
        });
    let mut best = Match {
        slot: 0,
        reserve: 0,
        score: u64::MAX,
        cache: false,
    };
    for (slot, (t, c)) in terms
        .chunks_exact(TERMS_LEN)
        .zip(capacity.chunks_exact(CAPACITY_LEN))
        .enumerate()
    {
        let provider = &t[..32];
        // both complete entries contain integer bytes only; alignment and lengths were checked above.
        let words = unsafe { &*t.as_ptr().cast::<[u64; 15]>() };
        let state = unsafe { &*c.as_ptr().cast::<[u64; 4]>() };
        let input = words[12] as u32;
        let cached = (words[12] >> 32) as u32;
        let output = words[13] as u32;
        let speed = (words[13] >> 32) as u16;
        let privacy = (words[13] >> 48) as u8;
        let active = (words[13] >> 56) as u8;
        if active != 1
            || (r.private && privacy != 1)
            || u32::from(speed) < min_speed
            || state[2] as u32 >= (words[14] >> 32) as u32
            || input > r.caps[0]
            || cached > r.caps[1]
            || output > r.caps[2]
        {
            continue;
        }
        let cached = r.estimated_cached > 0
            && words[3] == cache_words[3]
            && words[2] == cache_words[2]
            && words[1] == cache_words[1]
            && words[0] == cache_words[0];
        let score = if cached {
            fee(r.estimated_input - r.estimated_cached, input)
                + fee(r.estimated_cached, (words[12] >> 32) as u32)
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
            members.contains(provider)
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
    require(best.score != u64::MAX, 6)?;
    let at = best.slot as usize * TERMS_LEN;
    best.cache = terms[at..at + 32] == r.cache_provider;
    Ok(best)
}
