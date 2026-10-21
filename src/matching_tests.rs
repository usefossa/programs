use super::*;
use std::vec::Vec;

#[test]
fn zero_check_rejects_every_dirty_byte_in_aligned_and_unaligned_slices() {
    for offset in 0..8 {
        for length in 0..=BOOKING_LEN {
            let mut d = std::vec![0; length + offset];
            assert!(all_zero(&d[offset..]));
            for at in offset..d.len() {
                d[at] = 1;
                assert!(
                    !all_zero(&d[offset..]),
                    "offset {offset}, length {length}, byte {at}"
                );
                d[at] = 0;
            }
        }
    }
}

fn word(d: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(d[i..i + 4].try_into().unwrap())
}
fn wide(d: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(d[i..i + 8].try_into().unwrap())
}
fn short(d: &[u8], i: usize) -> u16 {
    u16::from_le_bytes(d[i..i + 2].try_into().unwrap())
}
fn reference(t: &[u8], c: &[u8], p: &[u8], free: u64) -> Option<Match> {
    let cost = |n: u32, rate: u32| (u128::from(n) * u128::from(rate)).div_ceil(1_000_000);
    let mut eligible = Vec::new();
    for index in 0..word(t, 44) as usize {
        let at = 48 + index * 120;
        let state = 48 + index * 32;
        let prices = [word(t, at + 96), word(t, at + 100), word(t, at + 104)];
        if t[at + 111] != 1
            || t[at + 112] != 1
            || (p[54] == 1 && t[at + 110] != 1)
            || short(t, at + 108) < short(p, 44)
            || u128::from(word(p, 50))
                > (86_400 - CLAIM_SECONDS) as u128 * u128::from(short(t, at + 108))
            || word(c, state + 16) >= word(t, at + 116)
            || word(c, state + 20) == u32::MAX
            || (0..3).any(|j| prices[j] > word(p, 32 + j * 4))
        {
            continue;
        }
        let reserve = cost(word(p, 46), prices[0]) + cost(word(p, 50), prices[2]) + 1;
        if reserve > u128::from(free) || reserve > u128::from(wide(p, 55)) {
            continue;
        }
        let exposure = u128::from(wide(c, state + 8)) + reserve;
        if exposure + exposure.div_ceil(100) > u128::from(wide(c, state)) {
            continue;
        }
        let cache = t[at..at + 32] == p[75..107];
        let cached = if cache { word(p, 67) } else { 0 };
        let score = cost(word(p, 63) - cached, prices[0])
            + cost(cached, prices[1])
            + cost(word(p, 71), prices[2]);
        eligible.push(Match {
            slot: index as u32,
            reserve: reserve as u64,
            score: score as u64,
            exposure: exposure as u64,
            cache,
        });
    }
    eligible.sort_by_key(|m| (m.score, m.slot));
    eligible.into_iter().next()
}

fn matcher_fixture(speeds: &[u16]) -> (Vec<u8>, Vec<u8>, [u8; 107]) {
    let mut t = std::vec![0; terms_book_len(speeds.len())];
    let mut c = std::vec![0; capacity_book_len(speeds.len())];
    wr32(&mut t, 44, speeds.len() as u32);
    wr32(&mut c, 44, speeds.len() as u32);
    for (slot, speed) in speeds.iter().enumerate() {
        let ti = HEADER_LEN + slot * TERMS_LEN;
        let ci = HEADER_LEN + slot * CAPACITY_LEN;
        t[ti..ti + 32].fill((slot % 255 + 1) as u8);
        for offset in [96, 100, 104] {
            wr32(&mut t, ti + offset, (slot as u32 + 1) * 1_000_000);
        }
        t[ti + 108..ti + 110].copy_from_slice(&speed.to_le_bytes());
        t[ti + 110..ti + 113].fill(1);
        wr32(&mut t, ti + 116, 2);
        wr64(&mut c, ci, u64::MAX);
    }
    let mut p = [0; 107];
    for offset in [32, 36, 40] {
        wr32(&mut p, offset, u32::MAX);
    }
    for offset in [46, 50, 63, 71] {
        wr32(&mut p, offset, 1);
    }
    wr64(&mut p, 55, u64::MAX);
    (t, c, p)
}

