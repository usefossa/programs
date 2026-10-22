use crate::{
    abi::*,
    helpers::{collateral, fee, wr32, wr64},
    matching::{select, Request},
};
use std::vec;

fn payload() -> [u8; 107] {
    let mut p = [0; 107];
    for i in [32, 36, 40] {
        wr32(&mut p, i, u32::MAX);
    }
    wr32(&mut p, 46, 1_000_000);
    wr32(&mut p, 50, 1_000_000);
    wr64(&mut p, 55, u64::MAX);
    wr32(&mut p, 63, 1_000_000);
    wr32(&mut p, 71, 1_000_000);
    p
}
fn books(rates: &[[u32; 3]]) -> (std::vec::Vec<u8>, std::vec::Vec<u8>) {
    let mut t = vec![0; terms_book_len(rates.len())];
    let mut c = vec![0; capacity_book_len(rates.len())];
    wr32(&mut t, 44, rates.len() as u32);
    wr32(&mut c, 44, rates.len() as u32);
    for (i, rates) in rates.iter().enumerate() {
        let ti = HEADER_LEN + i * TERMS_LEN;
        let ci = HEADER_LEN + i * CAPACITY_LEN;
        t[ti..ti + 32].fill(i as u8 + 1);
        for (j, rate) in rates.iter().enumerate() {
            wr32(&mut t, ti + 96 + j * 4, *rate);
        }
        t[ti + 108] = 100;
        t[ti + 110..ti + 113].fill(1);
        wr32(&mut t, ti + 116, 2);
        wr64(&mut c, ci, u64::MAX);
    }
    (t, c)
}
#[test]
fn split_rounding_and_maximum_arithmetic() {
    for input in 0..50u32 {
        for output in 0..10u32 {
            for rate in [1, 50_000, 999_999, 1_000_000, u32::MAX] {
                let reserve = fee(input, rate) + fee(output, rate) + 1;
                for cached in 0..=input {
                    for cached_rate in [0, rate / 2, rate] {
                        let charge = fee(input - cached, rate)
                            + fee(cached, cached_rate)
                            + fee(output, rate);
                        assert!(charge <= reserve);
                    }
                }
            }
        }
    }
    assert_eq!(
        fee(u32::MAX, u32::MAX),
        (u128::from(u32::MAX).pow(2)).div_ceil(u128::from(UNIT)) as u64
    );
    assert_eq!(collateral(101).unwrap(), 103);
    assert!(collateral(u64::MAX).is_err());
}
#[test]
fn multidimensional_score_cache_provenance_and_ties() {
    let (t, c) = books(&[[100, 1, 800], [300, 2, 50], [100, 1, 800]]);
    let mut p = payload();
    assert_eq!(
        select(&t, &c, &Request::parse(&p, u64::MAX).unwrap())
            .unwrap()
            .slot,
        1
    );
    wr32(&mut p, 71, 0);
    assert_eq!(
        select(&t, &c, &Request::parse(&p, u64::MAX).unwrap())
            .unwrap()
            .slot,
        0
    );
    wr32(&mut p, 67, 1_000_000);
    p[75..107].fill(2);
    let chosen = select(&t, &c, &Request::parse(&p, u64::MAX).unwrap()).unwrap();
    assert_eq!(
        (chosen.slot, chosen.score, chosen.reserve, chosen.cache),
        (1, 2, 351, true)
    );
    p[75..107].fill(9);
    let chosen = select(&t, &c, &Request::parse(&p, u64::MAX).unwrap()).unwrap();
    assert_eq!((chosen.slot, chosen.score, chosen.cache), (0, 100, false));
}
#[test]
fn eligibility_excludes_every_unfunded_or_disallowed_candidate() {
    let (t, c) = books(&[[1, 1, 1], [2, 2, 2]]);
    let p = payload();
    let r = Request::parse(&p, u64::MAX).unwrap();
    for offset in [111, 112] {
        let mut changed = t.clone();
        changed[HEADER_LEN + offset] = 0;
        assert_eq!(select(&changed, &c, &r).unwrap().slot, 1);
    }
    let mut changed = c.clone();
    wr32(&mut changed, HEADER_LEN + 20, u32::MAX);
    assert_eq!(select(&t, &changed, &r).unwrap().slot, 1);
    wr32(&mut changed, HEADER_LEN + 20, 0);
    wr32(&mut changed, HEADER_LEN + 16, 2);
    assert_eq!(select(&t, &changed, &r).unwrap().slot, 1);
    wr32(&mut changed, HEADER_LEN + 16, 0);
    wr64(&mut changed, HEADER_LEN, 3);
    assert_eq!(select(&t, &changed, &r).unwrap().slot, 1);
    wr64(&mut changed, HEADER_LEN, 4);
    assert_eq!(select(&t, &changed, &r).unwrap().slot, 0);
    wr64(&mut changed, HEADER_LEN + 8, u64::MAX);
    assert_eq!(select(&t, &changed, &r).unwrap().slot, 1);
    let mut q = p;
    q[54] = 1;
    let mut changed = t.clone();
    changed[HEADER_LEN + 110] = 0;
    assert_eq!(
        select(&changed, &c, &Request::parse(&q, u64::MAX).unwrap())
            .unwrap()
            .slot,
        1
    );
    q[44] = 101;
    assert!(select(&t, &c, &Request::parse(&q, u64::MAX).unwrap()).is_err());
    q = p;
    wr32(&mut q, 32, 0);
    assert!(select(&t, &c, &Request::parse(&q, u64::MAX).unwrap()).is_err());
    assert!(select(&t, &c, &Request::parse(&p, 2).unwrap()).is_err());
    q = p;
    wr64(&mut q, 55, 2);
    assert!(select(&t, &c, &Request::parse(&q, u64::MAX).unwrap()).is_err());
}
#[test]
fn request_bounds_and_book_count() {
    let mut p = payload();
    wr32(&mut p, 67, 1);
    assert!(Request::parse(&p, 100).is_err());
    p[75] = 1;
    assert!(Request::parse(&p, 100).is_ok());
    wr32(&mut p, 67, 1_000_001);
    assert!(Request::parse(&p, 100).is_err());
    wr32(&mut p, 67, 0);
    wr32(&mut p, 71, 1_000_001);
    assert!(Request::parse(&p, 100).is_err());
    p = payload();
    wr32(&mut p, 46, 0);
    assert!(Request::parse(&p, 100).is_err());
    let (t, mut c) = books(&[[1, 0, 1]]);
    wr32(&mut c, 44, 0);
    assert!(select(&t, &c, &Request::parse(&payload(), 100).unwrap()).is_err());
}
#[test]
fn funding_survives_partial_refunds_and_pending_unbond() {
    for a in 0..150u64 {
        for b in 0..150u64 {
            let funded = collateral(a + b).unwrap();
            for refund in [0, a / 2, a] {
                assert!(funded - refund >= collateral(b).unwrap());
            }
        }
    }
}
