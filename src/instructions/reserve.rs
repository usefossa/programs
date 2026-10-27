use crate::{
    abi::*,
    helpers::{addr, err, rd16, rd64, require},
    state::{all_zero, initialize_pda, market, model, writable},
    token::{self, payment_token, TOKEN_2022},
};
use anchor_lang::prelude::*;
use anchor_lang::prelude::{AccountView, Address, ProgramResult};
use anchor_lang::solana_program::instruction::{AccountMeta, Instruction};
use anchor_spl::{token_2022_extensions as fees, token_interface as spl};

fn mint_state(mint: &AccountView, mo: &AccountView) -> ProgramResult {
    require(mint.owner() == &TOKEN_2022 && mint.data_len() == 278, 2)?;
    let d = mint.try_borrow()?;
    require(
        d[..4] == [0; 4]
            && d[45] == 1
            && d[46..50] == [0; 4]
            && all_zero(&d[82..165])
            && d[165] == 1
            && rd16(&d, 166) == 1
            && rd16(&d, 168) == 108
            && &d[170..202] == mo.address().as_ref()
            && &d[202..234] == mo.address().as_ref(),
        2,
    )
}

fn reserve_accounts(
    program: &Address,
    m: &AccountView,
    mo: &AccountView,
    mint: &AccountView,
    backing: &AccountView,
    quote: &AccountView,
) -> Result<([u8; 32], u8)> {
    let md = market(m, program)?;
    let d = model(mo, program, m.address())?;
    require(
        d.len() == RESERVE_MODEL_LEN
            && &d[176..208] == mint.address().as_ref()
            && &d[144..176] == backing.address().as_ref()
            && &md[40..72] == quote.address().as_ref(),
        2,
    )?;
    mint_state(mint, mo)?;
    token::vault_owned_by(backing, quote.address(), mo.address())?;
    Ok((d[40..72].try_into().unwrap(), d[140]))
}

