#![allow(unexpected_cfgs)]

pub mod args;
mod helpers;
mod instructions;
mod matching;
mod receipt;
mod token;

extern crate alloc;
pub mod contexts;
pub mod state_types;
use anchor_lang::prelude::*;
use args::*;
pub use contexts::*;
pub const ID: Address = anchor_lang::address!("F8q85nw6UScZCBKBJqVAu7ZdYhmT9XRsm6TqZzJqXSNT");

#[error_code]
pub enum RouterError {
    #[msg("arithmetic overflow")]
    Arithmetic,
    #[msg("invalid account")]
    InvalidAccount,
    #[msg("invalid argument")]
    InvalidArgument,
    #[msg("unauthorized")]
    Unauthorized,
    #[msg("slippage exceeded")]
    Slippage,
    #[msg("capacity unavailable")]
    Unavailable,
    #[msg("invalid state")]
    InvalidState,
    #[msg("limit exceeded")]
    LimitExceeded,
}

#[cfg(feature = "local-test")]
pub const CLAIM_SECONDS: i64 = 30;
#[cfg(not(feature = "local-test"))]
pub const CLAIM_SECONDS: i64 = 300;

#[program]
pub mod fossa_router {
    use super::*;
    pub fn initialize_market(ctx: &mut Context<InitializeMarket>) -> Result<()> {
        instructions::market::initialize(ctx)
    }
    pub fn initialize_user(ctx: &mut Context<InitializeUser>) -> Result<()> {
        instructions::user::initialize(ctx)
    }
    pub fn register_offer(ctx: &mut Context<RegisterOffer>, offer: Offer) -> Result<()> {
        instructions::provider::register_offer(ctx, &offer)
    }
    pub fn deposit(ctx: &mut Context<Deposit>, amount: u64) -> Result<()> {
        instructions::user::deposit(ctx, amount)
    }
    pub fn update_offer(ctx: &mut Context<UpdateOffer>, offer: Offer) -> Result<()> {
        instructions::provider::update_offer(ctx, &offer)
    }
    pub fn reserve_booking(
        ctx: &mut Context<ReserveBooking>,
        request: ReserveRequest,
        generation: Option<u64>,
    ) -> Result<()> {
        instructions::booking::reserve(ctx, &request, generation)
    }
    pub fn claim_booking(ctx: &mut Context<ClaimBooking>, claim: Claim) -> Result<()> {
        instructions::booking::claim(ctx, &claim)
    }
    pub fn expire_booking(ctx: &mut Context<ExpireBooking>, generation: Option<u64>) -> Result<()> {
        instructions::booking::expire(ctx, generation)
    }
    pub fn withdraw(ctx: &mut Context<Withdraw>, amount: u64) -> Result<()> {
        instructions::user::withdraw(ctx, amount)
    }
    pub fn release_booking(
        ctx: &mut Context<ReleaseBooking>,
        generation: Option<u64>,
    ) -> Result<()> {
        instructions::booking::release(ctx, generation)
    }
    pub fn close_booking(ctx: &mut Context<CloseBooking>) -> Result<()> {
        instructions::booking::close(ctx)
    }
    pub fn initialize_model(
        ctx: &mut Context<InitializeModel>,
        model_id: Address,
        limit: u32,
        metadata: Metadata,
    ) -> Result<()> {
        instructions::market::initialize_model(ctx, model_id, limit, &metadata)
    }
    pub fn initialize_provider(
        ctx: &mut Context<InitializeProvider>,
        metadata: Metadata,
    ) -> Result<()> {
        instructions::provider::initialize(ctx, &metadata)
    }
    pub fn update_profile(ctx: &mut Context<UpdateProfile>, metadata: Metadata) -> Result<()> {
        instructions::provider::update_profile(ctx, &metadata)
    }
    pub fn update_model_metadata(
        ctx: &mut Context<UpdateModelMetadata>,
        metadata: Metadata,
    ) -> Result<()> {
        instructions::market::update_metadata(ctx, &metadata)
    }
    pub fn initialize_receipt(ctx: &mut Context<InitializeReceipt>) -> Result<()> {
        receipt::initialize(ctx)
    }
    pub fn initialize_list(ctx: &mut Context<InitializeList>, config: ListInit) -> Result<()> {
        instructions::list::initialize(ctx, &config)
    }
    pub fn update_list(
        ctx: &mut Context<UpdateList>,
        provider: Address,
        enabled: bool,
    ) -> Result<()> {
        instructions::list::update(ctx, provider, enabled)
    }
    pub fn close_list(ctx: &mut Context<CloseList>) -> Result<()> {
        instructions::list::close(ctx)
    }
    pub fn grow_books(ctx: &mut Context<GrowBooks>, limit: u32) -> Result<()> {
        instructions::market::grow(ctx, limit)
    }
    pub fn initialize_list_metadata(
        ctx: &mut Context<InitializeListMetadata>,
        metadata: Metadata,
    ) -> Result<()> {
        instructions::list_metadata::initialize(ctx, &metadata)
    }
    pub fn update_list_metadata(
        ctx: &mut Context<UpdateListMetadata>,
        metadata: Metadata,
    ) -> Result<()> {
        instructions::list_metadata::update(ctx, &metadata)
    }
    pub fn configure_fees(ctx: &mut Context<ConfigureFees>, config: FeeConfig) -> Result<()> {
        instructions::fees::configure(ctx, &config)
    }
    pub fn configure_list_fees(ctx: &mut Context<ConfigureListFees>, fees: ListFees) -> Result<()> {
        instructions::fees::configure_list(ctx, fees)
    }
    pub fn initialize_model_group(
        ctx: &mut Context<InitializeModelGroup>,
        group_id: Address,
        config: GroupInit,
    ) -> Result<()> {
        instructions::reserve::initialize_group(ctx, group_id, &config)
    }
    pub fn assign_model_group(ctx: &mut Context<AssignModelGroup>) -> Result<()> {
        instructions::reserve::assign(ctx)
    }
    pub fn update_group_metadata(
        ctx: &mut Context<UpdateGroupMetadata>,
        metadata: TokenMetadata,
    ) -> Result<()> {
        instructions::reserve::update_metadata(ctx, &metadata)
    }
    pub fn configure_reserve(
        ctx: &mut Context<ConfigureReserve>,
        config: ReserveConfig,
    ) -> Result<()> {
        instructions::reserve::configure(ctx, config)
    }
    pub fn redeem_reserve(ctx: &mut Context<RedeemReserve>, redeem: Redeem) -> Result<()> {
        instructions::reserve::redeem(ctx, redeem)
    }
    pub fn collect_reserve_tax(ctx: &mut Context<CollectReserveTax>) -> Result<()> {
        instructions::reserve::collect(ctx)
    }
    pub fn sweep_reserve(ctx: &mut Context<SweepReserve>) -> Result<()> {
        instructions::reserve::sweep_empty(ctx)
    }
    pub fn configure_dex(ctx: &mut Context<ConfigureDex>) -> Result<()> {
        instructions::fees::configure_dex(ctx)
    }
    pub fn rotate_council(ctx: &mut Context<RotateCouncil>) -> Result<()> {
        instructions::fees::rotate_council(ctx)
    }
}
