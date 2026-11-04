use crate::RouterError;
use anchor_lang::prelude::{ProgramError, ProgramResult};

const UNIT: u64 = 1_000_000;

pub(crate) fn require(ok: bool, error: RouterError) -> ProgramResult {
    anchor_lang::require!(ok, ProgramError::from(error));
    Ok(())
}
pub(crate) fn bps_ceil(amount: u64, bps: u16) -> u64 {
    amount / 10_000 * u64::from(bps) + (amount % 10_000 * u64::from(bps)).div_ceil(10_000)
}
pub(crate) fn fee_budget(total: u64, bps: u16) -> u64 {
    let denominator = 10_000 + u64::from(bps);
    total / denominator * 10_000 + total % denominator * 10_000 / denominator
}
pub(crate) fn add(a: u64, b: u64) -> Result<u64, ProgramError> {
    a.checked_add(b).ok_or(RouterError::Arithmetic.into())
}
pub(crate) fn sub(a: u64, b: u64) -> Result<u64, ProgramError> {
    a.checked_sub(b).ok_or(RouterError::Arithmetic.into())
}
pub(crate) fn inc(a: u32) -> Result<u32, ProgramError> {
    a.checked_add(1).ok_or(RouterError::Arithmetic.into())
}
pub(crate) fn dec(a: u32) -> Result<u32, ProgramError> {
    a.checked_sub(1).ok_or(RouterError::Arithmetic.into())
}
#[allow(
    clippy::manual_div_ceil,
    reason = "bounded addition removes sbf remainder reconstruction"
)]
pub(crate) fn fee(tokens: u32, rate: u32) -> u64 {
    // maximum u32 product plus 999_999 fits u64; all caller operands have those bounds.
    (u64::from(tokens) * u64::from(rate) + UNIT - 1) / UNIT
}