pub(crate) fn activate(
    ctx: &mut anchor_lang::Context<crate::contexts::ActivateReserve>,
    p: &[u8],
) -> ProgramResult {
    crate::helpers::require(ctx.remaining_accounts()?.is_empty(), 3)?;
    let program = ctx.program_id;
    let m = ctx.accounts.m.account();
    let mut mo = *ctx.accounts.mo.account();
    let mo = &mut mo;
    let admin = ctx.accounts.admin.account();
    let mut payer = *ctx.accounts.payer.account();
    let payer = &mut payer;
    let system = ctx.accounts.system.account();
    let mut mint = *ctx.accounts.mint.account();
    let mint = &mut mint;
    let mut backing = *ctx.accounts.backing.account();
    let backing = &mut backing;
    let mut allocation = *ctx.accounts.allocation.account();
    let allocation = &mut allocation;
    let quote = ctx.accounts.quote.account();
    let token = ctx.accounts.token.account();
    let t22 = ctx.accounts.t22.account();
    let dex = ctx.accounts.dex.account();
    let mut curve_payer = *ctx.accounts.curve_payer.account();
    let curve_payer = &mut curve_payer;
    let curve_config = ctx.accounts.curve_config.account();
    let mut curve_pool = *ctx.accounts.curve_pool.account();
    let curve_pool = &mut curve_pool;
    let curve_authority = ctx.accounts.curve_authority.account();
    let curve_mint = ctx.accounts.curve_mint.account();
    let curve_quote = ctx.accounts.curve_quote.account();
    let mut curve_source = *ctx.accounts.curve_source.account();
    let curve_source = &mut curve_source;
    let curve_quote_source = ctx.accounts.curve_quote_source.account();
    let mut curve_base_vault = *ctx.accounts.curve_base_vault.account();
    let curve_base_vault = &mut curve_base_vault;
    let mut curve_quote_vault = *ctx.accounts.curve_quote_vault.account();
    let curve_quote_vault = &mut curve_quote_vault;
    let mut curve_lp_mint = *ctx.accounts.curve_lp_mint.account();
    let curve_lp_mint = &mut curve_lp_mint;
    let mut curve_lp = *ctx.accounts.curve_lp.account();
    let curve_lp = &mut curve_lp;
    let curve_token = ctx.accounts.curve_token.account();
    let curve_quote_token = ctx.accounts.curve_quote_token.account();
    let curve_lp_token = ctx.accounts.curve_lp_token.account();
    let dex_global = ctx.accounts.dex_global.account();
    require(
        p.len() == 59
            && rd64(p, 0) > 0
            && rd16(p, 9) <= 10_000
            && rd64(p, 19) > 0
            && rd64(p, 19) <= rd64(p, 0)
            && system.address() == &anchor_lang::system_program::ID
            && token.address() == &anchor_spl::token::ID
            && t22.address() == &TOKEN_2022,
        3,
    )?;
    let md = market(m, program)?;
    require(
        md.len() == FEE_MARKET_LEN && &md[40..72] == quote.address().as_ref(),
        2,
    )?;
    super::fees::admin(&md, admin)?;
    let treasury = addr(&md, 72);
    require(
        &md[216..248] == dex.address().as_ref()
            && &md[248..280] == curve_config.address().as_ref()
            && curve_payer.address() == &treasury
            && curve_mint.address() == mint.address()
            && curve_quote.address() == quote.address()
            && curve_source.address() == allocation.address()
            && curve_quote_source.address() != backing.address()
            && curve_quote_vault.address() != backing.address(),
        2,
    )?;
    let d = model(mo, program, m.address())?;
    require(d.len() == MODEL_LEN, 7)?;
    let id: [u8; 32] = d[40..72].try_into().unwrap();
    let bump = [d[140]];
    let model_seeds: &[&[u8]] = &[b"model", m.address().as_ref(), &id, &bump];
    drop((md, d));
    writable(&[mo, mint, backing, allocation])?;
    for (account, owner, len) in [
        (backing as &AccountView, &anchor_spl::token::ID, 165),
        (allocation as &AccountView, &TOKEN_2022, 178),
    ] {
        require(
            account.owner() == owner
                && account.data_len() == len
                && all_zero(&account.try_borrow()?),
            2,
        )?;
    }
    let (expected, mint_bump) =
        Address::derive_program_address(&[b"reserve_mint", mo.address().as_ref()], program)
            .ok_or(err(2))?;
    require(mint.address() == &expected, 2)?;
    let mint_bump = [mint_bump];
    let mint_seeds: &[&[u8]] = &[b"reserve_mint", mo.address().as_ref(), &mint_bump];
    initialize_pda(mint, payer, &TOKEN_2022, 278, mint_seeds)?;
    fees::transfer_fee_initialize(
        CpiContext::new(
            &TOKEN_2022,
            fees::TransferFeeInitialize {
                mint: mint.try_to_cpi_handle_mut()?,
            },
        ),
        Some(mo.address()),
        Some(mo.address()),
        rd16(p, 9),
        rd64(p, 11),
    )?;
    spl::initialize_mint2(
        CpiContext::new(
            &TOKEN_2022,
            spl::InitializeMint2 {
                mint: mint.try_to_cpi_handle_mut()?,
            },
        ),
        p[8],
        mo.address(),
        None,
    )?;
    spl::initialize_account3(CpiContext::new(
        &anchor_spl::token::ID,
        spl::InitializeAccount3 {
            account: backing.try_to_cpi_handle_mut()?,
            mint: quote.to_cpi_handle(),
            authority: mo.to_cpi_handle(),
        },
    ))?;
    spl::initialize_account3(CpiContext::new(
        &TOKEN_2022,
        spl::InitializeAccount3 {
            account: allocation.try_to_cpi_handle_mut()?,
            mint: mint.to_cpi_handle(),
            authority: curve_payer.to_cpi_handle(),
        },
    ))?;
    require(
        curve_base_vault.owner() == &TOKEN_2022
            && curve_base_vault.data_len() == 178
            && all_zero(&curve_base_vault.try_borrow()?),
        2,
    )?;
    let mut base_vault = *curve_base_vault;
    spl::initialize_account3(CpiContext::new(
        &TOKEN_2022,
        spl::InitializeAccount3 {
            account: base_vault.try_to_cpi_handle_mut()?,
            mint: mint.to_cpi_handle(),
            authority: curve_authority.to_cpi_handle(),
        },
    ))?;
    spl::mint_to_checked(
        CpiContext::new_with_signer(
            &TOKEN_2022,
            spl::MintToChecked {
                mint: mint.try_to_cpi_handle_mut()?,
                to: allocation.try_to_cpi_handle_mut()?,
                authority: mo.to_cpi_handle(),
            },
            &[model_seeds],
        ),
        rd64(p, 0),
        p[8],
    )?;
    crate::cross_program::revoke_mint(mint, mo, &TOKEN_2022, &[model_seeds])?;
    let mut launch = [0u8; 41];
    launch[0] = 9;
    launch[1..].copy_from_slice(&p[19..]);
    let mut forwarded = [
        *curve_payer,
        *curve_config,
        *curve_pool,
        *curve_authority,
        *curve_mint,
        *curve_quote,
        *curve_source,
        *curve_quote_source,
        *curve_base_vault,
        *curve_quote_vault,
        *curve_lp_mint,
        *curve_lp,
        *curve_token,
        *curve_quote_token,
        *curve_lp_token,
        *admin,
        *dex_global,
    ];
    let accounts = forwarded
        .iter()
        .enumerate()
        .map(|(i, account)| AccountMeta {
            pubkey: *account.address(),
            is_writable: i < 15 && account.is_writable(),
            is_signer: i == 15 || (i < 15 && account.is_signer()),
        })
        .collect();
    let handles = forwarded
        .iter_mut()
        .enumerate()
        .map(|(i, account)| {
            if i < 15 && account.is_writable() {
                CpiHandle::writable(account)
            } else {
                CpiHandle::readonly(account)
            }
        })
        .collect();
    CpiContext::new(
        dex.address(),
        ModelCurveLaunch {
            _phantom: core::marker::PhantomData,
        },
    )
    .with_remaining_accounts(handles)
    .invoke_ix(Instruction {
        program_id: *dex.address(),
        accounts,
        data: launch.to_vec(),
    })?;
    super::fees::grow(mo, payer, RESERVE_MODEL_LEN)?;
    let mut d = mo.try_borrow_mut()?;
    d[141] = 1;
    d[142..144].fill(0);
    d[144..176].copy_from_slice(backing.address().as_ref());
    d[176..208].copy_from_slice(mint.address().as_ref());
    Ok(())
}

