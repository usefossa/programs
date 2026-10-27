use crate::{
    abi::*,
    helpers::{addr, err, rd16, rd32, rd64, require},
    state::{market, writable},
    token::{payment_mint, payment_token, payment_transfer},
};
use anchor_lang::prelude::Rent;
use anchor_lang::prelude::{AccountView, ProgramResult};
use anchor_lang::solana_program::sysvar::Sysvar as _;
use anchor_lang::AnchorAccount;

pub(crate) fn admin(md: &[u8], signer: &AccountView) -> ProgramResult {
    require(
        signer.is_signer() && &md[8..40] == signer.address().as_ref(),
        4,
    )
}
pub(crate) fn rotate_admin(
    ctx: &mut anchor_lang::Context<crate::contexts::RotateAdmin>,
    p: &[u8],
) -> ProgramResult {
    require(ctx.remaining_accounts()?.is_empty() && p.is_empty(), 3)?;
    let accounts = &mut ctx.accounts;
    require(accounts.current.address() != accounts.next.address(), 3)?;
    require(
        matches!(accounts.m.account().data_len(), MARKET_LEN | FEE_MARKET_LEN),
        2,
    )?;
    require(accounts.m.admin == *accounts.current.address(), 4)?;
    accounts.m.admin = *accounts.next.address();
    Ok(())
}

pub(crate) fn grow(account: &mut AccountView, payer: &AccountView, len: usize) -> ProgramResult {
    writable(&[account, payer])?;
    require(payer.is_signer(), 4)?;
    if account.data_len() == len {
        return Ok(());
    }
    let rent = Rent::get()?.try_minimum_balance(len)?;
    if account.lamports() < rent {
        crate::cross_program::transfer_lamports(payer, account, rent - account.lamports())?;
    }
    anchor_lang::cpi::realloc_account(account, len, payer, true)
}

pub(crate) fn validate_terms(fee: u16, protocol_share: u16) -> ProgramResult {
    require(
        (MIN_LIST_FEE_BPS..=MAX_LIST_FEE_BPS).contains(&fee)
            && (MIN_PROTOCOL_SHARE_BPS..=10_000).contains(&protocol_share),
        3,
    )
}

pub(crate) fn configure(
    ctx: &mut anchor_lang::Context<crate::contexts::ConfigureFees>,
    p: &[u8],
) -> ProgramResult {
    let mut remaining = ctx.remaining_accounts()?;
    let extra = remaining.as_mut_slice();
    let mut m = *ctx.accounts.m.account();
    let m = &mut m;
    let signer = ctx.accounts.signer.account();
    let mut payer = *ctx.accounts.payer.account();
    let payer = &mut payer;
    let system = ctx.accounts.system.account();
    let program = ctx.program_id;
    require(
        p.len() == 137 && p[136] <= 1 && p[..32] != [0; 32] && p[32..64] != [0; 32],
        3,
    )?;
    require(extra.len() == if p[136] == 1 { 2 } else { 0 }, 3)?;
    require(system.address() == &anchor_lang::system_program::ID, 3)?;
    let md = market(m, program)?;
    admin(&md, signer)?;
    if p[136] == 1 {
        require(
            rd64(p, 128) > 0
                && &p[64..96] == extra[0].address().as_ref()
                && &p[96..128] == extra[1].address().as_ref(),
            3,
        )?;
        payment_mint(&extra[0], extra[1].address())?;
    }
    drop(md);
    grow(m, payer, FEE_MARKET_LEN)?;
    let mut d = m.try_borrow_mut()?;
    d[72..209].copy_from_slice(p);
    d[209..216].fill(0);
    Ok(())
}

pub(crate) fn configure_dex(
    ctx: &mut anchor_lang::Context<crate::contexts::ConfigureDex>,
    p: &[u8],
) -> ProgramResult {
    crate::helpers::require(ctx.remaining_accounts()?.is_empty(), 3)?;
    let program = ctx.program_id;
    let mut m = *ctx.accounts.m.account();
    let m = &mut m;
    let admin = ctx.accounts.admin.account();
    let dex = ctx.accounts.dex.account();
    let config = ctx.accounts.config.account();
    require(
        p.is_empty()
            && dex.executable()
            && config.owner() == dex.address()
            && config.data_len() == 192,
        3,
    )?;
    writable(&[m])?;
    let md = market(m, program)?;
    require(md.len() == FEE_MARKET_LEN, 2)?;
    self::admin(&md, admin)?;
    let d = config.try_borrow()?;
    require(
        &d[..8] == b"FDEXCFG1" && d[8..40] == md[8..40] && d[72..104] == md[40..72],
        2,
    )?;
    drop((d, md));
    let mut d = m.try_borrow_mut()?;
    d[216..248].copy_from_slice(dex.address().as_ref());
    d[248..280].copy_from_slice(config.address().as_ref());
    Ok(())
}

pub(crate) fn configure_list(
    ctx: &mut anchor_lang::Context<crate::contexts::ConfigureListFees>,
    p: &[u8],
) -> ProgramResult {
    crate::helpers::require(ctx.remaining_accounts()?.is_empty(), 3)?;
    let program = ctx.program_id;
    let m = ctx.accounts.m.account();
    let mut list = *ctx.accounts.list.account();
    let list = &mut list;
    let authority = ctx.accounts.authority.account();
    let mut payer = *ctx.accounts.payer.account();
    let payer = &mut payer;
    let system = ctx.accounts.system.account();
    require(
        p.len() == 4 && system.address() == &anchor_lang::system_program::ID,
        3,
    )?;
    validate_terms(rd16(p, 0), rd16(p, 2))?;
    require(market(m, program)?.len() == FEE_MARKET_LEN, 7)?;
    let d = super::list::state(list, program)?;
    require(&d[8..40] == m.address().as_ref(), 2)?;
    require(
        authority.is_signer() && &d[40..72] == authority.address().as_ref(),
        4,
    )?;
    let at = provider_list_len(rd32(&d, 72) as usize);
    drop(d);
    grow(list, payer, at + LIST_FEE_LEN)?;
    list.try_borrow_mut()?[at..at + 4].copy_from_slice(p);
    Ok(())
}

pub(crate) fn creation_payment(
    md: &[u8],
    authority: &AccountView,
    accounts: &[AccountView],
    maximum: u64,
) -> ProgramResult {
    if md[208] == 0 {
        return require(accounts.is_empty(), 3);
    }
    let [source, destination, mint, token] = accounts else {
        return Err(err(3));
    };
    writable(&[source, destination])?;
    require(
        &md[136..168] == mint.address().as_ref()
            && &md[168..200] == token.address().as_ref()
            && source.address() != destination.address(),
        2,
    )?;
    let (decimals, amount) = payment_mint(mint, token.address())?.gross(rd64(md, 200))?;
    require(amount <= maximum, 8)?;
    payment_token(source, mint.address(), authority.address(), token.address())?;
    payment_token(destination, mint.address(), &addr(md, 72), token.address())?;
    let before = rd64(&destination.try_borrow()?, 64);
    payment_transfer(
        source,
        mint,
        destination,
        authority,
        token.address(),
        amount,
        decimals,
    )?;
    require(
        rd64(&destination.try_borrow()?, 64).checked_sub(before) == Some(rd64(md, 200)),
        8,
    )
}
