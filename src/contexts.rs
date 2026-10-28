use crate::state_types::*;
use anchor_lang::accounts::Slab;
use anchor_lang::prelude::*;
use anchor_spl::{mint, token};

#[derive(Accounts)]
pub struct InitializeMarket {
    #[account(mut)]
    pub admin: Signer,
    pub mint: Account<anchor_spl::mint::Mint>,
    #[account(init, payer = admin)]
    pub market: Account<Market>,
    pub system_program: Program<System>,
}

#[derive(Accounts)]
pub struct InitializeUser {
    #[account(mut)]
    pub owner: Signer,
    pub market: Account<Market>,
    #[account(init, payer = owner)]
    pub user: Account<User>,
    #[account(seeds = [b"vault", user.address().as_ref()], bump)]
    pub authority: UncheckedAccount,
    #[account(
        token::authority = authority,
        constraint = vault.mint() == &market.mint @ crate::RouterError::InvalidAccount,
        constraint = !vault.has_delegate() && !vault.has_close_authority() && !vault.is_native() @ crate::RouterError::InvalidAccount
    )]
    pub vault: Account<anchor_spl::token::TokenAccount>,
    pub system_program: Program<System>,
}

#[derive(Accounts)]
pub struct RegisterOffer {
    pub market: Account<Market>,
    #[account(
        seeds = [b"model", market.address().as_ref(), model.model_id.as_ref()],
        bump = model.bump,
        constraint = model.market == *market.address() @ crate::RouterError::InvalidAccount
    )]
    pub model: Account<Model>,
    #[account(mut, owner = crate::ID @ crate::RouterError::InvalidAccount)]
    pub terms: Slab<Terms, TermsEntry>,
    #[account(mut, owner = crate::ID @ crate::RouterError::InvalidAccount)]
    pub capacity: Slab<Capacity, CapacityEntry>,
    #[account(
        seeds = [b"provider", market.address().as_ref(), provider.address().as_ref()],
        bump = profile.bump,
        constraint = profile.market == *market.address() @ crate::RouterError::Unauthorized,
        constraint = profile.authority == *provider.address() @ crate::RouterError::Unauthorized
    )]
    pub profile: Account<Provider>,
    pub provider: Signer,
    #[account(
        constraint = payout.mint() == &market.mint @ crate::RouterError::InvalidAccount,
        constraint = payout.owner() == provider.address() @ crate::RouterError::InvalidAccount
    )]
    pub payout: Account<anchor_spl::token::TokenAccount>,
}

#[derive(Accounts)]
pub struct Deposit {
    pub market: Account<Market>,
    #[account(
        mut,
        constraint = user.owner == *owner.address() @ crate::RouterError::InvalidAccount,
        constraint = user.market == *market.address() @ crate::RouterError::InvalidAccount
    )]
    pub user: Account<User>,
    pub owner: Signer,
    #[account(
        mut,
        token::mint = mint,
        token::authority = owner
    )]
    pub source: Account<anchor_spl::token::TokenAccount>,
    #[account(seeds = [b"vault", user.address().as_ref()], bump = user.bump)]
    pub authority: UncheckedAccount,
    #[account(
        mut,
        address = user.vault @ crate::RouterError::InvalidAccount,
        token::mint = mint,
        token::authority = authority,
        constraint = !vault.has_delegate() && !vault.has_close_authority() && !vault.is_native() @ crate::RouterError::InvalidAccount
    )]
    pub vault: Account<anchor_spl::token::TokenAccount>,
    #[account(address = market.mint @ crate::RouterError::InvalidAccount)]
    pub mint: Account<anchor_spl::mint::Mint>,
    pub token: Program<Token>,
}

#[derive(Accounts)]
pub struct UpdateOffer {
    pub market: Account<Market>,
    #[account(
        seeds = [b"model", market.address().as_ref(), model.model_id.as_ref()],
        bump = model.bump,
        constraint = model.market == *market.address() @ crate::RouterError::InvalidAccount
    )]
    pub model: Account<Model>,
    #[account(
        mut,
        address = model.terms @ crate::RouterError::InvalidAccount,
        constraint = terms.model == *model.address() @ crate::RouterError::InvalidAccount,
        constraint = terms.limit.get() == model.limit.get() @ crate::RouterError::InvalidAccount
    )]
    pub terms: Slab<Terms, TermsEntry>,
    pub signer: Signer,
}

