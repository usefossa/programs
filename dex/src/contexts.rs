use crate::state_types::*;
use anchor_lang::prelude::*;
use anchor_spl::{mint, token};

#[derive(Accounts)]
pub struct InitializeConfig {
    #[account(seeds = [b"protocol"], bump)]
    pub global: Account<Global>,
    #[account(mut, address = global.council @ crate::DexError::Unauthorized)]
    pub council: Signer,
    pub token: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    #[account(mint::token_program = token)]
    pub mint: InterfaceAccount<anchor_spl::token_interface::Mint>,
    pub reference_pool: Option<Account<Pool>>,
    #[account(
        init,
        payer = council,
        seeds = [b"config", mint.address().as_ref()],
        bump
    )]
    pub config: Account<Config>,
    pub system_program: Program<System>,
}

#[derive(Accounts)]
pub struct Launch {
    // signer views hold no data borrow; council aliases are used only through checked cpi handles.
    #[account(mut, unsafe(dup))]
    pub payer: Signer,
    #[account(seeds = [b"protocol"], bump)]
    pub global: Account<Global>,
    // the system view holds no data borrow; action_fee skips a self-transfer before borrowing handles.
    #[account(mut, unsafe(dup), address = global.treasury @ crate::DexError::InvalidAccount)]
    pub treasury: SystemAccount,
    #[account(constraint = config.enabled == 1 @ crate::DexError::Unauthorized)]
    pub config: Account<Config>,
    pub token_a: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    pub token_b: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    pub token_program: Program<Token>,
    #[account(mut, mint::token_program = token_a)]
    pub mint_a: InterfaceAccount<anchor_spl::token_interface::Mint>,
    #[account(
        address = config.quote_mint @ crate::DexError::InvalidAccount,
        mint::token_program = token_b
    )]
    pub mint_b: InterfaceAccount<anchor_spl::token_interface::Mint>,
    #[account(
        init,
        payer = payer,
        seeds = [
            b"pool",
            mint_first(mint_a.address(), mint_b.address()).as_ref(),
            mint_last(mint_a.address(), mint_b.address()).as_ref()
        ],
        bump
    )]
    pub pool: Account<Pool>,
    #[account(seeds = [b"authority", pool.address().as_ref()], bump)]
    pub authority: UncheckedAccount,
    #[account(
        mut,
        token::mint = mint_a,
        token::authority = payer,
        token::token_program = token_a
    )]
    pub source_a: InterfaceAccount<anchor_spl::token_interface::TokenAccount>,
    #[account(
        mut,
        token::mint = mint_b,
        token::authority = payer,
        token::token_program = token_b
    )]
    pub source_b: InterfaceAccount<anchor_spl::token_interface::TokenAccount>,
    #[account(
        mut,
        token::mint = mint_a,
        token::authority = authority,
        token::token_program = token_a
    )]
    pub vault_a: InterfaceAccount<anchor_spl::token_interface::TokenAccount>,
    #[account(
        mut,
        token::mint = mint_b,
        token::authority = authority,
        token::token_program = token_b
    )]
    pub vault_b: InterfaceAccount<anchor_spl::token_interface::TokenAccount>,
    #[account(
        init,
        payer = payer,
        seeds = [b"lp_mint", pool.address().as_ref()],
        bump,
        mint::decimals = 9,
        mint::authority = authority
    )]
    pub lp_mint: Account<anchor_spl::mint::Mint>,
    #[account(
        init,
        payer = payer,
        token::mint = lp_mint,
        token::authority = payer
    )]
    pub user_lp: Account<anchor_spl::token::TokenAccount>,
    pub system_program: Program<System>,
}

#[derive(Accounts)]
pub struct InitializePool {
    pub launch: Nested<Launch>,
    pub council: Option<Signer>,
}

#[derive(Accounts)]
pub struct InitializeCurve {
    pub launch: Nested<Launch>,
}

#[derive(Accounts)]
pub struct InitializeModelCurve {
    pub launch: Nested<Launch>,
    #[account(address = launch.global.council @ crate::DexError::Unauthorized)]
    pub council: Signer,
}

#[derive(Accounts)]
pub struct Trade {
    pub user: Signer,
    #[account(mut)]
    pub pool: Account<Pool>,
    #[account(seeds = [b"authority", pool.address().as_ref()], bump = pool.bump)]
    pub authority: UncheckedAccount,
    pub token_a: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    pub token_b: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    #[account(address = pool.mint_a @ crate::DexError::InvalidAccount)]
    pub mint_a: InterfaceAccount<anchor_spl::token_interface::Mint>,
    #[account(address = pool.mint_b @ crate::DexError::InvalidAccount)]
    pub mint_b: InterfaceAccount<anchor_spl::token_interface::Mint>,
    #[account(
        mut,
        token::mint = mint_a,
        token::authority = user,
        token::token_program = token_a
    )]
    pub user_a: InterfaceAccount<anchor_spl::token_interface::TokenAccount>,
    #[account(
        mut,
        token::mint = mint_b,
        token::authority = user,
        token::token_program = token_b
    )]
    pub user_b: InterfaceAccount<anchor_spl::token_interface::TokenAccount>,
    #[account(
        mut,
        address = pool.vault_a @ crate::DexError::InvalidAccount
    )]
    pub vault_a: InterfaceAccount<anchor_spl::token_interface::TokenAccount>,
    #[account(
        mut,
        address = pool.vault_b @ crate::DexError::InvalidAccount
    )]
    pub vault_b: InterfaceAccount<anchor_spl::token_interface::TokenAccount>,
}

