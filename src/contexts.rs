use crate::schemas::*;
use anchor_lang::prelude::*;

// identity and funding roles may reuse state or token addresses. exemptions keep checked
// data borrows and each handler's original ownership, pda and distinctness requirements.

#[derive(Accounts)]
pub struct InitializeMarket {
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub m: Signer,
    pub admin: Signer,
    pub mint: Account<anchor_spl::mint::Mint>,
}

#[derive(Accounts)]
pub struct InitializeUser {
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub u: Signer,
    pub owner: Signer,
    pub m: Account<MarketState>,
    pub vault: UncheckedAccount,
}

#[derive(Accounts)]
pub struct RegisterOffer {
    pub m: Account<MarketState>,
    pub mo: Account<ModelState>,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub t: UncheckedAccount,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub c: UncheckedAccount,
    pub profile: Account<ProviderState>,
    pub provider: Signer,
    pub payout: UncheckedAccount,
}

#[derive(Accounts)]
pub struct Deposit {
    pub m: Account<MarketState>,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub u: Account<UserState>,
    pub owner: Signer,
    #[account(mut, unsafe(dup))]
    pub source: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub vault: UncheckedAccount,
    pub mint: UncheckedAccount,
    pub token: Program<Token>,
}

#[derive(Accounts)]
pub struct UpdateOffer {
    pub m: Account<MarketState>,
    pub mo: Account<ModelState>,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub t: UncheckedAccount,
    pub signer: Signer,
}

#[derive(Accounts)]
pub struct ReserveBooking {
    pub m: Account<MarketState>,
    pub mo: Account<ModelState>,
    pub t: Account<TermsState>,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub c: UncheckedAccount,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub u: UncheckedAccount,
    pub owner: Signer,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub b: UncheckedAccount,
    pub rent_payer: UncheckedAccount,
    pub list: Account<ListState>,
}

#[derive(Accounts)]
pub struct ClaimBooking {
    pub m: Account<MarketState>,
    pub mo: Account<ModelState>,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub c: UncheckedAccount,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub u: UncheckedAccount,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub b: UncheckedAccount,
    pub provider: Signer,
    #[account(mut, unsafe(dup))]
    pub vault: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub payout: UncheckedAccount,
    pub mint: UncheckedAccount,
    pub authority: UncheckedAccount,
    pub token: Program<Token>,
}

#[derive(Accounts)]
pub struct ExpireBooking {
    pub m: Account<MarketState>,
    pub mo: Account<ModelState>,
    #[account(mut, owner = crate::ID @ ProgramError::Custom(2))]
    pub c: UncheckedAccount,
    #[account(mut, owner = crate::ID @ ProgramError::Custom(2))]
    pub u: UncheckedAccount,
    #[account(mut, owner = crate::ID @ ProgramError::Custom(2))]
    pub b: UncheckedAccount,
}

#[derive(Accounts)]
pub struct Withdraw {
    pub m: Account<MarketState>,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub u: Account<UserState>,
    pub owner: Signer,
    #[account(mut, unsafe(dup))]
    pub vault: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub destination: UncheckedAccount,
    pub mint: UncheckedAccount,
    pub authority: UncheckedAccount,
    pub token: Program<Token>,
}

#[derive(Accounts)]
pub struct ReleaseBooking {
    pub m: Account<MarketState>,
    pub mo: Account<ModelState>,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub c: UncheckedAccount,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub u: UncheckedAccount,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub b: UncheckedAccount,
    pub provider: Signer,
}

#[derive(Accounts)]
pub struct CloseBooking {
    #[account(mut, owner = crate::ID @ ProgramError::Custom(2))]
    pub a: UncheckedAccount,
    #[account(mut)]
    pub recipient: UncheckedAccount,
}

#[derive(Accounts)]
pub struct InitializeModel {
    pub m: Account<MarketState>,
    #[account(mut)]
    pub mo: UncheckedAccount,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub t: Signer,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub c: Signer,
    pub provider: Signer,
    #[account(mut, unsafe(dup))]
    pub payer: Signer,
    pub system: Program<System>,
    pub profile: Account<ProviderState>,
    #[account(mut)]
    pub metadata: UncheckedAccount,
}

#[derive(Accounts)]
pub struct InitializeProvider {
    pub m: Account<MarketState>,
    #[account(mut)]
    pub profile: UncheckedAccount,
    pub provider: Signer,
    #[account(mut, unsafe(dup))]
    pub payer: Signer,
    pub system: Program<System>,
}

#[derive(Accounts)]
pub struct UpdateProfile {
    pub m: Account<MarketState>,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub pr: Account<ProviderState>,
    pub signer: Signer,
}

#[derive(Accounts)]
pub struct UpdateModelMetadata {
    pub m: Account<MarketState>,
    pub mo: Account<ModelState>,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub metadata: UncheckedAccount,
    pub provider: Signer,
}

#[derive(Accounts)]
pub struct InitializeReceipt {
    pub m: Account<MarketState>,
    pub u: Account<UserState>,
    pub owner: Signer,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub b: Signer,
    pub payer: Signer,
}

#[derive(Accounts)]
pub struct InitializeList {
    pub m: Account<MarketState>,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub list: Signer,
    pub authority: Signer,
}

#[derive(Accounts)]
pub struct UpdateList {
    pub m: Account<MarketState>,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub list: UncheckedAccount,
    pub authority: Signer,
}