#[derive(Accounts)]
pub struct ReserveBooking {
    pub market: Account<Market>,
    #[account(
        seeds = [b"model", market.address().as_ref(), model.model_id.as_ref()],
        bump = model.bump,
        constraint = model.market == *market.address() @ crate::RouterError::InvalidAccount
    )]
    pub model: Account<Model>,
    #[account(
        address = model.terms @ crate::RouterError::InvalidAccount,
        constraint = terms.model == *model.address() @ crate::RouterError::InvalidAccount,
        constraint = terms.limit.get() == model.limit.get() @ crate::RouterError::InvalidAccount
    )]
    pub terms: Slab<Terms, TermsEntry>,
    #[account(
        mut,
        address = model.capacity @ crate::RouterError::InvalidAccount,
        constraint = capacity.model == *model.address() @ crate::RouterError::InvalidAccount,
        constraint = capacity.limit.get() == model.limit.get() @ crate::RouterError::InvalidAccount,
        constraint = capacity.len() == terms.len() @ crate::RouterError::InvalidAccount
    )]
    pub capacity: Slab<Capacity, CapacityEntry>,
    #[account(
        mut,
        constraint = user.owner == *owner.address() @ crate::RouterError::InvalidAccount,
        constraint = user.market == *market.address() @ crate::RouterError::InvalidAccount
    )]
    pub user: Account<User>,
    pub owner: Signer,
    #[account(mut, owner = crate::ID @ crate::RouterError::InvalidAccount)]
    pub booking: Account<Booking>,
    pub rent_payer: UncheckedAccount,
    #[account(constraint = list.market == *market.address() @ crate::RouterError::InvalidAccount)]
    pub list: Slab<List, Address>,
}

#[derive(Accounts)]
pub struct ClaimBooking {
    pub market: Account<Market>,
    #[account(
        seeds = [b"model", market.address().as_ref(), model.model_id.as_ref()],
        bump = model.bump,
        constraint = model.market == *market.address() @ crate::RouterError::InvalidAccount
    )]
    pub model: Account<Model>,
    #[account(
        mut,
        address = model.capacity @ crate::RouterError::InvalidAccount,
        constraint = capacity.model == *model.address() @ crate::RouterError::InvalidAccount,
        constraint = capacity.limit.get() == model.limit.get() @ crate::RouterError::InvalidAccount
    )]
    pub capacity: Slab<Capacity, CapacityEntry>,
    #[account(
        mut,
        address = booking.user @ crate::RouterError::InvalidAccount,
        constraint = user.owner == booking.owner @ crate::RouterError::InvalidAccount,
        constraint = user.market == *market.address() @ crate::RouterError::InvalidAccount
    )]
    pub user: Account<User>,
    #[account(
        mut,
        constraint = booking.market == *market.address() @ crate::RouterError::InvalidState,
        constraint = booking.model == *model.address() @ crate::RouterError::InvalidState,
        constraint = booking.provider == *provider.address() @ crate::RouterError::InvalidState,
        constraint = booking.payout == *payout.address() @ crate::RouterError::InvalidState
    )]
    pub booking: Account<Booking>,
    pub provider: Signer,
    #[account(
        mut,
        address = user.vault @ crate::RouterError::InvalidAccount,
        token::mint = mint,
        token::authority = authority,
        constraint = !vault.has_delegate() && !vault.has_close_authority() && !vault.is_native() @ crate::RouterError::InvalidAccount
    )]
    pub vault: Account<anchor_spl::token::TokenAccount>,
    #[account(mut, token::mint = mint, token::authority = provider)]
    pub payout: Account<anchor_spl::token::TokenAccount>,
    #[account(address = market.mint @ crate::RouterError::InvalidAccount)]
    pub mint: Account<anchor_spl::mint::Mint>,
    #[account(seeds = [b"vault", user.address().as_ref()], bump = user.bump)]
    pub authority: UncheckedAccount,
    pub token: Program<Token>,
}