#[derive(Accounts)]
pub struct Swap {
    pub trade: Nested<Trade>,
}

#[derive(Accounts)]
pub struct SwapExactOut {
    pub trade: Nested<Trade>,
}

#[derive(Accounts)]
pub struct Graduate {
    #[account(mut)]
    pub pool: Account<Pool>,
    #[account(seeds = [b"authority", pool.address().as_ref()], bump = pool.bump)]
    pub authority: UncheckedAccount,
    pub token_a: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    pub token_b: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    pub token_program: Program<Token>,
    #[account(
        mut,
        address = pool.mint_a @ crate::DexError::InvalidAccount
    )]
    pub mint_a: InterfaceAccount<anchor_spl::token_interface::Mint>,
    #[account(address = pool.mint_b @ crate::DexError::InvalidAccount)]
    pub mint_b: InterfaceAccount<anchor_spl::token_interface::Mint>,
    #[account(
        mut,
        address = pool.vault_a @ crate::DexError::InvalidAccount
    )]
    pub vault_a: InterfaceAccount<anchor_spl::token_interface::TokenAccount>,
    #[account(
        mut,
        address = pool.vault_b @ crate::DexError::InvalidAccount
    )]
    pub vault_b: InterfaceAccount<anchor_spl::token_interface::TokenAccount>,
    #[account(address = pool.lp_mint @ crate::DexError::InvalidAccount)]
    pub lp_mint: Account<anchor_spl::mint::Mint>,
    #[account(address = pool.distributor @ crate::DexError::Unauthorized)]
    pub operator: Signer,
}

#[derive(Accounts)]
pub struct Liquidity {
    // signer/system aliases hold no data borrow; action_fee skips self-transfers.
    #[account(mut, unsafe(dup))]
    pub user: Signer,
    #[account(seeds = [b"protocol"], bump)]
    pub global: Account<Global>,
    #[account(mut, unsafe(dup), address = global.treasury @ crate::DexError::InvalidAccount)]
    pub treasury: SystemAccount,
    pub system_program: Program<System>,
    #[account(mut)]
    pub pool: Account<Pool>,
    #[account(seeds = [b"authority", pool.address().as_ref()], bump = pool.bump)]
    pub authority: UncheckedAccount,
    pub token_a: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    pub token_b: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    pub token_program: Program<Token>,
    #[account(address = pool.mint_a @ crate::DexError::InvalidAccount)]
    pub mint_a: InterfaceAccount<anchor_spl::token_interface::Mint>,
    #[account(address = pool.mint_b @ crate::DexError::InvalidAccount)]
    pub mint_b: InterfaceAccount<anchor_spl::token_interface::Mint>,
    #[account(mut, token::mint = mint_a, token::authority = user, token::token_program = token_a)]
    pub user_a: InterfaceAccount<anchor_spl::token_interface::TokenAccount>,
    #[account(mut, token::mint = mint_b, token::authority = user, token::token_program = token_b)]
    pub user_b: InterfaceAccount<anchor_spl::token_interface::TokenAccount>,
    #[account(
        mut,
        address = pool.vault_a @ crate::DexError::InvalidAccount
    )]
    pub vault_a: InterfaceAccount<anchor_spl::token_interface::TokenAccount>,
    #[account(
        mut,
        address = pool.vault_b @ crate::DexError::InvalidAccount
    )]
    pub vault_b: InterfaceAccount<anchor_spl::token_interface::TokenAccount>,
    #[account(
        mut,
        address = pool.lp_mint @ crate::DexError::InvalidAccount
    )]
    pub lp_mint: Account<anchor_spl::mint::Mint>,
    #[account(mut, token::mint = lp_mint, token::authority = user)]
    pub user_lp: Account<anchor_spl::token::TokenAccount>,
}

#[derive(Accounts)]
pub struct AddLiquidity {
    pub liquidity: Nested<Liquidity>,
}

#[derive(Accounts)]
pub struct RemoveLiquidity {
    pub liquidity: Nested<Liquidity>,
}