#[test]
fn duration_eligibility_skips_cheapest_slow_offer_at_exact_boundary() {
    let (t, c, mut p) = matcher_fixture(&[1, 2]);
    let window = (86_400 - CLAIM_SECONDS) as u32;
    // production window is 86,100: 86,101 output tokens require the speed-two offer.
    for (output, slot) in [
        (window, Some(0)),
        (window + 1, Some(1)),
        (window * 2, Some(1)),
        (window * 2 + 1, None),
    ] {
        wr32(&mut p, 50, output);
        wr32(&mut p, 71, output);
        let actual = select(&t, &c, &Request::parse(&p, u64::MAX).unwrap()).ok();
        assert_eq!(actual.as_ref().map(|m| m.slot), slot);
        assert_eq!(actual, reference(&t, &c, &p, u64::MAX));
    }
    let (t, c, mut p) = matcher_fixture(&[1, u16::MAX]);
    wr32(&mut p, 50, u32::MAX);
    let actual = select(&t, &c, &Request::parse(&p, u64::MAX).unwrap()).unwrap();
    assert_eq!(actual.slot, 1);
    assert_eq!(Some(actual), reference(&t, &c, &p, u64::MAX));
}

#[test]
fn zero_score_keeps_first_fully_eligible_tie_and_checks_reserves() {
    let (mut t, mut c, mut p) = matcher_fixture(&[2, 2, 1, 2, 2, 2, 2, 2]);
    wr32(&mut p, 50, (86_400 - CLAIM_SECONDS) as u32 + 1);
    wr32(&mut p, 63, 0);
    wr32(&mut p, 71, 0);
    p[75..107].fill(7);
    t[HEADER_LEN + 111] = 0;
    t[HEADER_LEN + TERMS_LEN + 112] = 0;
    wr32(&mut c, HEADER_LEN + 3 * CAPACITY_LEN + 16, 2);
    wr64(&mut c, HEADER_LEN + 4 * CAPACITY_LEN, 0);
    wr64(&mut c, HEADER_LEN + 5 * CAPACITY_LEN + 8, u64::MAX);
    let actual = select(&t, &c, &Request::parse(&p, u64::MAX).unwrap()).unwrap();
    assert_eq!((actual.slot, actual.score, actual.cache), (6, 0, true));
    assert_eq!(Some(actual), reference(&t, &c, &p, u64::MAX));
    wr32(&mut c, HEADER_LEN + 6 * CAPACITY_LEN + 20, u32::MAX);
    let actual = select(&t, &c, &Request::parse(&p, u64::MAX).unwrap()).unwrap();
    assert_eq!((actual.slot, actual.score), (7, 0));
    assert_eq!(Some(actual), reference(&t, &c, &p, u64::MAX));
    wr64(&mut p, 55, 1);
    assert!(select(&t, &c, &Request::parse(&p, u64::MAX).unwrap()).is_err());
    assert!(reference(&t, &c, &p, u64::MAX).is_none());
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn to(&mut self, max: u32) -> u32 {
        (self.next() % (u64::from(max) + 1)) as u32
    }
}
#[test]
fn randomized_matcher_matches_independent_sorted_u128_reference() {
    let mut rng = Rng(0x6a09e667f3bcc909);
    let mut matched = 0;
    let mut cached_winners = 0;
    for case in 0..12_000 {
        let count = rng.to(63) as usize + 1;
        // cheap unregistered tail must never participate.
        let mut t = std::vec![0; terms_book_len(count + 1)];
        let mut c = std::vec![0; capacity_book_len(count + 1)];
        wr32(&mut t, 44, count as u32);
        wr32(&mut c, 44, count as u32);
        let mut p = [0; 107];
        let tokens = if case % 4 == 0 { u32::MAX } else { 1_000_000 };
        let max_in = rng.to(tokens - 1) + 1;
        let max_out = rng.to(tokens - 1) + 1;
        wr32(&mut p, 46, max_in);
        wr32(&mut p, 50, max_out);
        let est_in = rng.to(max_in);
        let est_out = rng.to(max_out);
        wr32(&mut p, 63, est_in);
        wr32(&mut p, 71, est_out);
        wr32(&mut p, 67, rng.to(est_in));
        p[75..107].fill(rng.to(8) as u8 + 1);
        let relaxed = case % 3 == 0;
        for at in [32, 36, 40] {
            wr32(&mut p, at, if relaxed { u32::MAX } else { rng.to(500) });
        }
        p[54] = if relaxed { 0 } else { rng.to(1) as u8 };
        p[44..46].copy_from_slice(&(if relaxed { 0 } else { rng.to(100) } as u16).to_le_bytes());
        wr64(
            &mut p,
            55,
            if relaxed {
                u64::MAX
            } else {
                rng.next() % 2_000
            },
        );
        let free = if case % 5 == 0 {
            rng.next() % 2_000
        } else {
            u64::MAX
        };
        for slot in 0..=count {
            let ti = 48 + slot * 120;
            let ci = 48 + slot * 32;
            t[ti..ti + 32].fill(rng.to(8) as u8 + 1);
            let rate_bound = if case % 4 == 0 { u32::MAX } else { 1_000 };
            let input = rng.to(rate_bound - 1) + 1;
            wr32(&mut t, ti + 96, input);
            wr32(&mut t, ti + 100, rng.to(input));
            wr32(&mut t, ti + 104, rng.to(rate_bound - 1) + 1);
            let max_speed = if case % 4 == 0 { u16::MAX as u32 } else { 100 };
            t[ti + 108..ti + 110]
                .copy_from_slice(&(rng.to(max_speed - 1) as u16 + 1).to_le_bytes());
            for at in [110, 111, 112] {
                t[ti + at] = if relaxed { 1 } else { u8::from(rng.to(9) > 0) };
            }
            wr32(&mut t, ti + 116, rng.to(10) + 1);
            wr64(
                &mut c,
                ci,
                if relaxed {
                    u64::MAX
                } else {
                    rng.next() % 10_000
                },
            );
            wr64(
                &mut c,
                ci + 8,
                if case % 17 == 0 {
                    u64::MAX - rng.to(100) as u64
                } else if relaxed {
                    0
                } else {
                    rng.next() % 1_000
                },
            );
            wr32(&mut c, ci + 16, if relaxed { 0 } else { rng.to(10) });
            wr32(
                &mut c,
                ci + 20,
                if case % 13 == 0 && slot % 2 == 0 {
                    u32::MAX
                } else {
                    rng.to(10)
                },
            );
            if slot == count {
                // even a fully eligible free-score tail is outside registered count.
                wr32(&mut t, ti + 96, 1);
                wr32(&mut t, ti + 100, 0);
                wr32(&mut t, ti + 104, 1);
                t[ti + 110..ti + 113].fill(1);
                wr64(&mut c, ci, u64::MAX);
                wr64(&mut c, ci + 8, 0);
                wr32(&mut c, ci + 16, 0);
                wr32(&mut c, ci + 20, 0);
            }
        }
        let actual = select(&t, &c, &Request::parse(&p, free).unwrap()).ok();
        let expected = reference(&t, &c, &p, free);
        assert_eq!(actual, expected, "seeded case {case}");
        matched += usize::from(actual.is_some());
        cached_winners += usize::from(actual.is_some_and(|m| m.cache));
    }
    assert!(
        matched > 2_000 && cached_winners > 200,
        "matcher coverage {matched}/{cached_winners}"
    );
}
#[test]
fn fee_matches_wide_integer_ceil_at_boundaries_and_random_products() {
    let mut rng = Rng(0xbb67ae8584caa73b);
    for n in [0, 1, 999_999, 1_000_000, u32::MAX - 1, u32::MAX] {
        for rate in [0, 1, 999_999, 1_000_000, u32::MAX - 1, u32::MAX] {
            assert_eq!(
                u128::from(fee(n, rate)),
                (u128::from(n) * u128::from(rate)).div_ceil(1_000_000)
            );
        }
    }
    for _ in 0..100_000 {
        let n = rng.next() as u32;
        let rate = rng.next() as u32;
        assert_eq!(
            u128::from(fee(n, rate)),
            (u128::from(n) * u128::from(rate)).div_ceil(1_000_000)
        );
    }
}

#[test]
fn maximum_book_scan_ignores_unregistered_tail_and_rejects_oversized_count() {
    let (mut terms, mut capacity, payload) = matcher_fixture(&vec![100; MAX_ENTRIES + 1]);
    wr32(&mut terms, 44, MAX_ENTRIES as u32);
    wr32(&mut capacity, 44, MAX_ENTRIES as u32);
    let request = Request::parse(&payload, u64::MAX).unwrap();
    for slot in 0..MAX_ENTRIES - 1 {
        terms[HEADER_LEN + slot * TERMS_LEN + 111] = 0;
    }
    assert_eq!(select(&terms, &capacity, &request).unwrap().slot, MAX_ENTRIES as u32 - 1);
    terms[HEADER_LEN + (MAX_ENTRIES - 1) * TERMS_LEN + 111] = 0;
    assert!(select(&terms, &capacity, &request).is_err());
    wr32(&mut terms, 44, MAX_ENTRIES as u32 + 1);
    wr32(&mut capacity, 44, MAX_ENTRIES as u32 + 1);
    assert!(select(&terms, &capacity, &request).is_err());
}
