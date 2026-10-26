use crate::{
    abi::*,
    helpers::{addr, err, rd16, rd64, require},
    state::{all_zero, initialize_pda, market, model, writable},
    token::{self, payment_token, raw, TOKEN_2022},
};
use pinocchio::{cpi::Signer, instruction::seeds, AccountView, Address, ProgramResult};

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
) -> Result<([u8; 32], u8), pinocchio::error::ProgramError> {
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

pub(crate) fn activate(program: &Address, accounts: &mut [AccountView], p: &[u8]) -> ProgramResult {
    require(accounts.len() == 28, 3)?;
    let (core, curve) = accounts.split_at_mut(12);
    let [m, mo, admin, payer, system, mint, backing, allocation, quote, token, t22, dex] = core
    else {
        return Err(err(3));
    };
    require(
        p.len() == 59
            && rd64(p, 0) > 0
            && rd16(p, 9) <= 10_000
            && rd64(p, 19) > 0
            && rd64(p, 19) <= rd64(p, 0)
            && system.address() == &pinocchio_system::ID
            && token.address() == &pinocchio_token::ID
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
            && &md[248..280] == curve[1].address().as_ref()
            && curve[0].address() == &treasury
            && curve[4].address() == mint.address()
            && curve[5].address() == quote.address()
            && curve[6].address() == allocation.address()
            && curve[7].address() != backing.address()
            && curve[9].address() != backing.address(),
        2,
    )?;
    let d = model(mo, program, m.address())?;
    require(d.len() == MODEL_LEN, 7)?;
    let id: [u8; 32] = d[40..72].try_into().unwrap();
    let bump = [d[140]];
    let model_seeds = seeds!(b"model", m.address().as_ref(), &id, &bump);
    drop((md, d));
    writable(&[mo, mint, backing, allocation])?;
    for (account, owner, len) in [
        (backing as &AccountView, &pinocchio_token::ID, 165),
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
    let mint_seeds = seeds!(b"reserve_mint", mo.address().as_ref(), &mint_bump);
    initialize_pda(mint, payer, &TOKEN_2022, 278, Signer::from(&mint_seeds))?;
    let mut fee = [0u8; 78];
    fee[..3].copy_from_slice(&[26, 0, 1]);
    fee[3..35].copy_from_slice(mo.address().as_ref());
    fee[35] = 1;
    fee[36..68].copy_from_slice(mo.address().as_ref());
    fee[68..].copy_from_slice(&p[9..19]);
    raw(&TOKEN_2022, [mint], [(true, false)], &fee, &[])?;
    let mut init = [0u8; 35];
    init[0] = 20;
    init[1] = p[8];
    init[2..34].copy_from_slice(mo.address().as_ref());
    raw(&TOKEN_2022, [mint], [(true, false)], &init, &[])?;
    let mut init_account = [0u8; 33];
    init_account[0] = 18;
    init_account[1..].copy_from_slice(mo.address().as_ref());
    raw(
        &pinocchio_token::ID,
        [backing, quote],
        [(true, false), (false, false)],
        &init_account,
        &[],
    )?;
    init_account[1..].copy_from_slice(treasury.as_ref());
    raw(
        &TOKEN_2022,
        [allocation, mint],
        [(true, false), (false, false)],
        &init_account,
        &[],
    )?;
    require(
        curve[8].owner() == &TOKEN_2022
            && curve[8].data_len() == 178
            && all_zero(&curve[8].try_borrow()?),
        2,
    )?;
    init_account[1..].copy_from_slice(curve[3].address().as_ref());
    raw(
        &TOKEN_2022,
        [&curve[8], mint],
        [(true, false), (false, false)],
        &init_account,
        &[],
    )?;
    let mut issue = [0u8; 10];
    issue[0] = 14;
    issue[1..9].copy_from_slice(&p[..8]);
    issue[9] = p[8];
    raw(
        &TOKEN_2022,
        [mint, allocation, mo],
        [(true, false), (true, false), (false, true)],
        &issue,
        &[Signer::from(&model_seeds)],
    )?;
    raw(
        &TOKEN_2022,
        [mint, mo],
        [(true, false), (false, true)],
        &[6, 0, 0],
        &[Signer::from(&model_seeds)],
    )?;
    let mut launch = [0u8; 41];
    launch[0] = 9;
    launch[1..].copy_from_slice(&p[19..]);
    let views: [&AccountView; 17] = core::array::from_fn(|i| {
        if i == 15 {
            &*admin
        } else if i == 16 {
            &curve[15]
        } else {
            &curve[i]
        }
    });
    let flags = core::array::from_fn(|i| {
        if i == 15 {
            (false, true)
        } else if i == 16 {
            (false, false)
        } else {
            (curve[i].is_writable(), curve[i].is_signer())
        }
    });
    raw(dex.address(), views, flags, &launch, &[])?;
    super::fees::grow(mo, payer, RESERVE_MODEL_LEN)?;
    let mut d = mo.try_borrow_mut()?;
    d[141] = 1;
    d[142..144].fill(0);
    d[144..176].copy_from_slice(backing.address().as_ref());
    d[176..208].copy_from_slice(mint.address().as_ref());
    Ok(())
}

pub(crate) fn configure(
    program: &Address,
    accounts: &mut [AccountView],
    p: &[u8],
) -> ProgramResult {
    let [m, mo, admin, mint, t22] = accounts else {
        return Err(err(3));
    };
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
    let signer = seeds!(b"model", m.address().as_ref(), &id, &bump);
    drop(d);
    let mut data = [0u8; 12];
    data[..2].copy_from_slice(&[26, 5]);
    data[2..].copy_from_slice(&p[1..]);
    raw(
        &TOKEN_2022,
        [mint, mo],
        [(true, false), (false, true)],
        &data,
        &[Signer::from(&signer)],
    )?;
    mo.try_borrow_mut()?[141] = p[0];
    Ok(())
}

pub(crate) fn redeem(program: &Address, accounts: &mut [AccountView], p: &[u8]) -> ProgramResult {
    let [m, mo, mint, source, holder, backing, destination, quote, token, t22] = accounts else {
        return Err(err(3));
    };
    require(
        p.len() == 16
            && rd64(p, 0) > 0
            && token.address() == &pinocchio_token::ID
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
    let mut burn = [0u8; 10];
    burn[0] = 15;
    burn[1..9].copy_from_slice(&p[..8]);
    burn[9] = mint.try_borrow()?[44];
    raw(
        &TOKEN_2022,
        [source, mint, holder],
        [(true, false), (true, false), (false, true)],
        &burn,
        &[],
    )?;
    let bump = [bump];
    let signer = seeds!(b"model", m.address().as_ref(), &id, &bump);
    pinocchio_token::instructions::transfer_checked::TransferChecked::<
        &AccountView,
        pinocchio_token::TokenProgram,
    >::new(backing, quote, destination, mo, out, 6)
    .invoke_signed(&[Signer::from(&signer)])
}

pub(crate) fn collect(program: &Address, accounts: &mut [AccountView], p: &[u8]) -> ProgramResult {
    let [m, mo, mint, destination, t22] = accounts else {
        return Err(err(3));
    };
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
    let signer = seeds!(b"model", m.address().as_ref(), &id, &bump);
    drop(d);
    raw(
        &TOKEN_2022,
        [mint, destination, mo],
        [(true, false), (true, false), (false, true)],
        &[26, 2],
        &[Signer::from(&signer)],
    )
}

pub(crate) fn sweep_empty(
    program: &Address,
    accounts: &mut [AccountView],
    p: &[u8],
) -> ProgramResult {
    let [m, mo, mint, backing, destination, quote, token] = accounts else {
        return Err(err(3));
    };
    require(
        p.is_empty()
            && token.address() == &pinocchio_token::ID
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
    let signer = seeds!(b"model", m.address().as_ref(), &id, &bump);
    pinocchio_token::instructions::transfer_checked::TransferChecked::<
        &AccountView,
        pinocchio_token::TokenProgram,
    >::new(backing, quote, destination, mo, amount, 6)
    .invoke_signed(&[Signer::from(&signer)])
}
