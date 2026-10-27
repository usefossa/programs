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

extern crate alloc;
mod contexts;
mod cross_program;
mod schemas;
mod wire;
use anchor_lang::prelude::*;
use contexts::*;
use wire::Payload;
pub const ID: Address = Address::new_from_array([8; 32]);

#[cfg(feature = "local-test")]
pub const CLAIM_SECONDS: i64 = 30;
#[cfg(not(feature = "local-test"))]
pub const CLAIM_SECONDS: i64 = 300;

#[program]
pub mod fossa_router {
    use super::*;
    #[discrim = 0]
    pub fn initialize_market(ctx: &mut Context<InitializeMarket>, p: Payload) -> Result<()> {
        instructions::market::initialize(ctx, &p.0)
    }
    #[discrim = 1]
    pub fn initialize_user(ctx: &mut Context<InitializeUser>, p: Payload) -> Result<()> {
        instructions::user::initialize(ctx, &p.0)
    }
    #[discrim = 2]
    pub fn register_offer(ctx: &mut Context<RegisterOffer>, p: Payload) -> Result<()> {
        instructions::provider::register_offer(ctx, &p.0)
    }
    #[discrim = 3]
    pub fn deposit(ctx: &mut Context<Deposit>, p: Payload) -> Result<()> {
        instructions::user::deposit(ctx, &p.0)
    }
    #[discrim = 5]
    pub fn update_offer(ctx: &mut Context<UpdateOffer>, p: Payload) -> Result<()> {
        instructions::provider::update_offer(ctx, &p.0)
    }
    #[discrim = 6]
    pub fn reserve_booking(ctx: &mut Context<ReserveBooking>, p: Payload) -> Result<()> {
        instructions::booking::reserve(ctx, &p.0)
    }
    #[discrim = 7]
    pub fn claim_booking(ctx: &mut Context<ClaimBooking>, p: Payload) -> Result<()> {
        instructions::booking::claim(ctx, &p.0)
    }
    #[discrim = 8]
    pub fn expire_booking(ctx: &mut Context<ExpireBooking>, p: Payload) -> Result<()> {
        helpers::require(ctx.remaining_accounts()?.is_empty(), 3)?;
        instructions::booking::release(
            ctx.program_id,
            &mut [
                *ctx.accounts.m.account(),
                *ctx.accounts.mo.account(),
                *ctx.accounts.c.account(),
                *ctx.accounts.u.account(),
                *ctx.accounts.b.account(),
            ],
            8,
            &p.0,
        )
    }
    #[discrim = 10]
    pub fn withdraw(ctx: &mut Context<Withdraw>, p: Payload) -> Result<()> {
        instructions::user::withdraw(ctx, &p.0)
    }
    #[discrim = 15]
    pub fn release_booking(ctx: &mut Context<ReleaseBooking>, p: Payload) -> Result<()> {
        helpers::require(ctx.remaining_accounts()?.is_empty(), 3)?;
        instructions::booking::release(
            ctx.program_id,
            &mut [
                *ctx.accounts.m.account(),
                *ctx.accounts.mo.account(),
                *ctx.accounts.c.account(),
                *ctx.accounts.u.account(),
                *ctx.accounts.b.account(),
                *ctx.accounts.provider.account(),
            ],
            15,
            &p.0,
        )
    }
    #[discrim = 17]
    pub fn close_booking(ctx: &mut Context<CloseBooking>, p: Payload) -> Result<()> {
        instructions::booking::close(ctx, &p.0)
    }
    #[discrim = 18]
    pub fn initialize_model(ctx: &mut Context<InitializeModel>, p: Payload) -> Result<()> {
        instructions::market::initialize_model(ctx, &p.0)
    }
    #[discrim = 19]
    pub fn initialize_provider(ctx: &mut Context<InitializeProvider>, p: Payload) -> Result<()> {
        instructions::provider::initialize(ctx, &p.0)
    }
    #[discrim = 21]
    pub fn update_profile(ctx: &mut Context<UpdateProfile>, p: Payload) -> Result<()> {
        instructions::provider::update_profile(ctx, &p.0)
    }
    #[discrim = 22]
    pub fn update_model_metadata(ctx: &mut Context<UpdateModelMetadata>, p: Payload) -> Result<()> {
        instructions::market::update_metadata(ctx, &p.0)
    }
    #[discrim = 26]
    pub fn initialize_receipt(ctx: &mut Context<InitializeReceipt>, p: Payload) -> Result<()> {
        receipt::initialize(ctx, &p.0)
    }
    #[discrim = 28]
    pub fn initialize_list(ctx: &mut Context<InitializeList>, p: Payload) -> Result<()> {
        instructions::list::initialize(ctx, &p.0)
    }
    #[discrim = 29]
    pub fn update_list(ctx: &mut Context<UpdateList>, p: Payload) -> Result<()> {
        instructions::list::update(ctx, &p.0)
    }
    #[discrim = 30]
    pub fn close_list(ctx: &mut Context<CloseList>, p: Payload) -> Result<()> {
        instructions::list::close(ctx, &p.0)
    }
    #[discrim = 32]
    pub fn grow_books(ctx: &mut Context<GrowBooks>, p: Payload) -> Result<()> {
        instructions::market::grow(ctx, &p.0)
    }
    #[discrim = 33]
    pub fn initialize_list_metadata(
        ctx: &mut Context<InitializeListMetadata>,
        p: Payload,
    ) -> Result<()> {
        instructions::list_metadata::initialize(ctx, &p.0)
    }
    #[discrim = 34]
    pub fn update_list_metadata(ctx: &mut Context<UpdateListMetadata>, p: Payload) -> Result<()> {
        instructions::list_metadata::update(ctx, &p.0)
    }
    #[discrim = 35]
    pub fn configure_fees(ctx: &mut Context<ConfigureFees>, p: Payload) -> Result<()> {
        instructions::fees::configure(ctx, &p.0)
    }
    #[discrim = 36]
    pub fn configure_list_fees(ctx: &mut Context<ConfigureListFees>, p: Payload) -> Result<()> {
        instructions::fees::configure_list(ctx, &p.0)
    }
    #[discrim = 37]
    pub fn activate_reserve(ctx: &mut Context<ActivateReserve>, p: Payload) -> Result<()> {
        instructions::reserve::activate(ctx, &p.0)
    }
    #[discrim = 38]
    pub fn configure_reserve(ctx: &mut Context<ConfigureReserve>, p: Payload) -> Result<()> {
        instructions::reserve::configure(ctx, &p.0)
    }
    #[discrim = 39]
    pub fn redeem_reserve(ctx: &mut Context<RedeemReserve>, p: Payload) -> Result<()> {
        instructions::reserve::redeem(ctx, &p.0)
    }
    #[discrim = 40]
    pub fn collect_reserve_tax(ctx: &mut Context<CollectReserveTax>, p: Payload) -> Result<()> {
        instructions::reserve::collect(ctx, &p.0)
    }
    #[discrim = 41]
    pub fn sweep_reserve(ctx: &mut Context<SweepReserve>, p: Payload) -> Result<()> {
        instructions::reserve::sweep_empty(ctx, &p.0)
    }
    #[discrim = 42]
    pub fn configure_dex(ctx: &mut Context<ConfigureDex>, p: Payload) -> Result<()> {
        instructions::fees::configure_dex(ctx, &p.0)
    }
    #[discrim = 43]
    pub fn rotate_admin(ctx: &mut Context<RotateAdmin>, p: Payload) -> Result<()> {
        instructions::fees::rotate_admin(ctx, &p.0)
    }
    #[cfg(feature = "legacy-migration")]
    #[discrim = 31]
    pub fn sweep_legacy(ctx: &mut Context<SweepLegacy>, p: Payload) -> Result<()> {
        migration::sweep(ctx, &p.0)
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