#[derive(Accounts)]
pub struct ExpireBooking {
    pub market: Account<Market>,
    #[account(
        seeds = [b"model", market.address().as_ref(), model.model_id.as_ref()],
        bump = model.bump,
        constraint = model.market == *market.address() @ crate::RouterError::InvalidAccount
    )]
    pub model: Account<Model>,
    #[account(
        mut,
        address = model.capacity @ crate::RouterError::InvalidAccount,
        constraint = capacity.model == *model.address() @ crate::RouterError::InvalidAccount,
        constraint = capacity.limit.get() == model.limit.get() @ crate::RouterError::InvalidAccount
    )]
    pub capacity: Slab<Capacity, CapacityEntry>,
    #[account(
        mut,
        address = booking.user @ crate::RouterError::InvalidAccount,
        constraint = user.owner == booking.owner @ crate::RouterError::InvalidAccount,
        constraint = user.market == *market.address() @ crate::RouterError::InvalidAccount
    )]
    pub user: Account<User>,
    #[account(
        mut,
        constraint = booking.market == *market.address() @ crate::RouterError::InvalidState,
        constraint = booking.model == *model.address() @ crate::RouterError::InvalidState
    )]
    pub booking: Account<Booking>,
}

#[derive(Accounts)]
pub struct Withdraw {
    pub market: Account<Market>,
    #[account(
        mut,
        constraint = user.owner == *owner.address() @ crate::RouterError::InvalidAccount,
        constraint = user.market == *market.address() @ crate::RouterError::InvalidAccount
    )]
    pub user: Account<User>,
    pub owner: Signer,
    #[account(mut, address = user.vault @ crate::RouterError::InvalidAccount, token::mint = mint)]
    pub vault: Account<anchor_spl::token::TokenAccount>,
    #[account(mut, token::mint = mint, token::authority = owner)]
    pub destination: Account<anchor_spl::token::TokenAccount>,
    #[account(address = market.mint @ crate::RouterError::InvalidAccount)]
    pub mint: Account<anchor_spl::mint::Mint>,
    #[account(seeds = [b"vault", user.address().as_ref()], bump = user.bump)]
    pub authority: UncheckedAccount,
    pub token: Program<Token>,
}

#[derive(Accounts)]
pub struct ReleaseBooking {
    pub market: Account<Market>,
    #[account(
        seeds = [b"model", market.address().as_ref(), model.model_id.as_ref()],
        bump = model.bump,
        constraint = model.market == *market.address() @ crate::RouterError::InvalidAccount
    )]
    pub model: Account<Model>,
    #[account(
        mut,
        address = model.capacity @ crate::RouterError::InvalidAccount,
        constraint = capacity.model == *model.address() @ crate::RouterError::InvalidAccount,
        constraint = capacity.limit.get() == model.limit.get() @ crate::RouterError::InvalidAccount
    )]
    pub capacity: Slab<Capacity, CapacityEntry>,
    #[account(
        mut,
        address = booking.user @ crate::RouterError::InvalidAccount,
        constraint = user.owner == booking.owner @ crate::RouterError::InvalidAccount,
        constraint = user.market == *market.address() @ crate::RouterError::InvalidAccount
    )]
    pub user: Account<User>,
    #[account(
        mut,
        constraint = booking.market == *market.address() @ crate::RouterError::InvalidState,
        constraint = booking.model == *model.address() @ crate::RouterError::InvalidState,
        constraint = booking.provider == *provider.address() @ crate::RouterError::InvalidState
    )]
    pub booking: Account<Booking>,
    pub provider: Signer,
}

#[derive(Accounts)]
pub struct CloseBooking {
    #[account(
        mut,
        constraint = booking.rent_payer == *recipient.address() @ crate::RouterError::InvalidState
    )]
    pub booking: Account<Booking>,
    #[account(mut)]
    pub recipient: UncheckedAccount,
}

