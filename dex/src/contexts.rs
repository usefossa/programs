use crate::state_types::*;
use anchor_lang::prelude::*;
use anchor_spl::{mint, token};

#[derive(Accounts)]
pub struct InitializeConfig {
    #[account(seeds = [b"protocol"], bump)]
    pub global: Account<Global>,
    #[account(mut, address = global.admin @ crate::DexError::Unauthorized)]
    pub admin: Signer,
    pub token: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    #[account(mint::token_program = token)]
    pub mint: InterfaceAccount<anchor_spl::token_interface::Mint>,
    #[account(
        init,
        payer = admin,
        seeds = [b"config", mint.address().as_ref()],
        bump
    )]
    pub config: Account<Config>,
    pub system_program: Program<System>,
}

#[derive(Accounts)]
pub struct Launch {
    #[account(mut)]
    pub payer: Signer,
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
        seeds = [b"pool", mint_a.address().as_ref()],
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
}

#[derive(Accounts)]
pub struct InitializeCurve {
    pub launch: Nested<Launch>,
}

#[derive(Accounts)]
pub struct InitializeModelCurve {
    pub launch: Nested<Launch>,
    #[account(seeds = [b"protocol"], bump)]
    pub global: Account<Global>,
    #[account(address = global.admin @ crate::DexError::Unauthorized)]
    pub admin: Signer,
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
    pub user: Signer,
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
pub struct CreateVesting {
    #[account(mut)]
    pub creator: Signer,
    #[account(mut)]
    pub pool: Account<Pool>,
    #[account(
        seeds = [b"protocol"],
        bump,
        constraint = creator.address() == &pool.creator || creator.address() == &global.admin @ crate::DexError::Unauthorized
    )]
    pub global: Account<Global>,
    #[account(
        init,
        payer = creator
    )]
    pub vesting: Account<Vesting>,
    pub beneficiary: UncheckedAccount,
    pub system_program: Program<System>,
}

#[derive(Accounts)]
pub struct ClaimVesting {
    pub beneficiary: Signer,
    #[account(mut)]
    pub pool: Account<Pool>,
    #[account(
        mut,
        constraint = vesting.pool == *pool.address() @ crate::DexError::Unauthorized,
        constraint = vesting.beneficiary == *beneficiary.address() @ crate::DexError::Unauthorized
    )]
    pub vesting: Account<Vesting>,
    #[account(seeds = [b"authority", pool.address().as_ref()], bump = pool.bump)]
    pub authority: UncheckedAccount,
    pub token: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    #[account(address = pool.mint_a @ crate::DexError::InvalidAccount)]
    pub mint: InterfaceAccount<anchor_spl::token_interface::Mint>,
    #[account(
        mut,
        address = pool.vault_a @ crate::DexError::InvalidAccount
    )]
    pub vault: InterfaceAccount<anchor_spl::token_interface::TokenAccount>,
    #[account(mut, token::mint = mint, token::authority = beneficiary, token::token_program = token)]
    pub destination: InterfaceAccount<anchor_spl::token_interface::TokenAccount>,
}

#[derive(Accounts)]
pub struct UpdateConfig {
    #[account(seeds = [b"protocol"], bump)]
    pub global: Account<Global>,
    #[account(address = global.admin @ crate::DexError::Unauthorized)]
    pub admin: Signer,
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
pub struct InitializeGlobal {
    #[account(mut)]
    pub admin: Signer,
    #[account(
        init,
        payer = admin,
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
            admin.address(),
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
    #[account(address = global.admin @ crate::DexError::Unauthorized)]
    pub admin: Signer,
    pub new_admin: Signer,
}

#[derive(Accounts)]
pub struct UpdateRecipients {
    #[account(seeds = [b"protocol"], bump)]
    pub global: Account<Global>,
    #[account(address = global.admin @ crate::DexError::Unauthorized)]
    pub admin: Signer,
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