#[derive(Accounts)]
pub struct CloseList {
    #[account(mut, owner = crate::ID @ ProgramError::Custom(2))]
    pub list: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub authority: Signer,
    #[account(mut, unsafe(dup))]
    pub metadata: UncheckedAccount,
}

#[derive(Accounts)]
pub struct GrowBooks {
    pub m: Account<MarketState>,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub mo: UncheckedAccount,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub t: UncheckedAccount,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub c: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub payer: Signer,
    pub system: Program<System>,
}

#[derive(Accounts)]
pub struct InitializeListMetadata {
    pub m: Account<MarketState>,
    pub list: Account<ListState>,
    #[account(mut, unsafe(dup))]
    pub metadata: UncheckedAccount,
    pub authority: Signer,
    #[account(mut, unsafe(dup))]
    pub payer: Signer,
    pub system: Program<System>,
}

#[derive(Accounts)]
pub struct UpdateListMetadata {
    pub m: Account<MarketState>,
    pub list: Account<ListState>,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub metadata: UncheckedAccount,
    pub authority: Signer,
}

#[derive(Accounts)]
pub struct ConfigureFees {
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub m: UncheckedAccount,
    pub signer: Signer,
    #[account(mut, unsafe(dup))]
    pub payer: Signer,
    pub system: Program<System>,
}

#[derive(Accounts)]
pub struct ConfigureListFees {
    pub m: Account<MarketState>,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub list: UncheckedAccount,
    pub authority: Signer,
    #[account(mut, unsafe(dup))]
    pub payer: Signer,
    pub system: Program<System>,
}

#[derive(Accounts)]
pub struct ActivateReserve {
    #[account(owner = crate::ID @ ProgramError::Custom(2))]
    pub m: UncheckedAccount,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub mo: UncheckedAccount,
    pub admin: Signer,
    #[account(mut, unsafe(dup))]
    pub payer: Signer,
    pub system: Program<System>,
    #[account(mut, unsafe(dup))]
    pub mint: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub backing: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub allocation: UncheckedAccount,
    pub quote: UncheckedAccount,
    pub token: Program<Token>,
    pub t22: Program<Token2022>,
    #[account(executable)]
    pub dex: UncheckedAccount,
    // adapter accounts retain caller flags; configured callee validates its own interface.
    pub curve_payer: UncheckedAccount,
    pub curve_config: UncheckedAccount,
    pub curve_pool: UncheckedAccount,
    pub curve_authority: UncheckedAccount,
    pub curve_mint: UncheckedAccount,
    pub curve_quote: UncheckedAccount,
    pub curve_source: UncheckedAccount,
    pub curve_quote_source: UncheckedAccount,
    pub curve_base_vault: UncheckedAccount,
    pub curve_quote_vault: UncheckedAccount,
    pub curve_lp_mint: UncheckedAccount,
    pub curve_lp: UncheckedAccount,
    pub curve_token: UncheckedAccount,
    pub curve_quote_token: UncheckedAccount,
    pub curve_lp_token: UncheckedAccount,
    pub dex_global: UncheckedAccount,
}

#[derive(Accounts)]
pub struct ConfigureReserve {
    pub m: Account<MarketState>,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub mo: UncheckedAccount,
    pub admin: Signer,
    #[account(mut, unsafe(dup))]
    pub mint: UncheckedAccount,
    pub t22: Program<Token2022>,
}

#[derive(Accounts)]
pub struct RedeemReserve {
    pub m: Account<MarketState>,
    pub mo: Account<ModelState>,
    #[account(mut, unsafe(dup))]
    pub mint: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub source: UncheckedAccount,
    pub holder: Signer,
    #[account(mut, unsafe(dup))]
    pub backing: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub destination: UncheckedAccount,
    pub quote: UncheckedAccount,
    pub token: Program<Token>,
    pub t22: Program<Token2022>,
}

#[derive(Accounts)]
pub struct CollectReserveTax {
    pub m: Account<MarketState>,
    pub mo: Account<ModelState>,
    #[account(mut)]
    pub mint: UncheckedAccount,
    #[account(mut)]
    pub destination: UncheckedAccount,
    pub t22: Program<Token2022>,
}

#[derive(Accounts)]
pub struct SweepReserve {
    pub m: Account<MarketState>,
    pub mo: Account<ModelState>,
    pub mint: UncheckedAccount,
    #[account(mut)]
    pub backing: UncheckedAccount,
    #[account(mut)]
    pub destination: UncheckedAccount,
    pub quote: UncheckedAccount,
    pub token: Program<Token>,
}

#[derive(Accounts)]
pub struct ConfigureDex {
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub m: UncheckedAccount,
    pub admin: Signer,
    #[account(executable)]
    pub dex: UncheckedAccount,
    #[account(owner = dex.address() @ ProgramError::Custom(2))]
    pub config: UncheckedAccount,
}

#[derive(Accounts)]
pub struct RotateAdmin {
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub m: Account<MarketState>,
    pub current: Signer,
    pub next: Signer,
}

#[derive(Accounts)]
#[cfg_attr(not(feature = "legacy-migration"), allow(dead_code))]
pub struct SweepLegacy {
    pub m: Account<MarketState>,
    pub mo: Account<ModelState>,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub t: UncheckedAccount,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub c: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub provider: Signer,
    #[account(mut)]
    pub vault: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub payout: UncheckedAccount,
    pub mint: UncheckedAccount,
    pub authority: UncheckedAccount,
    pub token: Program<Token>,
}