#[derive(Accounts)]
#[instruction(_model_id: Address, _limit: u32)]
pub struct InitializeModel {
    pub market: Account<Market>,
    pub provider: Signer,
    #[account(mut)]
    pub payer: Signer,
    #[account(
        seeds = [b"provider", market.address().as_ref(), provider.address().as_ref()],
        bump = profile.bump,
        constraint = profile.market == *market.address() @ crate::RouterError::Unauthorized,
        constraint = profile.authority == *provider.address() @ crate::RouterError::Unauthorized
    )]
    pub profile: Account<Provider>,
    #[account(
        init,
        payer = payer,
        seeds = [b"model", market.address().as_ref(), _model_id.as_ref()],
        bump
    )]
    pub model: Account<Model>,
    #[account(
        init,
        payer = payer,
        space = Slab::<Terms, TermsEntry>::space_for(_limit)
    )]
    pub terms: Slab<Terms, TermsEntry>,
    #[account(
        init,
        payer = payer,
        space = Slab::<Capacity, CapacityEntry>::space_for(_limit)
    )]
    pub capacity: Slab<Capacity, CapacityEntry>,
    #[account(
        init,
        payer = payer,
        seeds = [b"model_metadata", model.address().as_ref()],
        bump
    )]
    pub metadata: Account<ModelMetadata>,
    pub system_program: Program<System>,
}

#[derive(Accounts)]
pub struct InitializeProvider {
    pub market: Account<Market>,
    pub provider: Signer,
    #[account(mut)]
    pub payer: Signer,
    #[account(
        init,
        payer = payer,
        seeds = [b"provider", market.address().as_ref(), provider.address().as_ref()],
        bump
    )]
    pub profile: Account<Provider>,
    pub system_program: Program<System>,
}

#[derive(Accounts)]
pub struct UpdateProfile {
    pub market: Account<Market>,
    #[account(
        mut,
        seeds = [b"provider", market.address().as_ref(), profile.authority.as_ref()],
        bump = profile.bump,
        constraint = profile.account().data_len() == Account::<Provider>::INIT_SPACE @ crate::RouterError::InvalidAccount,
        constraint = profile.market == *market.address() @ crate::RouterError::InvalidAccount,
        constraint = profile.authority == *signer.address() @ crate::RouterError::Unauthorized
    )]
    pub profile: Account<Provider>,
    pub signer: Signer,
}

#[derive(Accounts)]
pub struct UpdateModelMetadata {
    pub market: Account<Market>,
    #[account(
        seeds = [b"model", market.address().as_ref(), model.model_id.as_ref()],
        bump = model.bump,
        constraint = model.market == *market.address() @ crate::RouterError::InvalidAccount
    )]
    pub model: Account<Model>,
    #[account(
        mut,
        seeds = [b"model_metadata", model.address().as_ref()],
        bump = metadata.bump,
        constraint = metadata.market == *market.address() @ crate::RouterError::InvalidAccount,
        constraint = metadata.target == *model.address() @ crate::RouterError::InvalidAccount,
        constraint = metadata.authority == *provider.address() @ crate::RouterError::Unauthorized
    )]
    pub metadata: Account<ModelMetadata>,
    pub provider: Signer,
}

#[derive(Accounts)]
pub struct InitializeReceipt {
    pub market: Account<Market>,
    #[account(
        constraint = user.owner == *owner.address() @ crate::RouterError::InvalidAccount,
        constraint = user.market == *market.address() @ crate::RouterError::InvalidAccount
    )]
    pub user: Account<User>,
    pub owner: Signer,
    #[account(mut)]
    pub payer: Signer,
    #[account(init, payer = payer)]
    pub booking: Account<Booking>,
    pub system_program: Program<System>,
}

#[derive(Accounts)]
#[instruction(_config: crate::args::ListInit)]
pub struct InitializeList {
    pub market: Account<Market>,
    #[account(mut)]
    pub authority: Signer,
    #[account(
        init,
        payer = authority,
        space = Slab::<List, Address>::space_for(_config.limit)
    )]
    pub list: Slab<List, Address>,
    pub system_program: Program<System>,
}

#[derive(Accounts)]
pub struct UpdateList {
    pub market: Account<Market>,
    #[account(
        mut,
        constraint = list.market == *market.address() @ crate::RouterError::InvalidAccount,
        constraint = list.authority == *authority.address() @ crate::RouterError::Unauthorized
    )]
    pub list: Slab<List, Address>,
    pub authority: Signer,
}

