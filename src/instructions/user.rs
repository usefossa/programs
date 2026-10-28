use crate::{
    helpers::{add, require, sub},
    token::user_transfer,
};
use anchor_lang::prelude::ProgramResult;
use anchor_lang::{AnchorAccount, CpiContext, ToCpiHandle, ToCpiHandleMut};

#[inline(always)]
pub(crate) fn initialize(
    ctx: &mut anchor_lang::Context<crate::contexts::InitializeUser>,
) -> ProgramResult {
    crate::helpers::require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    let owner = accounts.owner.account();
    accounts.user.owner = *owner.address();
    accounts.user.market = *accounts.market.address();
    accounts.user.vault = *accounts.vault.address();
    accounts.user.bump = ctx.bumps.authority;
    Ok(())
}

#[inline(always)]
pub(crate) fn deposit(
    ctx: &mut anchor_lang::Context<crate::contexts::Deposit>,
    amount: u64,
) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    require(amount > 0, crate::RouterError::InvalidArgument)?;
    let balance = add(accounts.user.free.get(), amount)?;
    let authority = if accounts.user.address() == accounts.owner.address() {
        accounts.user.to_cpi_handle()
    } else {
        accounts.owner.to_cpi_handle()
    };
    anchor_spl::token::transfer_checked(
        CpiContext::new(
            &anchor_spl::token::ID,
            anchor_spl::token::TransferChecked {
                from: accounts.source.try_to_cpi_handle_mut()?,
                mint: accounts.mint.to_cpi_handle(),
                to: accounts.vault.try_to_cpi_handle_mut()?,
                authority,
            },
        ),
        amount,
        6,
    )?;
    accounts.user.free = balance.into();
    Ok(())
}

#[inline(always)]
pub(crate) fn withdraw(
    ctx: &mut anchor_lang::Context<crate::contexts::Withdraw>,
    amount: u64,
) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    require(amount > 0, crate::RouterError::InvalidArgument)?;
    let balance = sub(accounts.user.free.get(), amount)?;
    user_transfer(
        [
            accounts.vault.account(),
            accounts.mint.account(),
            accounts.destination.account(),
            accounts.authority.account(),
        ],
        accounts.user.address(),
        accounts.user.bump,
        ctx.program_id,
        amount,
    )?;
    accounts.user.free = balance.into();
    Ok(())
}
