#![allow(dead_code)]

use anchor_lang::prelude::*;
use anchor_spl::token_2022::spl_token_2022::instruction::AuthorityType;
use anchor_spl::token_interface as token;

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
    token::transfer_checked(
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
        amount,
        decimals,
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
    token::mint_to(
        CpiContext::new_with_signer(
            program,
            token::MintTo {
                mint: mint.try_to_cpi_handle_mut()?,
                to: to.try_to_cpi_handle_mut()?,
                authority: authority.to_cpi_handle(),
            },
            seeds,
        ),
        amount,
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
    token::burn(
        CpiContext::new_with_signer(
            program,
            token::Burn {
                from: from.try_to_cpi_handle_mut()?,
                mint: mint.try_to_cpi_handle_mut()?,
                authority: authority.to_cpi_handle(),
            },
            seeds,
        ),
        amount,
    )
}

pub fn revoke_mint(
    mint: &AccountView,
    authority: &AccountView,
    program: &Address,
    seeds: &[&[&[u8]]],
) -> ProgramResult {
    let mut mint = *mint;
    token::set_authority(
        CpiContext::new_with_signer(
            program,
            token::SetAuthority {
                account_or_mint: mint.try_to_cpi_handle_mut()?,
                current_authority: authority.to_cpi_handle(),
            },
            seeds,
        ),
        AuthorityType::MintTokens,
        None,
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
    token::close_account(CpiContext::new_with_signer(
        program,
        token::CloseAccount {
            account: account.try_to_cpi_handle_mut()?,
            destination: destination.try_to_cpi_handle_mut()?,
            authority: authority.to_cpi_handle(),
        },
        seeds,
    ))
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
