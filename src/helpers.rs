use crate::abi::UNIT;
use pinocchio::{error::ProgramError, Address, ProgramResult};

pub(crate) fn err(n: u32) -> ProgramError {
    ProgramError::Custom(n)
}
pub(crate) fn require(ok: bool, n: u32) -> ProgramResult {
    if ok {
        Ok(())
    } else {
        Err(err(n))
    }
}
pub(crate) fn rd16(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes(b[i..i + 2].try_into().unwrap())
}
pub(crate) fn rd32(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}
pub(crate) fn rd64(b: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(b[i..i + 8].try_into().unwrap())
}
pub(crate) fn ri64(b: &[u8], i: usize) -> i64 {
    i64::from_le_bytes(b[i..i + 8].try_into().unwrap())
}
pub(crate) fn wr32(b: &mut [u8], i: usize, v: u32) {
    b[i..i + 4].copy_from_slice(&v.to_le_bytes());
}
pub(crate) fn wr64(b: &mut [u8], i: usize, v: u64) {
    b[i..i + 8].copy_from_slice(&v.to_le_bytes());
}
pub(crate) fn wi64(b: &mut [u8], i: usize, v: i64) {
    b[i..i + 8].copy_from_slice(&v.to_le_bytes());
}
pub(crate) fn addr(b: &[u8], i: usize) -> Address {
    Address::new_from_array(b[i..i + 32].try_into().unwrap())
}
pub(crate) fn add(a: u64, b: u64) -> Result<u64, ProgramError> {
    a.checked_add(b).ok_or(err(1))
}
pub(crate) fn sub(a: u64, b: u64) -> Result<u64, ProgramError> {
    a.checked_sub(b).ok_or(err(1))
}
pub(crate) fn inc(a: u32) -> Result<u32, ProgramError> {
    a.checked_add(1).ok_or(err(1))
}
pub(crate) fn dec(a: u32) -> Result<u32, ProgramError> {
    a.checked_sub(1).ok_or(err(1))
}
#[allow(
    clippy::manual_div_ceil,
    reason = "bounded addition removes sbf remainder reconstruction"
)]
pub(crate) fn fee(tokens: u32, rate: u32) -> u64 {
    // maximum u32 product plus 999_999 fits u64; all caller operands have those bounds.
    (u64::from(tokens) * u64::from(rate) + UNIT - 1) / UNIT
}
pub(crate) fn collateral(exposure: u64) -> Result<u64, ProgramError> {
    add(exposure, exposure.div_ceil(100))
}