pub(crate) fn configure(
    ctx: &mut anchor_lang::Context<crate::contexts::ConfigureReserve>,
    p: &[u8],
) -> ProgramResult {
    crate::helpers::require(ctx.remaining_accounts()?.is_empty(), 3)?;
    let program = ctx.program_id;
    let m = ctx.accounts.m.account();
    let mut mo = *ctx.accounts.mo.account();
    let mo = &mut mo;
    let admin = ctx.accounts.admin.account();
    let mut mint = *ctx.accounts.mint.account();
    let mint = &mut mint;
    let t22 = ctx.accounts.t22.account();
    require(
        p.len() == 11 && p[0] <= 1 && rd16(p, 1) <= 10_000 && t22.address() == &TOKEN_2022,
        3,
    )?;
    writable(&[mo, mint])?;
    let md = market(m, program)?;
    super::fees::admin(&md, admin)?;
    let d = model(mo, program, m.address())?;
    require(
        d.len() == RESERVE_MODEL_LEN && &d[176..208] == mint.address().as_ref(),
        2,
    )?;
    mint_state(mint, mo)?;
    require(p[0] == 0 || rd64(&mint.try_borrow()?, 36) > 0, 7)?;
    let id: [u8; 32] = d[40..72].try_into().unwrap();
    let bump = [d[140]];
    let signer: &[&[u8]] = &[b"model", m.address().as_ref(), &id, &bump];
    drop(d);
    fees::transfer_fee_set(
        CpiContext::new_with_signer(
            &TOKEN_2022,
            fees::TransferFeeSetTransferFee {
                mint: mint.try_to_cpi_handle_mut()?,
                authority: mo.to_cpi_handle(),
            },
            &[signer],
        ),
        rd16(p, 1),
        rd64(p, 3),
    )?;
    mo.try_borrow_mut()?[141] = p[0];
    Ok(())
}

pub(crate) fn redeem(
    ctx: &mut anchor_lang::Context<crate::contexts::RedeemReserve>,
    p: &[u8],
) -> ProgramResult {
    crate::helpers::require(ctx.remaining_accounts()?.is_empty(), 3)?;
    let program = ctx.program_id;
    let m = ctx.accounts.m.account();
    let mo = ctx.accounts.mo.account();
    let mut mint = *ctx.accounts.mint.account();
    let mint = &mut mint;
    let mut source = *ctx.accounts.source.account();
    let source = &mut source;
    let holder = ctx.accounts.holder.account();
    let mut backing = *ctx.accounts.backing.account();
    let backing = &mut backing;
    let mut destination = *ctx.accounts.destination.account();
    let destination = &mut destination;
    let quote = ctx.accounts.quote.account();
    let token = ctx.accounts.token.account();
    let t22 = ctx.accounts.t22.account();
    require(
        p.len() == 16
            && rd64(p, 0) > 0
            && token.address() == &anchor_spl::token::ID
            && t22.address() == &TOKEN_2022,
        3,
    )?;
    require(holder.is_signer(), 4)?;
    writable(&[mint, source, backing, destination])?;
    require(backing.address() != destination.address(), 2)?;
    let (id, bump) = reserve_accounts(program, m, mo, mint, backing, quote)?;
    payment_token(source, mint.address(), holder.address(), &TOKEN_2022)?;
    token::token_owned_by(destination, quote.address(), holder.address())?;
    let supply = rd64(&mint.try_borrow()?, 36);
    let amount = rd64(p, 0);
    require(amount <= supply && supply > 0, 8)?;
    let available = rd64(&backing.try_borrow()?, 64);
    let out = (u128::from(available) * u128::from(amount) / u128::from(supply)) as u64;
    require(out > 0 && out >= rd64(p, 8), 8)?;
    let decimals = mint.try_borrow()?[44];
    spl::burn_checked(
        CpiContext::new(
            &TOKEN_2022,
            spl::BurnChecked {
                from: source.try_to_cpi_handle_mut()?,
                mint: mint.try_to_cpi_handle_mut()?,
                authority: holder.to_cpi_handle(),
            },
        ),
        amount,
        decimals,
    )?;
    let bump = [bump];
    let signer: &[&[u8]] = &[b"model", m.address().as_ref(), &id, &bump];
    crate::cross_program::transfer_checked(
        backing,
        quote,
        destination,
        mo,
        &anchor_spl::token::ID,
        out,
        6,
        &[signer],
    )
}

