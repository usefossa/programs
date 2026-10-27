use pinocchio::error::ProgramError;

pub type Result<T> = core::result::Result<T, ProgramError>;
pub const BPS: u64 = 10_000;

pub fn check(ok: bool, code: u32) -> Result<()> {
    if ok {
        Ok(())
    } else {
        Err(ProgramError::Custom(code))
    }
}
pub fn add(a: u64, b: u64) -> Result<u64> {
    a.checked_add(b).ok_or(ProgramError::ArithmeticOverflow)
}
pub fn sub(a: u64, b: u64) -> Result<u64> {
    a.checked_sub(b).ok_or(ProgramError::ArithmeticOverflow)
}
pub fn narrow(n: u128) -> Result<u64> {
    n.try_into().map_err(|_| ProgramError::ArithmeticOverflow)
}
pub fn mul_div(a: u64, b: u64, denominator: u64, ceil: bool) -> Result<u64> {
    check(denominator != 0, 1)?;
    let product = u128::from(a) * u128::from(b);
    let divisor = u128::from(denominator);
    narrow(product / divisor + u128::from(ceil && product % divisor != 0))
}
pub fn fee(amount: u64, bps: u16, cap: u64) -> Result<u64> {
    check(u64::from(bps) <= BPS, 1)?;
    if bps == 0 || cap == 0 {
        return Ok(0);
    }
    Ok(mul_div(amount, u64::from(bps), BPS, true)?.min(cap))
}
pub fn gross(net: u64, bps: u16, cap: u64) -> Result<u64> {
    check(u64::from(bps) <= BPS, 1)?;
    if net == 0 || bps == 0 || cap == 0 {
        return Ok(net);
    }
    let capped = net.checked_add(cap);
    let uncapped = if u64::from(bps) == BPS {
        None
    } else {
        mul_div(net, BPS, BPS - u64::from(bps), true).ok()
    };
    match (capped, uncapped) {
        (Some(a), Some(b)) => Ok(a.min(b)),
        (Some(a), None) | (None, Some(a)) => Ok(a),
        _ => Err(ProgramError::ArithmeticOverflow),
    }
}
pub fn output(reserve_in: u64, reserve_out: u64, input: u64) -> Result<u64> {
    check(reserve_in > 0 && reserve_out > 0 && input > 0, 1)?;
    mul_div(reserve_out, input, add(reserve_in, input)?, false)
}
pub fn input(reserve_in: u64, reserve_out: u64, output: u64) -> Result<u64> {
    check(reserve_in > 0 && output > 0 && output < reserve_out, 1)?;
    mul_div(reserve_in, output, sub(reserve_out, output)?, true)
}
pub fn sqrt(value: u128) -> u64 {
    if value < 2 {
        return value as u64;
    }
    let mut x = 1u128 << ((128 - value.leading_zeros()).div_ceil(2));
    loop {
        let next = (x + value / x) / 2;
        if next >= x {
            return x as u64;
        }
        x = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inverse_transfer_fees_are_minimal_and_exact() {
        for bps in [0, 1, 100, 3333, 9999, 10000] {
            for cap in [0, 1, 10, 1000, u64::MAX] {
                for n in [0, 1, 2, 13, 9999, 10000, 1_000_000, u64::MAX / 10000] {
                    if let Ok(g) = gross(n, bps, cap) {
                        assert_eq!(g - fee(g, bps, cap).unwrap(), n);
                        if g > 0 {
                            assert!(g - 1 - fee(g - 1, bps, cap).unwrap() < n);
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn constant_product_never_decreases_and_inverse_is_minimal() {
        for a in [1, 2, 113, 1_000_000, u32::MAX as u64] {
            for b in [2, 7, 359, 10_000_000, u32::MAX as u64] {
                for n in [1, 2, 11, 100_000] {
                    let out = output(a, b, n).unwrap();
                    assert!(
                        u128::from(a + n) * u128::from(b - out) >= u128::from(a) * u128::from(b)
                    );
                    if out > 0 {
                        let needed = input(a, b, out).unwrap();
                        assert!(needed <= n);
                        assert!(output(a, b, needed).unwrap() >= out);
                        if needed > 1 {
                            assert!(output(a, b, needed - 1).unwrap() < out);
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn integer_sqrt_preserves_minimum_lp_precision() {
        for n in [0, 1, 2, 3, 4, 15, 16, 17, u64::MAX as u128, u128::MAX] {
            let s = u128::from(sqrt(n));
            assert!(s * s <= n);
            assert!(s == u64::MAX as u128 || (s + 1) * (s + 1) > n);
        }
    }
    #[test]
    fn overflow_and_dust_fail_closed() {
        assert!(output(u64::MAX, 100, 1).is_err());
        assert!(input(100, 100, 100).is_err());
        assert!(gross(u64::MAX, 1, 1).is_err());
        assert_eq!(fee(1, 1, u64::MAX).unwrap(), 1);
        assert!(fee(1, 10001, 1).is_err());
    }
}
