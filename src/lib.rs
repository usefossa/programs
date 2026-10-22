#![cfg_attr(not(any(test, feature = "no-entrypoint")), no_std)]
#![allow(unexpected_cfgs)]

pub mod abi;
mod helpers;
mod instructions;
mod matching;
#[cfg(feature = "receipt-prototype")]
mod receipt;
mod state;
mod token;

use helpers::err;
use pinocchio::{AccountView, Address, ProgramResult};

#[cfg(not(any(test, feature = "no-entrypoint")))]
pinocchio::program_entrypoint!(process_instruction, 12);
#[cfg(not(any(test, feature = "no-entrypoint")))]
pinocchio::default_allocator!();
#[cfg(not(any(test, feature = "no-entrypoint")))]
pinocchio::nostd_panic_handler!();

#[cfg(feature = "local-test")]
pub const CLAIM_SECONDS: i64 = 30;
#[cfg(not(feature = "local-test"))]
pub const CLAIM_SECONDS: i64 = 300;
#[cfg(feature = "local-test")]
pub const DISPUTE_SECONDS: i64 = 8;
#[cfg(not(feature = "local-test"))]
pub const DISPUTE_SECONDS: i64 = 3_600;
#[cfg(feature = "local-test")]
pub const UNBOND_SECONDS: i64 = 5;
#[cfg(not(feature = "local-test"))]
pub const UNBOND_SECONDS: i64 = 3_600;

pub fn process_instruction(
    program: &Address,
    accounts: &mut [AccountView],
    data: &[u8],
) -> ProgramResult {
    let (&op, p) = data.split_first().ok_or(err(3))?;
    #[cfg(feature = "receipt-prototype")]
    if op >= 23 {
        return receipt::process(program, accounts, op, p);
    }
    match op {
        0 => instructions::market::initialize(program, accounts, p),
        1 => instructions::user::initialize(program, accounts, p),
        2 => instructions::provider::register_offer(program, accounts, p),
        3 => instructions::user::deposit(program, accounts, p),
        4 => instructions::bond::fund(program, accounts, p),
        5 | 16 => instructions::provider::update_offer(program, accounts, op, p),
        6 => instructions::booking::reserve(program, accounts, p),
        7 => instructions::booking::claim(program, accounts, p),
        8 | 15 => instructions::booking::release(program, accounts, op, p),
        9 => instructions::booking::finalize(program, accounts, p),
        10 => instructions::user::withdraw(program, accounts, p),
        11 => instructions::dispute::open(program, accounts, p),
        12 => instructions::dispute::resolve(program, accounts, p),
        13 => instructions::bond::request_unbond(program, accounts, p),
        14 => instructions::bond::execute_unbond(program, accounts, p),
        17 => instructions::booking::close(program, accounts, p),
        18 => instructions::market::initialize_model(program, accounts, p),
        19 => instructions::provider::initialize(program, accounts, p),
        20 | 21 => instructions::provider::update_profile(program, accounts, op, p),
        22 => instructions::market::update_metadata(program, accounts, p),
        _ => Err(err(3)),
    }
}

#[cfg(test)]
#[path = "../tests/unit/core.rs"]
mod tests;

#[cfg(test)]
#[path = "../tests/unit/matching.rs"]
mod matching_tests;

#[cfg(test)]
#[path = "../tests/unit/validation.rs"]
mod validation_tests;