#[derive(Accounts)]
pub struct CollectFees {
    pub owner: Signer,
    #[account(mut)]
    pub pool: Account<Pool>,
    #[account(seeds = [b"authority", pool.address().as_ref()], bump = pool.bump)]
    pub authority: UncheckedAccount,
    pub token_a: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    pub token_b: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    #[account(address = pool.mint_a @ crate::DexError::InvalidAccount)]
    pub mint_a: InterfaceAccount<anchor_spl::token_interface::Mint>,
    #[account(address = pool.mint_b @ crate::DexError::InvalidAccount)]
    pub mint_b: InterfaceAccount<anchor_spl::token_interface::Mint>,
    #[account(mut, token::mint = mint_a, token::authority = owner, token::token_program = token_a)]
    pub destination_a: InterfaceAccount<anchor_spl::token_interface::TokenAccount>,
    #[account(mut, token::mint = mint_b, token::authority = owner, token::token_program = token_b)]
    pub destination_b: InterfaceAccount<anchor_spl::token_interface::TokenAccount>,
    #[account(
        mut,
        address = pool.vault_a @ crate::DexError::InvalidAccount
    )]
    pub vault_a: InterfaceAccount<anchor_spl::token_interface::TokenAccount>,
    #[account(
        mut,
        address = pool.vault_b @ crate::DexError::InvalidAccount
    )]
    pub vault_b: InterfaceAccount<anchor_spl::token_interface::TokenAccount>,
}

#[derive(Accounts)]
pub struct CollectTransferTax {
    #[account(mut)]
    pub pool: Account<Pool>,
    #[account(seeds = [b"authority", pool.address().as_ref()], bump = pool.bump)]
    pub authority: UncheckedAccount,
    pub token: Program<Token2022>,
    #[account(
        mut,
        address = pool.mint_a @ crate::DexError::InvalidAccount
    )]
    pub mint: InterfaceAccount<anchor_spl::token_interface::Mint>,
    #[account(
        mut,
        token::mint = mint,
        token::authority = pool.distributor,
        token::token_program = token
    )]
    pub destination: InterfaceAccount<anchor_spl::token_interface::TokenAccount>,
}

#[derive(Accounts)]
pub struct UpdateConfig {
    #[account(seeds = [b"protocol"], bump)]
    pub global: Account<Global>,
    #[account(address = global.council @ crate::DexError::Unauthorized)]
    pub council: Signer,
    pub token: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    #[account(mint::token_program = token)]
    pub mint: InterfaceAccount<anchor_spl::token_interface::Mint>,
    #[account(
        mut,
        seeds = [b"config", mint.address().as_ref()],
        bump
    )]
    pub config: Account<Config>,
}

#[derive(Accounts)]
pub struct RefreshQuote {
    #[account(mut)]
    pub config: Account<Config>,
    #[account(address = config.reference_pool @ crate::DexError::InvalidAccount)]
    pub reference_pool: Account<Pool>,
}

#[derive(Accounts)]
pub struct InitializeGlobal {
    #[account(mut)]
    pub council: Signer,
    #[account(
        init,
        payer = council,
        seeds = [b"protocol"],
        bump
    )]
    pub global: Account<Global>,
    #[account(
        address = crate::ID @ crate::DexError::Unauthorized,
        executable,
        owner = solana_sdk_ids::bpf_loader_upgradeable::ID @ crate::DexError::Unauthorized
    )]
    pub executable: UncheckedAccount,
    #[account(
        owner = solana_sdk_ids::bpf_loader_upgradeable::ID @ crate::DexError::Unauthorized,
        constraint = crate::contexts::loader_authority(
            executable.account(),
            program_data.account(),
            council.address(),
        )? @ crate::DexError::Unauthorized
    )]
    pub program_data: UncheckedAccount,
    pub system_program: Program<System>,
}

#[derive(Accounts)]
pub struct UpdateGlobal {
    #[account(
        mut,
        seeds = [b"protocol"],
        bump,
        constraint = global.account().data_len() == Account::<Global>::INIT_SPACE @ crate::DexError::InvalidAccount
    )]
    pub global: Account<Global>,
    #[account(address = global.council @ crate::DexError::Unauthorized)]
    pub council: Signer,
    pub new_council: Signer,
}

#[derive(Accounts)]
pub struct UpdateRecipients {
    #[account(seeds = [b"protocol"], bump)]
    pub global: Account<Global>,
    #[account(address = global.council @ crate::DexError::Unauthorized)]
    pub council: Signer,
    #[account(mut)]
    pub pool: Account<Pool>,
}

pub(crate) fn loader_authority(
    executable: &AccountView,
    program_data: &AccountView,
    authority: &Address,
) -> Result<bool> {
    use solana_loader_v3_interface::state::UpgradeableLoaderState;

    let executable_data = executable.try_borrow()?;
    let program_data_bytes = program_data.try_borrow()?;
    let Ok(UpgradeableLoaderState::Program {
        programdata_address,
    }) = bincode::deserialize(&executable_data)
    else {
        return Ok(false);
    };
    let Ok(UpgradeableLoaderState::ProgramData {
        upgrade_authority_address: Some(upgrade_authority),
        ..
    }) = bincode::deserialize(&program_data_bytes)
    else {
        return Ok(false);
    };
    Ok(
        programdata_address.to_bytes() == program_data.address().to_bytes()
            && upgrade_authority.to_bytes() == authority.to_bytes(),
    )
}

pub fn mint_first<'a>(a: &'a Address, b: &'a Address) -> &'a Address {
    if a.as_ref() < b.as_ref() {
        a
    } else {
        b
    }
}

pub fn mint_last<'a>(a: &'a Address, b: &'a Address) -> &'a Address {
    if a.as_ref() < b.as_ref() {
        b
    } else {
        a
    }
}