#[derive(Accounts)]
pub struct CloseList {
    #[account(
        mut,
        constraint = list.authority == *authority.address() @ crate::RouterError::Unauthorized
    )]
    pub list: Slab<List, Address>,
    #[account(mut)]
    pub authority: Signer,
    #[account(
        mut,
        seeds = [b"list_metadata", list.address().as_ref()],
        bump
    )]
    pub metadata: UncheckedAccount,
}

#[derive(Accounts)]
pub struct GrowBooks {
    pub market: Account<Market>,
    #[account(
        mut,
        seeds = [b"model", market.address().as_ref(), model.model_id.as_ref()],
        bump = model.bump,
        constraint = model.market == *market.address() @ crate::RouterError::InvalidAccount
    )]
    pub model: Account<Model>,
    #[account(
        mut,
        address = model.terms @ crate::RouterError::InvalidAccount,
        constraint = terms.model == *model.address() @ crate::RouterError::InvalidAccount
    )]
    pub terms: Slab<Terms, TermsEntry>,
    #[account(
        mut,
        address = model.capacity @ crate::RouterError::InvalidAccount,
        constraint = capacity.model == *model.address() @ crate::RouterError::InvalidAccount,
        constraint = capacity.len() == terms.len() @ crate::RouterError::InvalidAccount
    )]
    pub capacity: Slab<Capacity, CapacityEntry>,
    #[account(mut)]
    pub payer: Signer,
}

#[derive(Accounts)]
pub struct InitializeListMetadata {
    pub market: Account<Market>,
    #[account(
        constraint = list.market == *market.address() @ crate::RouterError::InvalidAccount,
        constraint = list.authority == *authority.address() @ crate::RouterError::Unauthorized
    )]
    pub list: Slab<List, Address>,
    pub authority: Signer,
    #[account(mut)]
    pub payer: Signer,
    #[account(
        init,
        payer = payer,
        seeds = [b"list_metadata", list.address().as_ref()],
        bump
    )]
    pub metadata: Account<ListMetadata>,
    pub system_program: Program<System>,
}

#[derive(Accounts)]
pub struct UpdateListMetadata {
    pub market: Account<Market>,
    #[account(
        constraint = list.market == *market.address() @ crate::RouterError::InvalidAccount,
        constraint = list.authority == *authority.address() @ crate::RouterError::Unauthorized
    )]
    pub list: Slab<List, Address>,
    #[account(
        mut,
        seeds = [b"list_metadata", list.address().as_ref()],
        bump = metadata.bump,
        constraint = metadata.market == list.market @ crate::RouterError::InvalidAccount,
        constraint = metadata.target == *list.address() @ crate::RouterError::InvalidAccount,
        constraint = metadata.authority == list.authority @ crate::RouterError::InvalidAccount
    )]
    pub metadata: Account<ListMetadata>,
    pub authority: Signer,
}

#[derive(Accounts)]
pub struct ConfigureFees {
    #[account(
        mut,
        constraint = market.admin == *signer.address() @ crate::RouterError::Unauthorized
    )]
    pub market: Account<Market>,
    pub signer: Signer,
}

#[derive(Accounts)]
pub struct ConfigureListFees {
    pub market: Account<Market>,
    #[account(
        mut,
        constraint = list.market == *market.address() @ crate::RouterError::InvalidAccount,
        constraint = list.authority == *authority.address() @ crate::RouterError::Unauthorized
    )]
    pub list: Slab<List, Address>,
    pub authority: Signer,
}