pub(crate) fn collect(
    ctx: &mut anchor_lang::Context<crate::contexts::CollectReserveTax>,
    p: &[u8],
) -> ProgramResult {
    crate::helpers::require(ctx.remaining_accounts()?.is_empty(), 3)?;
    let program = ctx.program_id;
    let m = ctx.accounts.m.account();
    let mo = ctx.accounts.mo.account();
    let mut mint = *ctx.accounts.mint.account();
    let mint = &mut mint;
    let mut destination = *ctx.accounts.destination.account();
    let destination = &mut destination;
    let t22 = ctx.accounts.t22.account();
    require(p.is_empty() && t22.address() == &TOKEN_2022, 3)?;
    writable(&[mint, destination])?;
    let md = market(m, program)?;
    require(md.len() == FEE_MARKET_LEN, 2)?;
    let d = model(mo, program, m.address())?;
    require(
        d.len() == RESERVE_MODEL_LEN && &d[176..208] == mint.address().as_ref(),
        2,
    )?;
    mint_state(mint, mo)?;
    payment_token(destination, mint.address(), &addr(&md, 104), &TOKEN_2022)?;
    let id: [u8; 32] = d[40..72].try_into().unwrap();
    let bump = [d[140]];
    let signer: &[&[u8]] = &[b"model", m.address().as_ref(), &id, &bump];
    drop(d);
    fees::withdraw_withheld_tokens_from_mint(CpiContext::new_with_signer(
        &TOKEN_2022,
        fees::WithdrawWithheldTokensFromMint {
            mint: mint.try_to_cpi_handle_mut()?,
            destination: destination.try_to_cpi_handle_mut()?,
            authority: mo.to_cpi_handle(),
        },
        &[signer],
    ))
}

pub(crate) fn sweep_empty(
    ctx: &mut anchor_lang::Context<crate::contexts::SweepReserve>,
    p: &[u8],
) -> ProgramResult {
    crate::helpers::require(ctx.remaining_accounts()?.is_empty(), 3)?;
    let program = ctx.program_id;
    let m = ctx.accounts.m.account();
    let mo = ctx.accounts.mo.account();
    let mint = ctx.accounts.mint.account();
    let mut backing = *ctx.accounts.backing.account();
    let backing = &mut backing;
    let mut destination = *ctx.accounts.destination.account();
    let destination = &mut destination;
    let quote = ctx.accounts.quote.account();
    let token = ctx.accounts.token.account();
    require(
        p.is_empty()
            && token.address() == &anchor_spl::token::ID
            && backing.address() != destination.address(),
        3,
    )?;
    writable(&[backing, destination])?;
    let (id, bump) = reserve_accounts(program, m, mo, mint, backing, quote)?;
    require(rd64(&mint.try_borrow()?, 36) == 0, 7)?;
    let md = market(m, program)?;
    require(md.len() == FEE_MARKET_LEN, 2)?;
    token::token_owned_by(destination, quote.address(), &addr(&md, 72))?;
    let amount = rd64(&backing.try_borrow()?, 64);
    if amount == 0 {
        return Ok(());
    }
    let bump = [bump];
    let signer: &[&[u8]] = &[b"model", m.address().as_ref(), &id, &bump];
    crate::cross_program::transfer_checked(
        backing,
        quote,
        destination,
        mo,
        &anchor_spl::token::ID,
        amount,
        6,
        &[signer],
    )
}

#[derive(ToCpiAccounts)]
struct ModelCurveLaunch<'a> {
    _phantom: core::marker::PhantomData<&'a ()>,
}
