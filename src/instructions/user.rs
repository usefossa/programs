use crate::{
    abi::{USER_LEN, USER_MAGIC},
    helpers::{add, addr, err, rd64, require, sub},
    state::{fresh, market},
    token::{token_owned_by, user_transfer, vault_owned_by},
};
use anchor_lang::prelude::{Address, ProgramResult};
use anchor_lang::{AnchorAccount, CpiContext, ToCpiHandle, ToCpiHandleMut};

#[inline(always)]
pub(crate) fn initialize(
    ctx: &mut anchor_lang::Context<crate::contexts::InitializeUser>,
    p: &[u8],
) -> ProgramResult {
    crate::helpers::require(ctx.remaining_accounts()?.is_empty(), 3)?;
    let program = ctx.program_id;
    let mut u = *ctx.accounts.u.account();
    let u = &mut u;
    let owner = ctx.accounts.owner.account();
    let m = ctx.accounts.m.account();
    let vault = ctx.accounts.vault.account();
    require(p.is_empty() && owner.is_signer(), 3)?;
    fresh(u, program, USER_LEN)?;
    let md = market(m, program)?;
    let (authority, bump) =
        Address::derive_program_address(&[b"vault", u.address().as_ref()], program)
            .ok_or(err(2))?;
    vault_owned_by(vault, &addr(&md, 40), &authority)?;
    let mut d = u.try_borrow_mut()?;
    d[..8].copy_from_slice(USER_MAGIC);
    d[8..40].copy_from_slice(owner.address().as_ref());
    d[40..72].copy_from_slice(m.address().as_ref());
    d[80..112].copy_from_slice(vault.address().as_ref());
    d[112] = bump;
    Ok(())
}

#[inline(always)]
pub(crate) fn deposit(
    ctx: &mut anchor_lang::Context<crate::contexts::Deposit>,
    p: &[u8],
) -> ProgramResult {
    require(ctx.remaining_accounts()?.is_empty() && p.len() == 8, 3)?;
    let accounts = &mut ctx.accounts;
    market(accounts.m.account(), ctx.program_id)?;
    require(
        accounts.u.account().data_len() == USER_LEN
            && accounts.u.owner == *accounts.owner.address()
            && accounts.u.market == *accounts.m.address(),
        2,
    )?;
    require(
        accounts.u.vault == *accounts.vault.address()
            && accounts.m.mint == *accounts.mint.address(),
        2,
    )?;
    let authority = Address::derive_address(
        &[b"vault", accounts.u.address().as_ref()],
        Some(accounts.u.bump),
        ctx.program_id,
    );
    vault_owned_by(
        accounts.vault.account(),
        accounts.mint.address(),
        &authority,
    )?;
    let amount = rd64(p, 0);
    require(amount > 0, 3)?;
    token_owned_by(
        accounts.source.account(),
        accounts.mint.address(),
        accounts.owner.address(),
    )?;
    let balance = add(accounts.u.free.get(), amount)?;
    let authority = if accounts.u.address() == accounts.owner.address() {
        accounts.u.to_cpi_handle()
    } else {
        accounts.owner.to_cpi_handle()
    };
    crate::cross_program::invoke_token(
        CpiContext::new(
            &anchor_spl::token::ID,
            anchor_spl::token::TransferChecked {
                from: accounts.source.try_to_cpi_handle_mut()?,
                mint: accounts.mint.to_cpi_handle(),
                to: accounts.vault.try_to_cpi_handle_mut()?,
                authority,
            },
        ),
        anchor_spl::token_2022::spl_token_2022::instruction::TokenInstruction::TransferChecked {
            amount,
            decimals: 6,
        },
    )?;
    accounts.u.free = balance.into();
    Ok(())
}

#[inline(always)]
pub(crate) fn withdraw(
    ctx: &mut anchor_lang::Context<crate::contexts::Withdraw>,
    p: &[u8],
) -> ProgramResult {
    require(ctx.remaining_accounts()?.is_empty() && p.len() == 8, 3)?;
    let accounts = &mut ctx.accounts;
    market(accounts.m.account(), ctx.program_id)?;
    require(
        accounts.u.account().data_len() == USER_LEN
            && accounts.u.owner == *accounts.owner.address()
            && accounts.u.market == *accounts.m.address(),
        2,
    )?;
    require(
        accounts.u.vault == *accounts.vault.address()
            && accounts.m.mint == *accounts.mint.address(),
        2,
    )?;
    let authority = Address::derive_address(
        &[b"vault", accounts.u.address().as_ref()],
        Some(accounts.u.bump),
        ctx.program_id,
    );
    vault_owned_by(
        accounts.vault.account(),
        accounts.mint.address(),
        &authority,
    )?;
    let amount = rd64(p, 0);
    require(amount > 0, 3)?;
    token_owned_by(
        accounts.destination.account(),
        accounts.mint.address(),
        accounts.owner.address(),
    )?;
    let balance = sub(accounts.u.free.get(), amount)?;
    user_transfer(
        [
            accounts.vault.account(),
            accounts.mint.account(),
            accounts.destination.account(),
            accounts.authority.account(),
        ],
        accounts.u.address(),
        accounts.u.bump,
        ctx.program_id,
        amount,
    )?;
    accounts.u.free = balance.into();
    Ok(())
}