#[derive(Accounts)]
pub struct ActivateReserve {
    #[account(constraint = market.admin == *admin.address() @ crate::RouterError::Unauthorized)]
    pub market: Account<Market>,
    #[account(
        mut,
        seeds = [b"model", market.address().as_ref(), model.model_id.as_ref()],
        bump = model.bump,
        constraint = model.market == *market.address() @ crate::RouterError::InvalidAccount
    )]
    pub model: Account<Model>,
    pub admin: Signer,
    #[account(mut)]
    pub payer: Signer,
    #[account(
        mut,
        seeds = [b"reserve_mint", model.address().as_ref()],
        bump,
        owner = anchor_lang::system_program::ID @ crate::RouterError::InvalidAccount,
        constraint = mint.data_len() == 0 @ crate::RouterError::InvalidAccount
    )]
    pub mint: UncheckedAccount,
    #[account(
        mut,
        owner = anchor_spl::token::ID @ crate::RouterError::InvalidAccount,
        constraint = backing.data_len() == anchor_spl::token::TokenAccount::LEN @ crate::RouterError::InvalidAccount
    )]
    pub backing: UncheckedAccount,
    #[account(
        mut,
        owner = anchor_spl::token_2022::ID @ crate::RouterError::InvalidAccount,
        constraint = allocation.data_len() == 178 @ crate::RouterError::InvalidAccount
    )]
    pub allocation: UncheckedAccount,
    #[account(address = market.mint @ crate::RouterError::InvalidAccount)]
    pub quote: Account<anchor_spl::mint::Mint>,
    pub token: Program<Token>,
    pub t22: Program<Token2022>,
    #[account(address = market.dex_program @ crate::RouterError::InvalidAccount, executable)]
    pub dex: UncheckedAccount,
    #[account(mut, address = market.treasury @ crate::RouterError::InvalidAccount)]
    pub curve_payer: Signer,
    #[account(
        address = market.dex_config @ crate::RouterError::InvalidAccount,
        seeds = [b"config", quote.address().as_ref()],
        bump,
        seeds::program = dex.address()
    )]
    pub curve_config: Account<fossa_dex::state_types::Config>,
    #[account(
        mut,
        seeds = [b"pool", mint.address().as_ref()],
        bump,
        seeds::program = dex.address()
    )]
    pub curve_pool: UncheckedAccount,
    #[account(
        seeds = [b"authority", curve_pool.address().as_ref()],
        bump,
        seeds::program = dex.address()
    )]
    pub curve_authority: UncheckedAccount,
    #[account(
        mut,
        token::mint = quote,
        token::authority = curve_payer
    )]
    pub curve_quote_source: Account<anchor_spl::token::TokenAccount>,
    #[account(
        mut,
        owner = anchor_spl::token_2022::ID @ crate::RouterError::InvalidAccount,
        constraint = curve_base_vault.data_len() == 178 @ crate::RouterError::InvalidAccount
    )]
    pub curve_base_vault: UncheckedAccount,
    #[account(
        mut,
        token::mint = quote,
        token::authority = curve_authority
    )]
    pub curve_quote_vault: Account<anchor_spl::token::TokenAccount>,
    #[account(
        mut,
        seeds = [b"lp_mint", curve_pool.address().as_ref()],
        bump,
        seeds::program = dex.address()
    )]
    pub curve_lp_mint: UncheckedAccount,
    #[account(
        mut,
        token::mint = curve_lp_mint,
        token::authority = curve_payer
    )]
    pub curve_lp: Account<anchor_spl::token::TokenAccount>,
    #[account(
        seeds = [b"protocol"],
        bump,
        seeds::program = dex.address()
    )]
    pub dex_global: Account<fossa_dex::state_types::Global>,
    pub system_program: Program<System>,
}

#[derive(Accounts)]
pub struct ConfigureReserve {
    #[account(constraint = market.admin == *admin.address() @ crate::RouterError::Unauthorized)]
    pub market: Account<Market>,
    #[account(
        mut,
        seeds = [b"model", market.address().as_ref(), model.model_id.as_ref()],
        bump = model.bump,
        constraint = model.market == *market.address() @ crate::RouterError::InvalidAccount
    )]
    pub model: Account<Model>,
    pub admin: Signer,
    #[account(
        mut,
        address = model.fixed_token_mint @ crate::RouterError::InvalidAccount,
        mint::token_program = t22
    )]
    pub mint: InterfaceAccount<anchor_spl::token_interface::Mint>,
    pub t22: Program<Token2022>,
}

