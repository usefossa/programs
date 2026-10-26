use crate::{
    helpers::{bps_ceil, fee_budget},
    instructions::fees::validate_terms,
};

#[test]
fn fee_rounding_preserves_balance_and_protocol_floor() {
    for fee in 10..=300 {
        for base in [
            0,
            1,
            2,
            99,
            100,
            9999,
            10_000,
            1_000_001,
            u32::MAX as u64,
            u64::MAX,
        ] {
            let charged = bps_ceil(base, fee);
            assert_eq!(
                u128::from(charged),
                (u128::from(base) * u128::from(fee)).div_ceil(10_000)
            );
            for share in [5000, 5001, 7500, 9999, 10_000] {
                let protocol = bps_ceil(charged, share);
                assert!(protocol <= charged);
                assert!(u128::from(protocol) * 2 >= u128::from(charged));
                assert_eq!((charged - protocol) + protocol, charged);
            }
            let budget = fee_budget(base, fee);
            assert!(u128::from(budget) + u128::from(bps_ceil(budget, fee)) <= u128::from(base));
            assert!(
                u128::from(budget + 1) + u128::from(bps_ceil(budget + 1, fee)) > u128::from(base)
            );
        }
    }
    assert!(validate_terms(10, 5000).is_ok());
    assert!(validate_terms(300, 10_000).is_ok());
    for terms in [(0, 5000), (9, 5000), (301, 5000), (100, 4999), (100, 10001)] {
        assert!(validate_terms(terms.0, terms.1).is_err());
    }
}
