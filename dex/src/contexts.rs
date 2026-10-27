use crate::schemas::*;
use anchor_lang::prelude::*;

// identity and funding roles may reuse state or token addresses. exemptions keep checked
// data borrows and each handler's original ownership, pda and distinctness requirements.

#[derive(Accounts)]
pub struct InitializeConfig {
    pub admin: Signer,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub config: Signer,
    pub mint: UncheckedAccount,
    pub token: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    pub global: Account<GlobalState>,
}

#[derive(Accounts)]
pub struct InitializePool {
    #[account(mut, unsafe(dup))]
    pub payer: Signer,
    pub config: Account<ConfigState>,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub pool: Signer,
    pub authority: UncheckedAccount,
    pub ma: UncheckedAccount,
    pub mb: UncheckedAccount,
    pub sa: UncheckedAccount,
    pub sb: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub va: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub vb: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub lm: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub ulp: UncheckedAccount,
    pub ta: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    pub tb: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    pub tlp: Program<Token>,
}

#[derive(Accounts)]
pub struct InitializeCurve {
    #[account(mut, unsafe(dup))]
    pub payer: Signer,
    pub config: Account<ConfigState>,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub pool: Signer,
    pub authority: UncheckedAccount,
    pub ma: UncheckedAccount,
    pub mb: UncheckedAccount,
    pub sa: UncheckedAccount,
    pub sb: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub va: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub vb: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub lm: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub ulp: UncheckedAccount,
    pub ta: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    pub tb: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    pub tlp: Program<Token>,
}

#[derive(Accounts)]
pub struct InitializeModelCurve {
    #[account(mut, unsafe(dup))]
    pub payer: Signer,
    pub config: Account<ConfigState>,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub pool: Signer,
    pub authority: UncheckedAccount,
    pub ma: UncheckedAccount,
    pub mb: UncheckedAccount,
    pub sa: UncheckedAccount,
    pub sb: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub va: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub vb: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub lm: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub ulp: UncheckedAccount,
    pub ta: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    pub tb: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    pub tlp: Program<Token>,
    pub admin: Signer,
    pub global: Account<GlobalState>,
}

#[derive(Accounts)]
pub struct Swap {
    pub user: Signer,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub pool: UncheckedAccount,
    pub authority: UncheckedAccount,
    pub ma: UncheckedAccount,
    pub mb: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub ua: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub ub: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub va: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub vb: UncheckedAccount,
    pub ta: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    pub tb: Interface<'static, anchor_spl::token_interface::TokenInterface>,
}

#[derive(Accounts)]
pub struct SwapExactOut {
    pub user: Signer,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub pool: UncheckedAccount,
    pub authority: UncheckedAccount,
    pub ma: UncheckedAccount,
    pub mb: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub ua: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub ub: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub va: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub vb: UncheckedAccount,
    pub ta: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    pub tb: Interface<'static, anchor_spl::token_interface::TokenInterface>,
}

#[derive(Accounts)]
pub struct Graduate {
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub pool: UncheckedAccount,
    pub authority: UncheckedAccount,
    pub ma: UncheckedAccount,
    pub mb: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub va: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub vb: UncheckedAccount,
    pub lm: UncheckedAccount,
    pub ta: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    pub tb: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    pub tlp: Program<Token>,
    pub operator: Signer,
}

#[derive(Accounts)]
pub struct AddLiquidity {
    pub user: Signer,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub pool: UncheckedAccount,
    pub authority: UncheckedAccount,
    pub ma: UncheckedAccount,
    pub mb: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub ua: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub ub: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub va: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub vb: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub lm: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub ulp: UncheckedAccount,
    pub ta: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    pub tb: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    pub tlp: Program<Token>,
}

#[derive(Accounts)]
pub struct RemoveLiquidity {
    pub user: Signer,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub pool: UncheckedAccount,
    pub authority: UncheckedAccount,
    pub ma: UncheckedAccount,
    pub mb: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub ua: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub ub: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub va: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub vb: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub lm: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub ulp: UncheckedAccount,
    pub ta: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    pub tb: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    pub tlp: Program<Token>,
}

#[derive(Accounts)]
pub struct CollectFees {
    pub owner: Signer,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub pool: UncheckedAccount,
    pub authority: UncheckedAccount,
    pub ma: UncheckedAccount,
    pub mb: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub ua: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub ub: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub va: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub vb: UncheckedAccount,
    pub ta: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    pub tb: Interface<'static, anchor_spl::token_interface::TokenInterface>,
}

#[derive(Accounts)]
pub struct CollectTransferTax {
    #[account(mut, owner = crate::ID @ ProgramError::Custom(2))]
    pub pool: UncheckedAccount,
    pub authority: UncheckedAccount,
    #[account(mut)]
    pub mint: UncheckedAccount,
    #[account(mut)]
    pub destination: UncheckedAccount,
    pub token: Program<Token2022>,
}

#[derive(Accounts)]
pub struct CreateVesting {
    pub creator: Signer,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub pool: UncheckedAccount,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub vesting: Signer,
    pub beneficiary: UncheckedAccount,
}

#[derive(Accounts)]
pub struct ClaimVesting {
    pub beneficiary: Signer,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub pool: UncheckedAccount,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub vesting: UncheckedAccount,
    pub authority: UncheckedAccount,
    pub mint: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub vault: UncheckedAccount,
    #[account(mut, unsafe(dup))]
    pub destination: UncheckedAccount,
    pub token: Interface<'static, anchor_spl::token_interface::TokenInterface>,
}

#[derive(Accounts)]
pub struct UpdateConfig {
    pub admin: Signer,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub config: Account<ConfigState>,
    pub mint: UncheckedAccount,
    pub token: Interface<'static, anchor_spl::token_interface::TokenInterface>,
    pub global: Account<GlobalState>,
}

#[derive(Accounts)]
pub struct InitializeGlobal {
    #[account(mut, unsafe(dup))]
    pub admin: Signer,
    #[account(mut)]
    pub global: UncheckedAccount,
    #[account(executable)]
    pub executable: UncheckedAccount,
    pub program_data: UncheckedAccount,
    pub system: Program<System>,
}

#[derive(Accounts)]
pub struct UpdateGlobal {
    pub admin: Signer,
    pub new_admin: Signer,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub global: Account<GlobalState>,
}

#[derive(Accounts)]
pub struct UpdateRecipients {
    pub admin: Signer,
    #[account(mut, unsafe(dup), owner = crate::ID @ ProgramError::Custom(2))]
    pub pool: UncheckedAccount,
    pub global: Account<GlobalState>,
}
