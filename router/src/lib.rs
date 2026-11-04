#![allow(unexpected_cfgs)]

pub mod args;
mod helpers;
mod instructions;
mod matching;
mod receipt;
mod token;

extern crate alloc;
pub mod contexts;
pub mod scoped_contexts;
pub mod state_types;
use anchor_lang::prelude::*;
use args::*;
pub use contexts::*;
pub use scoped_contexts::*;
pub const ID: Address = anchor_lang::address!("routeppVPc5WdqJ2s4foU8QMFYVfQ3HjmucMycBED41");

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
    #[msg("wallet booking storage full")]
    WalletBookingsFull,
    #[msg("provider limit page required")]
    ProviderLimitPageRequired,
    #[msg("selected provider limit page must be writable")]
    ProviderLimitPageReadonly,
}

#[cfg(feature = "local-test")]
pub const CLAIM_SECONDS: i64 = 30;
#[cfg(not(feature = "local-test"))]
pub const CLAIM_SECONDS: i64 = 300;

#[program]
pub mod fossa_router {
    use super::*;
    pub fn activate_provider_limits(ctx: &mut Context<ActivateProviderLimits>) -> Result<()> {
        instructions::scoped::activate_provider_limits(ctx)
    }
    pub fn initialize_provider_limit_page(
        ctx: &mut Context<InitializeProviderLimitPage>,
        limit: u32,
        page: u8,
    ) -> Result<()> {
        instructions::scoped::initialize_provider_limit_page(ctx, limit, page)
    }
    pub fn grow_provider_limit_page(
        ctx: &mut Context<GrowProviderLimitPage>,
        limit: u32,
    ) -> Result<()> {
        instructions::scoped::grow_provider_limit_page(ctx, limit)
    }
    pub fn initialize_provider_limits(
        ctx: &mut Context<InitializeProviderLimits>,
        limits: ProviderLimits,
    ) -> Result<()> {
        instructions::scoped::initialize_provider_limits(ctx, &limits)
    }
    pub fn configure_provider_limits(
        ctx: &mut Context<ConfigureProviderLimits>,
        limits: ProviderLimits,
    ) -> Result<()> {
        instructions::scoped::configure_provider_limits(ctx, &limits)
    }
    pub fn activate_list_routing(ctx: &mut Context<ActivateListRouting>) -> Result<()> {
        instructions::scoped::activate(ctx)
    }
    pub fn initialize_provider_policy(
        ctx: &mut Context<InitializeProviderPolicy>,
        limits: BookingLimits,
    ) -> Result<()> {
        instructions::scoped::initialize_policy(ctx, &limits)
    }
    pub fn begin_provider_policy_update(ctx: &mut Context<ConfigureProviderPolicy>) -> Result<()> {
        instructions::scoped::begin_policy_update(ctx)
    }
    pub fn commit_provider_policy_update(
        ctx: &mut Context<ConfigureProviderPolicy>,
        limits: BookingLimits,
    ) -> Result<()> {
        instructions::scoped::commit_policy(ctx, &limits)
    }
    pub fn initialize_list_routing(
        ctx: &mut Context<InitializeListRouting>,
        limit: u32,
    ) -> Result<()> {
        instructions::scoped::initialize_routing(ctx, limit)
    }
    pub fn grow_list_routing(ctx: &mut Context<GrowListRouting>, limit: u32) -> Result<()> {
        instructions::scoped::grow_routing(ctx, limit)
    }
    pub fn update_routing_list(
        ctx: &mut Context<UpdateRoutingList>,
        provider: Address,
        enabled: bool,
    ) -> Result<()> {
        instructions::scoped::update_list(ctx, provider, enabled)
    }
    pub fn enable_policy_mirror(ctx: &mut Context<EnablePolicyMirror>) -> Result<()> {
        instructions::scoped::enable_mirror(ctx)
    }
    pub fn freeze_policy_mirror(ctx: &mut Context<FreezePolicyMirror>) -> Result<()> {
        instructions::scoped::freeze_mirror(ctx)
    }
    pub fn close_list_routing(ctx: &mut Context<CloseListRouting>) -> Result<()> {
        instructions::scoped::close_routing(ctx)
    }
    pub fn initialize_model_capacity(
        ctx: &mut Context<InitializeModelCapacity>,
        total: u32,
    ) -> Result<()> {
        instructions::scoped::initialize_capacity(ctx, total)
    }
    pub fn configure_model_capacity(
        ctx: &mut Context<ConfigureModelCapacity>,
        total: u32,
    ) -> Result<()> {
        instructions::scoped::configure_capacity(ctx, total)
    }
    pub fn sync_legacy_capacity(ctx: &mut Context<SyncLegacyCapacity>) -> Result<()> {
        instructions::scoped::sync_legacy(ctx)
    }
    pub fn initialize_list_books(ctx: &mut Context<InitializeListBooks>, limit: u32) -> Result<()> {
        instructions::scoped::initialize_books(ctx, limit)
    }
    pub fn grow_list_books(ctx: &mut Context<GrowListBooks>, limit: u32) -> Result<()> {
        instructions::scoped::grow_books(ctx, limit)
    }
    pub fn close_list_books(ctx: &mut Context<CloseListBooks>) -> Result<()> {
        instructions::scoped::close_books(ctx)
    }
    pub fn set_list_offer(ctx: &mut Context<SetListOffer>, offer: Offer) -> Result<()> {
        instructions::scoped::set_offer(ctx, &offer)
    }
    pub fn prune_list_offer(ctx: &mut Context<PruneListOffer>) -> Result<()> {
        instructions::scoped::prune_offer(ctx)
    }
    pub fn initialize_provider_wallet(
        ctx: &mut Context<InitializeProviderWallet>,
        limit: u32,
    ) -> Result<()> {
        instructions::scoped::initialize_wallet(ctx, limit)
    }
    pub fn grow_provider_wallet(ctx: &mut Context<GrowProviderWallet>, limit: u32) -> Result<()> {
        instructions::scoped::grow_wallet(ctx, limit)
    }
    pub fn prune_provider_wallet(ctx: &mut Context<PruneProviderWallet>) -> Result<()> {
        instructions::scoped::prune_wallet(ctx)
    }
    pub fn close_provider_wallet(ctx: &mut Context<CloseProviderWallet>) -> Result<()> {
        instructions::scoped::close_wallet(ctx)
    }
    pub fn reserve_list_booking(
        ctx: &mut Context<ReserveListBooking>,
        request: ReserveRequest,
        generation: Option<u64>,
        minimum_wallet_concurrency: Option<u32>,
    ) -> Result<()> {
        instructions::scoped::reserve(ctx, &request, generation, minimum_wallet_concurrency)
    }
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
    pub fn claim_list_booking(ctx: &mut Context<ClaimListBooking>, claim: Claim) -> Result<()> {
        instructions::booking::claim_list(ctx, &claim)
    }
    pub fn release_list_booking(
        ctx: &mut Context<ReleaseListBooking>,
        generation: Option<u64>,
    ) -> Result<()> {
        instructions::booking::release_list(ctx, generation)
    }
    pub fn expire_list_booking(
        ctx: &mut Context<ExpireListBooking>,
        generation: Option<u64>,
    ) -> Result<()> {
        instructions::booking::expire_list(ctx, generation)
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