#[derive(Accounts)]
pub struct RedeemReserve {
    pub market: Account<Market>,
    #[account(
        seeds = [b"model", market.address().as_ref(), model.model_id.as_ref()],
        bump = model.bump,
        constraint = model.market == *market.address() @ crate::RouterError::InvalidAccount
    )]
    pub model: Account<Model>,
    #[account(
        mut,
        address = model.fixed_token_mint @ crate::RouterError::InvalidAccount,
        mint::token_program = t22
    )]
    pub mint: InterfaceAccount<anchor_spl::token_interface::Mint>,
    #[account(
        mut,
        token::mint = mint,
        token::authority = holder,
        token::token_program = t22
    )]
    pub source: InterfaceAccount<anchor_spl::token_interface::TokenAccount>,
    pub holder: Signer,
    #[account(
        mut,
        address = model.backing_vault @ crate::RouterError::InvalidAccount,
        token::mint = quote,
        token::authority = model
    )]
    pub backing: Account<anchor_spl::token::TokenAccount>,
    #[account(mut, token::mint = quote, token::authority = holder)]
    pub destination: Account<anchor_spl::token::TokenAccount>,
    #[account(address = market.mint @ crate::RouterError::InvalidAccount)]
    pub quote: Account<anchor_spl::mint::Mint>,
    pub token: Program<Token>,
    pub t22: Program<Token2022>,
}

#[derive(Accounts)]
pub struct CollectReserveTax {
    pub market: Account<Market>,
    #[account(
        seeds = [b"model", market.address().as_ref(), model.model_id.as_ref()],
        bump = model.bump,
        constraint = model.market == *market.address() @ crate::RouterError::InvalidAccount
    )]
    pub model: Account<Model>,
    #[account(
        mut,
        address = model.fixed_token_mint @ crate::RouterError::InvalidAccount,
        mint::token_program = t22
    )]
    pub mint: InterfaceAccount<anchor_spl::token_interface::Mint>,
    #[account(
        mut,
        token::mint = mint,
        token::token_program = t22,
        constraint = destination.owner() == &market.treasury2 @ crate::RouterError::InvalidAccount
    )]
    pub destination: InterfaceAccount<anchor_spl::token_interface::TokenAccount>,
    pub t22: Program<Token2022>,
}

#[derive(Accounts)]
pub struct SweepReserve {
    pub market: Account<Market>,
    #[account(
        seeds = [b"model", market.address().as_ref(), model.model_id.as_ref()],
        bump = model.bump,
        constraint = model.market == *market.address() @ crate::RouterError::InvalidAccount
    )]
    pub model: Account<Model>,
    #[account(
        address = model.fixed_token_mint @ crate::RouterError::InvalidAccount,
        owner = anchor_spl::token_2022::ID @ crate::RouterError::InvalidAccount
    )]
    pub mint: InterfaceAccount<anchor_spl::token_interface::Mint>,
    #[account(
        mut,
        address = model.backing_vault @ crate::RouterError::InvalidAccount,
        token::mint = quote,
        token::authority = model
    )]
    pub backing: Account<anchor_spl::token::TokenAccount>,
    #[account(
        mut,
        token::mint = quote,
        constraint = destination.owner() == &market.treasury @ crate::RouterError::InvalidAccount
    )]
    pub destination: Account<anchor_spl::token::TokenAccount>,
    #[account(address = market.mint @ crate::RouterError::InvalidAccount)]
    pub quote: Account<anchor_spl::mint::Mint>,
    pub token: Program<Token>,
}

#[derive(Accounts)]
pub struct ConfigureDex {
    #[account(
        mut,
        constraint = market.admin == *admin.address() @ crate::RouterError::Unauthorized
    )]
    pub market: Account<Market>,
    pub admin: Signer,
    #[account(address = fossa_dex::ID @ crate::RouterError::InvalidAccount, executable)]
    pub dex: UncheckedAccount,
    #[account(
        seeds = [b"config", market.mint.as_ref()],
        bump,
        seeds::program = dex.address()
    )]
    pub config: Account<fossa_dex::state_types::Config>,
}

#[derive(Accounts)]
pub struct RotateAdmin {
    #[account(
        mut,
        constraint = market.admin == *current.address() @ crate::RouterError::Unauthorized
    )]
    pub market: Account<Market>,
    pub current: Signer,
    pub next: Signer,
}
