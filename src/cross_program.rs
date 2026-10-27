#![allow(dead_code)]

use anchor_lang::prelude::*;
use anchor_spl::token_2022::spl_token_2022::extension::transfer_fee::instruction::TransferFeeInstruction;
use anchor_spl::token_2022::spl_token_2022::instruction::{AuthorityType, TokenInstruction};
use anchor_spl::token_interface as token;

// fixed token account structs already define canonical metas; invoke validates them once.
pub fn invoke_token<'a, T: anchor_lang::ToCpiAccounts<'a>>(
    ctx: CpiContext<'a, T>,
    instruction: TokenInstruction,
) -> ProgramResult {
    require!(
        ctx.program == &anchor_spl::token::ID || ctx.program == &token::ID,
        ProgramError::IncorrectProgramId
    );
    ctx.invoke(&instruction.pack())
}

pub fn invoke_transfer_fee<'a, T: anchor_lang::ToCpiAccounts<'a>>(
    ctx: CpiContext<'a, T>,
    instruction: TransferFeeInstruction,
) -> ProgramResult {
    require!(ctx.program == &token::ID, ProgramError::IncorrectProgramId);
    let mut data = TokenInstruction::TransferFeeExtension.pack();
    instruction.pack(&mut data);
    ctx.invoke(&data)
}

pub fn transfer_checked(
    from: &AccountView,
    mint: &AccountView,
    to: &AccountView,
    authority: &AccountView,
    program: &Address,
    amount: u64,
    decimals: u8,
    seeds: &[&[&[u8]]],
) -> ProgramResult {
    let (mut from, mut to) = (*from, *to);
    invoke_token(
        CpiContext::new_with_signer(
            program,
            token::TransferChecked {
                from: from.try_to_cpi_handle_mut()?,
                mint: mint.to_cpi_handle(),
                to: to.try_to_cpi_handle_mut()?,
                authority: authority.to_cpi_handle(),
            },
            seeds,
        ),
        TokenInstruction::TransferChecked { amount, decimals },
    )
}

pub fn mint_to(
    mint: &AccountView,
    to: &AccountView,
    authority: &AccountView,
    program: &Address,
    amount: u64,
    seeds: &[&[&[u8]]],
) -> ProgramResult {
    let (mut mint, mut to) = (*mint, *to);
    invoke_token(
        CpiContext::new_with_signer(
            program,
            token::MintTo {
                mint: mint.try_to_cpi_handle_mut()?,
                to: to.try_to_cpi_handle_mut()?,
                authority: authority.to_cpi_handle(),
            },
            seeds,
        ),
        TokenInstruction::MintTo { amount },
    )
}

pub fn burn(
    from: &AccountView,
    mint: &AccountView,
    authority: &AccountView,
    program: &Address,
    amount: u64,
    seeds: &[&[&[u8]]],
) -> ProgramResult {
    let (mut from, mut mint) = (*from, *mint);
    invoke_token(
        CpiContext::new_with_signer(
            program,
            token::Burn {
                from: from.try_to_cpi_handle_mut()?,
                mint: mint.try_to_cpi_handle_mut()?,
                authority: authority.to_cpi_handle(),
            },
            seeds,
        ),
        TokenInstruction::Burn { amount },
    )
}

pub fn revoke_mint(
    mint: &AccountView,
    authority: &AccountView,
    program: &Address,
    seeds: &[&[&[u8]]],
) -> ProgramResult {
    let mut mint = *mint;
    invoke_token(
        CpiContext::new_with_signer(
            program,
            token::SetAuthority {
                account_or_mint: mint.try_to_cpi_handle_mut()?,
                current_authority: authority.to_cpi_handle(),
            },
            seeds,
        ),
        TokenInstruction::SetAuthority {
            authority_type: AuthorityType::MintTokens,
            new_authority: None.into(),
        },
    )
}

pub fn close_token(
    account: &AccountView,
    destination: &AccountView,
    authority: &AccountView,
    program: &Address,
    seeds: &[&[&[u8]]],
) -> ProgramResult {
    let (mut account, mut destination) = (*account, *destination);
    invoke_token(
        CpiContext::new_with_signer(
            program,
            token::CloseAccount {
                account: account.try_to_cpi_handle_mut()?,
                destination: destination.try_to_cpi_handle_mut()?,
                authority: authority.to_cpi_handle(),
            },
            seeds,
        ),
        TokenInstruction::CloseAccount,
    )
}

pub fn transfer_lamports(from: &AccountView, to: &AccountView, lamports: u64) -> ProgramResult {
    let (mut from, mut to) = (*from, *to);
    anchor_lang::system_program::transfer(
        CpiContext::new(
            &anchor_lang::system_program::ID,
            anchor_lang::system_program::Transfer {
                from: from.try_to_cpi_handle_mut()?,
                to: to.try_to_cpi_handle_mut()?,
            },
        ),
        lamports,
    )
}
