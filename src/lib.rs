#![cfg_attr(not(any(test, feature = "no-entrypoint")), no_std)]
#![allow(unexpected_cfgs)]

pub mod abi;
mod helpers;
mod instructions;
mod matching;
#[cfg(feature = "legacy-migration")]
mod migration;
mod receipt;
mod state;
mod token;

use helpers::err;
use pinocchio::{AccountView, Address, ProgramResult};

#[cfg(not(any(test, feature = "no-entrypoint")))]
pinocchio::program_entrypoint!(process_instruction, 32);
#[cfg(not(any(test, feature = "no-entrypoint")))]
pinocchio::default_allocator!();
#[cfg(not(any(test, feature = "no-entrypoint")))]
pinocchio::nostd_panic_handler!();

#[cfg(feature = "local-test")]
pub const CLAIM_SECONDS: i64 = 30;
#[cfg(not(feature = "local-test"))]
pub const CLAIM_SECONDS: i64 = 300;

pub fn process_instruction(
    program: &Address,
    accounts: &mut [AccountView],
    data: &[u8],
) -> ProgramResult {
    let (&op, p) = data.split_first().ok_or(err(3))?;
    match op {
        0 => instructions::market::initialize(program, accounts, p),
        1 => instructions::user::initialize(program, accounts, p),
        2 => instructions::provider::register_offer(program, accounts, p),
        3 => instructions::user::deposit(program, accounts, p),
        5 => instructions::provider::update_offer(program, accounts, p),
        6 => instructions::booking::reserve(program, accounts, p),
        7 => instructions::booking::claim(program, accounts, p),
        8 | 15 => instructions::booking::release(program, accounts, op, p),
        10 => instructions::user::withdraw(program, accounts, p),
        17 => instructions::booking::close(program, accounts, p),
        18 => instructions::market::initialize_model(program, accounts, p),
        19 => instructions::provider::initialize(program, accounts, p),
        21 => instructions::provider::update_profile(program, accounts, p),
        22 => instructions::market::update_metadata(program, accounts, p),
        26 => receipt::initialize(program, accounts, p),
        28 => instructions::list::initialize(program, accounts, p),
        29 => instructions::list::update(program, accounts, p),
        30 => instructions::list::close(program, accounts, p),
        32 => instructions::market::grow(program, accounts, p),
        33 => instructions::list_metadata::initialize(program, accounts, p),
        34 => instructions::list_metadata::update(program, accounts, p),
        35 => instructions::fees::configure(program, accounts, p),
        36 => instructions::fees::configure_list(program, accounts, p),
        37 => instructions::reserve::activate(program, accounts, p),
        38 => instructions::reserve::configure(program, accounts, p),
        39 => instructions::reserve::redeem(program, accounts, p),
        40 => instructions::reserve::collect(program, accounts, p),
        41 => instructions::reserve::sweep_empty(program, accounts, p),
        42 => instructions::fees::configure_dex(program, accounts, p),
        43 => instructions::fees::rotate_admin(program, accounts, p),
        #[cfg(feature = "legacy-migration")]
        31 => migration::sweep(program, accounts, p),
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

#[cfg(test)]
#[path = "../tests/unit/fees.rs"]
mod fee_tests;
