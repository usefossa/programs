use crate::math::{add, input, output, Result};

#[derive(Clone, Copy)]
pub struct Curve {
    pub initial: u64,
    pub virtual_a: u64,
    pub virtual_b: u64,
}

impl Curve {
    pub fn out(self, a: u64, b: u64, a_to_b: bool, amount: u64) -> Result<u64> {
        let a = add(a, self.virtual_a)?;
        let b = add(b, self.virtual_b)?;
        if a_to_b {
            output(a, b, amount)
        } else {
            output(b, a, amount)
        }
    }

    pub fn input(self, a: u64, b: u64, a_to_b: bool, amount: u64) -> Result<u64> {
        let a = add(a, self.virtual_a)?;
        let b = add(b, self.virtual_b)?;
        if a_to_b {
            input(a, b, amount)
        } else {
            input(b, a, amount)
        }
    }

    pub fn price(self, a: u64, b: u64) -> Result<(u64, u64)> {
        Ok((add(b, self.virtual_b)?, add(a, self.virtual_a)?))
    }
}
