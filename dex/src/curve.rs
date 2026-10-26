use crate::math::{add, check, input, mul_div, narrow, output, sqrt, sub, Result};
use pinocchio::error::ProgramError;

#[derive(Clone, Copy)]
pub struct Curve {
    pub kind: u8,
    pub initial: u64,
    pub virtual_a: u64,
    pub virtual_b: u64,
}
impl Curve {
    fn sold(self, a: u64) -> Result<u64> {
        sub(self.initial, a)
    }
    fn scaled(self, n: u128, ceil: bool) -> Result<u64> {
        let value = n
            .checked_mul(u128::from(self.virtual_b))
            .ok_or(ProgramError::ArithmeticOverflow)?;
        let denominator = u128::from(self.virtual_a) * 2;
        check(denominator > 0, 3)?;
        narrow(value / denominator + u128::from(ceil && value % denominator != 0))
    }
    pub fn out(self, a: u64, b: u64, a_to_b: bool, amount: u64) -> Result<u64> {
        check(amount > 0, 3)?;
        match self.kind {
            0 => {
                if a_to_b {
                    output(add(a, self.virtual_a)?, add(b, self.virtual_b)?, amount)
                } else {
                    output(add(b, self.virtual_b)?, add(a, self.virtual_a)?, amount)
                }
            }
            1 => {
                if a_to_b {
                    mul_div(amount, self.virtual_b, self.virtual_a, false)
                } else {
                    mul_div(amount, self.virtual_a, self.virtual_b, false)
                }
            }
            2 => {
                let sold = self.sold(a)?;
                if a_to_b {
                    check(amount <= sold, 5)?;
                    self.scaled(
                        u128::from(sold) * u128::from(sold)
                            - u128::from(sold - amount) * u128::from(sold - amount),
                        false,
                    )
                } else {
                    let budget = u128::from(amount)
                        .checked_mul(u128::from(self.virtual_a) * 2)
                        .ok_or(ProgramError::ArithmeticOverflow)?
                        / u128::from(self.virtual_b);
                    let square = (u128::from(sold) * u128::from(sold))
                        .checked_add(budget)
                        .ok_or(ProgramError::ArithmeticOverflow)?;
                    sub(sqrt(square), sold)
                }
            }
            _ => Err(ProgramError::Custom(3)),
        }
    }
    pub fn input(self, a: u64, b: u64, a_to_b: bool, amount: u64) -> Result<u64> {
        check(amount > 0, 3)?;
        match self.kind {
            0 => {
                if a_to_b {
                    input(add(a, self.virtual_a)?, add(b, self.virtual_b)?, amount)
                } else {
                    input(add(b, self.virtual_b)?, add(a, self.virtual_a)?, amount)
                }
            }
            1 => {
                if a_to_b {
                    mul_div(amount, self.virtual_a, self.virtual_b, true)
                } else {
                    mul_div(amount, self.virtual_b, self.virtual_a, true)
                }
            }
            2 => {
                let sold = self.sold(a)?;
                if a_to_b {
                    let value = u128::from(amount)
                        .checked_mul(u128::from(self.virtual_a) * 2)
                        .ok_or(ProgramError::ArithmeticOverflow)?;
                    let q = u128::from(self.virtual_b);
                    let decrease = value / q + u128::from(value % q != 0);
                    let remaining = (u128::from(sold) * u128::from(sold))
                        .checked_sub(decrease)
                        .ok_or(ProgramError::ArithmeticOverflow)?;
                    sub(sold, sqrt(remaining))
                } else {
                    let end = add(sold, amount)?;
                    self.scaled(
                        u128::from(end) * u128::from(end) - u128::from(sold) * u128::from(sold),
                        true,
                    )
                }
            }
            _ => Err(ProgramError::Custom(3)),
        }
    }
    pub fn price(self, a: u64, b: u64) -> Result<(u64, u64)> {
        match self.kind {
            0 => Ok((add(b, self.virtual_b)?, add(a, self.virtual_a)?)),
            1 => Ok((self.virtual_b, self.virtual_a)),
            2 => Ok((
                narrow(u128::from(self.virtual_b) * u128::from(self.sold(a)?))?,
                self.virtual_a,
            )),
            _ => Err(ProgramError::Custom(3)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_and_linear_rounding_has_no_profitable_round_trip() {
        for kind in [1, 2] {
            let c = Curve {
                kind,
                initial: 1_000_000,
                virtual_a: 10_000,
                virtual_b: 7,
            };
            for sold in [0, 1, 100, 10_000, 999_999] {
                let a = c.initial - sold;
                for q in [1, 2, 100, 10_000] {
                    let out = c.out(a, 1_000_000, false, q).unwrap();
                    if out == 0 || out >= a {
                        continue;
                    }
                    assert!(c.input(a, 1_000_000, false, out).unwrap() <= q);
                    assert!(c.out(a - out, 1_000_000, true, out).unwrap() <= q);
                    let quote = c.out(a - out, 1_000_000, true, out).unwrap();
                    if quote > 0 {
                        let required = c.input(a - out, 1_000_000, true, quote).unwrap();
                        assert!(required <= out);
                        assert!(c.out(a - out, 1_000_000, true, required).unwrap() >= quote);
                        if required > 1 {
                            assert!(c.out(a - out, 1_000_000, true, required - 1).unwrap() < quote);
                        }
                    }
                }
            }
        }
    }
}
