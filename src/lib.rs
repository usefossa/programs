#![cfg_attr(not(any(test, feature = "no-entrypoint")), no_std)]
#![allow(unexpected_cfgs)]

pub mod abi;
#[cfg(feature = "receipt-prototype")]
mod receipt_prototype;
use abi::*;
use pinocchio::{
    account::Ref,
    cpi::Signer,
    error::ProgramError,
    instruction::seeds,
    sysvars::{clock::Clock, rent::Rent, Sysvar},
    AccountView, Address, ProgramResult,
};
use pinocchio_system::instructions::{Allocate, Assign, Transfer};
use pinocchio_token::{
    instructions::transfer_checked::TransferChecked,
    state::{Account as TokenAccount, Mint},
    TokenProgram,
};

#[cfg(not(any(test, feature = "no-entrypoint")))]
pinocchio::program_entrypoint!(process_instruction, 12);
#[cfg(not(any(test, feature = "no-entrypoint")))]
pinocchio::default_allocator!();
#[cfg(not(any(test, feature = "no-entrypoint")))]
pinocchio::nostd_panic_handler!();

#[cfg(feature = "local-test")]
pub const CLAIM_SECONDS: i64 = 30;
#[cfg(not(feature = "local-test"))]
pub const CLAIM_SECONDS: i64 = 300;
#[cfg(feature = "local-test")]
pub const DISPUTE_SECONDS: i64 = 8;
#[cfg(not(feature = "local-test"))]
pub const DISPUTE_SECONDS: i64 = 3_600;
#[cfg(feature = "local-test")]
pub const UNBOND_SECONDS: i64 = 5;
#[cfg(not(feature = "local-test"))]
pub const UNBOND_SECONDS: i64 = 3_600;

fn err(n: u32) -> ProgramError {
    ProgramError::Custom(n)
}
fn require(ok: bool, n: u32) -> ProgramResult {
    if ok {
        Ok(())
    } else {
        Err(err(n))
    }
}
fn rd16(b: &[u8], i: usize) -> u16 {
    u16::from_le_bytes(b[i..i + 2].try_into().unwrap())
}
fn rd32(b: &[u8], i: usize) -> u32 {
    u32::from_le_bytes(b[i..i + 4].try_into().unwrap())
}
fn rd64(b: &[u8], i: usize) -> u64 {
    u64::from_le_bytes(b[i..i + 8].try_into().unwrap())
}
fn ri64(b: &[u8], i: usize) -> i64 {
    i64::from_le_bytes(b[i..i + 8].try_into().unwrap())
}
fn wr32(b: &mut [u8], i: usize, v: u32) {
    b[i..i + 4].copy_from_slice(&v.to_le_bytes());
}
fn wr64(b: &mut [u8], i: usize, v: u64) {
    b[i..i + 8].copy_from_slice(&v.to_le_bytes());
}
fn wi64(b: &mut [u8], i: usize, v: i64) {
    b[i..i + 8].copy_from_slice(&v.to_le_bytes());
}
fn addr(b: &[u8], i: usize) -> Address {
    Address::new_from_array(b[i..i + 32].try_into().unwrap())
}
fn add(a: u64, b: u64) -> Result<u64, ProgramError> {
    a.checked_add(b).ok_or(err(1))
}
fn sub(a: u64, b: u64) -> Result<u64, ProgramError> {
    a.checked_sub(b).ok_or(err(1))
}
fn inc(a: u32) -> Result<u32, ProgramError> {
    a.checked_add(1).ok_or(err(1))
}
fn dec(a: u32) -> Result<u32, ProgramError> {
    a.checked_sub(1).ok_or(err(1))
}
#[allow(
    clippy::manual_div_ceil,
    reason = "bounded addition removes sbf remainder reconstruction"
)]
fn fee(tokens: u32, rate: u32) -> u64 {
    // maximum u32 product plus 999_999 fits u64; all caller operands have those bounds.
    (u64::from(tokens) * u64::from(rate) + UNIT - 1) / UNIT
}
fn collateral(exposure: u64) -> Result<u64, ProgramError> {
    add(exposure, exposure.div_ceil(100))
}
fn writable(accounts: &[&AccountView]) -> ProgramResult {
    require(accounts.iter().all(|a| a.is_writable()), 2)
}
fn state<'a>(
    a: &'a AccountView,
    program: &Address,
    len: usize,
    magic: &[u8; 8],
) -> Result<Ref<'a, [u8]>, ProgramError> {
    require(a.owner() == program && a.data_len() == len, 2)?;
    let d = a.try_borrow()?;
    require(&d[..8] == magic, 2)?;
    Ok(d)
}
fn fresh(a: &AccountView, program: &Address, len: usize) -> ProgramResult {
    require(
        a.is_signer() && a.is_writable() && a.owner() == program && a.data_len() == len,
        2,
    )?;
    require(all_zero(&a.try_borrow()?), 2)
}
fn all_zero(data: &[u8]) -> bool {
    let mut words = data.chunks_exact(8);
    words.all(|w| u64::from_le_bytes(w.try_into().unwrap()) == 0)
        && words.remainder().iter().all(|b| *b == 0)
}
fn market<'a>(a: &'a AccountView, program: &Address) -> Result<Ref<'a, [u8]>, ProgramError> {
    state(a, program, MARKET_LEN, MARKET_MAGIC)
}
fn model<'a>(
    a: &'a AccountView,
    program: &Address,
    m: &Address,
) -> Result<Ref<'a, [u8]>, ProgramError> {
    let d = state(a, program, MODEL_LEN, MODEL_MAGIC)?;
    require(
        &d[8..40] == m.as_ref()
            && a.address()
                == &Address::derive_address(
                    &[b"model", m.as_ref(), &d[40..72]],
                    Some(d[140]),
                    program,
                ),
        2,
    )?;
    Ok(d)
}
fn profile<'a>(
    a: &'a AccountView,
    program: &Address,
    m: &Address,
) -> Result<Ref<'a, [u8]>, ProgramError> {
    let d = state(a, program, PROVIDER_LEN, PROVIDER_MAGIC)?;
    require(
        &d[8..40] == m.as_ref()
            && a.address()
                == &Address::derive_address(
                    &[b"provider", m.as_ref(), &d[40..72]],
                    Some(d[107]),
                    program,
                ),
        2,
    )?;
    Ok(d)
}
fn approved_provider(
    a: &AccountView,
    provider: &AccountView,
    m: &Address,
    program: &Address,
) -> ProgramResult {
    let d = profile(a, program, m)?;
    require(
        provider.is_signer() && &d[40..72] == provider.address().as_ref() && d[106] == 1,
        4,
    )
}
fn metadata_payload(p: &[u8]) -> ProgramResult {
    require((33..=32 + URI_MAX).contains(&p.len()), 3)?;
    require(
        core::str::from_utf8(&p[32..]).is_ok()
            && p[32..]
                .iter()
                .all(|b| !b.is_ascii_control() && !b.is_ascii_whitespace()),
        3,
    )
}
fn write_metadata(d: &mut [u8], hash: usize, uri: usize, p: &[u8]) {
    d[hash..hash + 32].copy_from_slice(&p[..32]);
    d[hash + 32..hash + 34].copy_from_slice(&((p.len() - 32) as u16).to_le_bytes());
    let end = uri + p.len() - 32;
    d[uri..end].copy_from_slice(&p[32..]);
    d[end..].fill(0);
}
fn initialize_pda(
    a: &AccountView,
    payer: &AccountView,
    program: &Address,
    len: usize,
    signer: Signer<'_, '_>,
) -> ProgramResult {
    writable(&[a, payer])?;
    require(payer.is_signer(), 4)?;
    require(a.owner() == &pinocchio_system::ID && a.data_len() == 0, 2)?;
    let rent = Rent::get()?.try_minimum_balance(len)?;
    if a.lamports() < rent {
        Transfer {
            from: payer,
            to: a,
            lamports: rent - a.lamports(),
        }
        .invoke()?;
    }
    Allocate {
        account: a,
        space: len as u64,
    }
    .invoke_signed(core::slice::from_ref(&signer))?;
    Assign {
        account: a,
        owner: program,
    }
    .invoke_signed(&[signer])
}
fn book<'a>(
    a: &'a AccountView,
    program: &Address,
    model: &AccountView,
    md: &[u8],
    terms: bool,
) -> Result<Ref<'a, [u8]>, ProgramError> {
    let limit = rd32(md, 136) as usize;
    require((1..=MAX_ENTRIES).contains(&limit), 2)?;
    let (offset, len, magic) = if terms {
        (72, terms_book_len(limit), TERMS_MAGIC)
    } else {
        (104, capacity_book_len(limit), CAPACITY_MAGIC)
    };
    require(&md[offset..offset + 32] == a.address().as_ref(), 2)?;
    let d = state(a, program, len, magic)?;
    require(
        &d[8..40] == model.address().as_ref()
            && rd32(&d, 40) as usize == limit
            && rd32(&d, 44) as usize <= limit,
        2,
    )?;
    Ok(d)
}
fn slot_offset(d: &[u8], slot: u32, terms: bool) -> Result<usize, ProgramError> {
    require(slot < rd32(d, 44), 2)?;
    Ok(HEADER_LEN + slot as usize * if terms { TERMS_LEN } else { CAPACITY_LEN })
}
fn user<'a>(
    a: &'a AccountView,
    program: &Address,
    owner: &Address,
    m: &Address,
) -> Result<Ref<'a, [u8]>, ProgramError> {
    let d = state(a, program, USER_LEN, USER_MAGIC)?;
    require(&d[8..40] == owner.as_ref() && &d[40..72] == m.as_ref(), 2)?;
    Ok(d)
}
fn booking<'a>(
    a: &'a AccountView,
    program: &Address,
    m: &Address,
    model: &Address,
) -> Result<Ref<'a, [u8]>, ProgramError> {
    let d = state(a, program, BOOKING_LEN, BOOK_MAGIC)?;
    require(
        &d[72..104] == m.as_ref() && &d[224..256] == model.as_ref(),
        2,
    )?;
    Ok(d)
}
fn booked_user<'a>(
    u: &'a AccountView,
    program: &Address,
    bd: &[u8],
    m: &Address,
) -> Result<Ref<'a, [u8]>, ProgramError> {
    require(&bd[192..224] == u.address().as_ref(), 2)?;
    user(u, program, &addr(bd, 8), m)
}
fn token_owned_by(token: &AccountView, mint: &Address, owner: &Address) -> ProgramResult {
    let d = TokenAccount::from_account_view(token)?;
    require(
        d.is_initialized() && d.mint() == mint && d.owner() == owner,
        2,
    )
}
fn vault_owned_by(vault: &AccountView, mint: &Address, authority: &Address) -> ProgramResult {
    token_owned_by(vault, mint, authority)?;
    let d = TokenAccount::from_account_view(vault)?;
    require(
        !d.has_delegate() && !d.has_close_authority() && !d.is_native(),
        2,
    )
}
fn user_vault(
    vault: &AccountView,
    mint: &AccountView,
    md: &[u8],
    u: &AccountView,
    ud: &[u8],
    program: &Address,
) -> ProgramResult {
    require(
        &ud[80..112] == vault.address().as_ref() && &md[40..72] == mint.address().as_ref(),
        2,
    )?;
    let authority =
        Address::derive_address(&[b"vault", u.address().as_ref()], Some(ud[112]), program);
    vault_owned_by(vault, mint.address(), &authority)
}
fn bond_vault(
    accounts: [&AccountView; 2],
    md: &[u8],
    model: &Address,
    slot: u32,
    stored: &[u8],
    bump: u8,
    program: &Address,
) -> ProgramResult {
    let [vault, mint] = accounts;
    require(
        stored == vault.address().as_ref() && &md[40..72] == mint.address().as_ref(),
        2,
    )?;
    let authority = Address::derive_address(
        &[b"bond", model.as_ref(), &slot.to_le_bytes()],
        Some(bump),
        program,
    );
    vault_owned_by(vault, mint.address(), &authority)
}
fn transfer(
    from: &AccountView,
    mint: &AccountView,
    to: &AccountView,
    authority: &AccountView,
    amount: u64,
) -> ProgramResult {
    TransferChecked::<&AccountView, TokenProgram>::new(from, mint, to, authority, amount, 6)
        .invoke()
}
fn user_transfer(
    accounts: [&AccountView; 4],
    lane: &Address,
    bump: u8,
    program: &Address,
    amount: u64,
) -> ProgramResult {
    let [from, mint, to, authority] = accounts;
    require(
        authority.address()
            == &Address::derive_address(&[b"vault", lane.as_ref()], Some(bump), program),
        2,
    )?;
    let bump_ref = [bump];
    let signer_seeds = seeds!(b"vault", lane.as_ref(), &bump_ref);
    TransferChecked::<&AccountView, TokenProgram>::new(from, mint, to, authority, amount, 6)
        .invoke_signed(&[Signer::from(&signer_seeds)])
}
fn bond_transfer(
    accounts: [&AccountView; 4],
    model: &Address,
    slot: u32,
    bump: u8,
    program: &Address,
    amount: u64,
) -> ProgramResult {
    let [from, mint, to, authority] = accounts;
    let slot_ref = slot.to_le_bytes();
    require(
        authority.address()
            == &Address::derive_address(&[b"bond", model.as_ref(), &slot_ref], Some(bump), program),
        2,
    )?;
    let bump_ref = [bump];
    let signer_seeds = seeds!(b"bond", model.as_ref(), &slot_ref, &bump_ref);
    TransferChecked::<&AccountView, TokenProgram>::new(from, mint, to, authority, amount, 6)
        .invoke_signed(&[Signer::from(&signer_seeds)])
}
fn rates(p: &[u8]) -> ProgramResult {
    require(
        rd32(p, 0) > 0
            && rd32(p, 8) > 0
            && rd32(p, 4) <= rd32(p, 0)
            && rd16(p, 12) > 0
            && p[14] <= 1
            && p[15] <= 1
            && rd32(p, 16) > 0,
        3,
    )
}

struct Request {
    caps: [u32; 3],
    min_speed: u16,
    private: bool,
    cache_provider: [u8; 32],
    max_input: u32,
    max_output: u32,
    estimated_input: u32,
    estimated_cached: u32,
    estimated_output: u32,
    spend: u64,
}
impl Request {
    fn parse(p: &[u8], free: u64) -> Result<Self, ProgramError> {
        require(p.len() == 107 && p[54] <= 1, 3)?;
        let r = Self {
            caps: [rd32(p, 32), rd32(p, 36), rd32(p, 40)],
            min_speed: rd16(p, 44),
            private: p[54] == 1,
            cache_provider: p[75..107].try_into().unwrap(),
            max_input: rd32(p, 46),
            max_output: rd32(p, 50),
            estimated_input: rd32(p, 63),
            estimated_cached: rd32(p, 67),
            estimated_output: rd32(p, 71),
            spend: free.min(rd64(p, 55)),
        };
        require(
            r.max_input > 0
                && r.max_output > 0
                && r.estimated_input <= r.max_input
                && r.estimated_output <= r.max_output
                && r.estimated_cached <= r.estimated_input
                && (r.estimated_cached == 0 || p[75..107] != [0; 32]),
            3,
        )?;
        Ok(r)
    }
}
#[derive(Debug, PartialEq)]
struct Match {
    slot: u32,
    reserve: u64,
    score: u64,
    exposure: u64,
    cache: bool,
}
fn select(terms: &[u8], capacity: &[u8], r: &Request) -> Result<Match, ProgramError> {
    let count = rd32(terms, 44) as usize;
    require(
        count <= MAX_ENTRIES && count == rd32(capacity, 44) as usize,
        2,
    )?;
    let terms = terms
        .get(HEADER_LEN..HEADER_LEN + count * TERMS_LEN)
        .ok_or(err(2))?;
    let capacity = capacity
        .get(HEADER_LEN..HEADER_LEN + count * CAPACITY_LEN)
        .ok_or(err(2))?;
    // ceil(output / speed) must fit the claim window; compute required speed once.
    let min_speed = u32::from(r.min_speed)
        .max(r.max_output.div_ceil((86_400 - CLAIM_SECONDS) as u32));
    let mut best = Match {
        slot: 0,
        reserve: 0,
        score: u64::MAX,
        exposure: 0,
        cache: false,
    };
    for (slot, (t, c)) in terms
        .chunks_exact(TERMS_LEN)
        .zip(capacity.chunks_exact(CAPACITY_LEN))
        .enumerate()
    {
        if t[111] != 1
            || t[112] != 1
            || (r.private && t[110] != 1)
            || u32::from(rd16(t, 108)) < min_speed
            || rd32(c, 16) >= rd32(t, 116)
            || rd32(c, 20) == u32::MAX
        {
            continue;
        }
        let input = rd32(t, 96);
        let cached = rd32(t, 100);
        let output = rd32(t, 104);
        if input > r.caps[0] || cached > r.caps[1] || output > r.caps[2] {
            continue;
        }
        let cached_tokens = if r.estimated_cached > 0 && t[..32] == r.cache_provider {
            r.estimated_cached
        } else {
            0
        };
        let score = fee(r.estimated_input - cached_tokens, input)
            + fee(cached_tokens, cached)
            + fee(r.estimated_output, output);
        // scores fit u64; ascending slots preserve lower-slot ties and permit safe score pruning.
        if score >= best.score {
            continue;
        }
        let reserve = fee(r.max_input, input) + fee(r.max_output, output) + 1;
        if reserve > r.spend {
            continue;
        }
        let Some(exposure) = rd64(c, 8).checked_add(reserve) else {
            continue;
        };
        if !collateral(exposure).is_ok_and(|needed| rd64(c, 0) >= needed) {
            continue;
        }
        best = Match {
            slot: slot as u32,
            reserve,
            score,
            exposure,
            cache: false,
        };
        if score == 0 {
            break;
        }
    }
    require(best.score != u64::MAX, 6)?;
    let at = best.slot as usize * TERMS_LEN;
    best.cache = terms[at..at + 32] == r.cache_provider;
    Ok(best)
}

pub fn process_instruction(
    program: &Address,
    accounts: &mut [AccountView],
    data: &[u8],
) -> ProgramResult {
    let (&op, p) = data.split_first().ok_or(err(3))?;
    #[cfg(feature = "receipt-prototype")]
    if op >= 23 {
        return receipt_prototype::process(program, accounts, op, p);
    }
    match op {
        0 => {
            let [m, admin, mint] = accounts else {
                return Err(err(3));
            };
            require(p.is_empty() && admin.is_signer(), 3)?;
            fresh(m, program, MARKET_LEN)?;
            let mt = Mint::from_account_view(mint)?;
            require(mt.is_initialized() && mt.decimals() == 6, 2)?;
            let mut d = m.try_borrow_mut()?;
            d[..8].copy_from_slice(MARKET_MAGIC);
            d[8..40].copy_from_slice(admin.address().as_ref());
            d[40..72].copy_from_slice(mint.address().as_ref());
            Ok(())
        }
        1 => {
            let [u, owner, m, vault] = accounts else {
                return Err(err(3));
            };
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
        2 => {
            require(accounts.len() == 8 || accounts.len() == 9, 3)?;
            let (accounts, extra) = accounts.split_at_mut(8);
            let [m, mo, t, c, profile, provider, payout, vault] = accounts else {
                return Err(err(3));
            };
            require(p.len() == 24, 3)?;
            rates(&p[4..])?;
            writable(&[t, c])?;
            let md = market(m, program)?;
            let modl = model(mo, program, m.address())?;
            approved_provider(profile, provider, m.address(), program)?;
            let td = book(t, program, mo, &modl, true)?;
            let cd = book(c, program, mo, &modl, false)?;
            let slot = rd32(p, 0);
            require(
                slot == rd32(&td, 44) && slot == rd32(&cd, 44) && slot < rd32(&td, 40),
                6,
            )?;
            if td[HEADER_LEN..HEADER_LEN + slot as usize * TERMS_LEN]
                .chunks_exact(TERMS_LEN)
                .any(|entry| &entry[..32] == provider.address().as_ref())
            {
                let admin = extra.first().unwrap_or(provider);
                require(admin.is_signer() && &md[8..40] == admin.address().as_ref(), 4)?;
            }
            token_owned_by(payout, &addr(&md, 40), provider.address())?;
            let (authority, bump) = Address::derive_program_address(
                &[b"bond", mo.address().as_ref(), &slot.to_le_bytes()],
                program,
            )
            .ok_or(err(2))?;
            vault_owned_by(vault, &addr(&md, 40), &authority)?;
            drop(td);
            drop(cd);
            let at = HEADER_LEN + slot as usize * TERMS_LEN;
            let mut d = t.try_borrow_mut()?;
            let entry = &mut d[at..at + TERMS_LEN];
            entry[..32].copy_from_slice(provider.address().as_ref());
            entry[32..64].copy_from_slice(payout.address().as_ref());
            entry[64..96].copy_from_slice(vault.address().as_ref());
            entry[96..112].copy_from_slice(&p[4..20]);
            entry[112] = 1;
            entry[113] = bump;
            wr32(entry, 116, rd32(p, 20));
            wr32(&mut d, 44, slot + 1);
            wr32(&mut c.try_borrow_mut()?, 44, slot + 1);
            Ok(())
        }
        3 => {
            let [m, u, owner, source, vault, mint, token] = accounts else {
                return Err(err(3));
            };
            require(p.len() == 8 && token.address() == &pinocchio_token::ID, 3)?;
            require(owner.is_signer(), 4)?;
            writable(&[u, source, vault])?;
            let md = market(m, program)?;
            let ud = user(u, program, owner.address(), m.address())?;
            user_vault(vault, mint, &md, u, &ud, program)?;
            token_owned_by(source, mint.address(), owner.address())?;
            let amount = rd64(p, 0);
            require(amount > 0, 3)?;
            let balance = add(rd64(&ud, 72), amount)?;
            drop(ud);
            transfer(source, mint, vault, owner, amount)?;
            wr64(&mut u.try_borrow_mut()?, 72, balance);
            Ok(())
        }
        4 => {
            let [m, mo, t, c, provider, source, vault, mint, token] = accounts else {
                return Err(err(3));
            };
            require(p.len() == 12 && token.address() == &pinocchio_token::ID, 3)?;
            writable(&[c, source, vault])?;
            let md = market(m, program)?;
            let modl = model(mo, program, m.address())?;
            let td = book(t, program, mo, &modl, true)?;
            let cd = book(c, program, mo, &modl, false)?;
            let slot = rd32(p, 0);
            let ti = slot_offset(&td, slot, true)?;
            let ci = slot_offset(&cd, slot, false)?;
            require(
                provider.is_signer() && &td[ti..ti + 32] == provider.address().as_ref(),
                4,
            )?;
            bond_vault(
                [vault, mint],
                &md,
                mo.address(),
                slot,
                &td[ti + 64..ti + 96],
                td[ti + 113],
                program,
            )?;
            token_owned_by(source, mint.address(), provider.address())?;
            let amount = rd64(p, 4);
            require(amount > 0, 3)?;
            let balance = add(rd64(&cd, ci), amount)?;
            drop(cd);
            transfer(source, mint, vault, provider, amount)?;
            wr64(&mut c.try_borrow_mut()?, ci, balance);
            Ok(())
        }
        5 | 16 => {
            let [m, mo, t, signer] = accounts else {
                return Err(err(3));
            };
            require(p.len() == if op == 5 { 24 } else { 5 }, 3)?;
            writable(&[t])?;
            let md = market(m, program)?;
            let modl = model(mo, program, m.address())?;
            let td = book(t, program, mo, &modl, true)?;
            let ti = slot_offset(&td, rd32(p, 0), true)?;
            require(signer.is_signer(), 4)?;
            if op == 5 {
                rates(&p[4..])?;
                require(&td[ti..ti + 32] == signer.address().as_ref(), 4)?;
            } else {
                require(p[4] <= 1, 3)?;
                require(&md[8..40] == signer.address().as_ref(), 4)?;
            }
            drop(td);
            let mut d = t.try_borrow_mut()?;
            if op == 5 {
                d[ti + 96..ti + 112].copy_from_slice(&p[4..20]);
                wr32(&mut d, ti + 116, rd32(p, 20));
            } else {
                d[ti + 112] = p[4];
            }
            Ok(())
        }
        6 => {
            let [m, mo, t, c, u, owner, b, rent_payer] = accounts else {
                return Err(err(3));
            };
            require(owner.is_signer(), 4)?;
            writable(&[c, u])?;
            #[cfg(not(feature = "receipt-prototype"))]
            { require(rent_payer.is_signer(), 4)?; fresh(b, program, BOOKING_LEN)?; }
            #[cfg(feature = "receipt-prototype")]
            let generation = if p.len() == 107 {
                require(rent_payer.is_signer(), 4)?;
                fresh(b, program, BOOKING_LEN)?;
                0
            } else { receipt_prototype::prepare_booking(
                b,
                program,
                owner.address(),
                m.address(),
                u.address(),
                rent_payer.address(),
                p,
            )? };
            #[cfg(feature = "receipt-prototype")]
            let p = &p[..107];
            market(m, program)?;
            let modl = model(mo, program, m.address())?;
            let ud = user(u, program, owner.address(), m.address())?;
            #[cfg(feature = "receipt-prototype")]
            {
                let mut counter = [0u8; 8];
                counter[..7].copy_from_slice(&ud[113..120]);
                require(generation == 0 || (
                    generation < (1u64 << 56) && generation == add(u64::from_le_bytes(counter), 1)?
                ), 7)?;
            }
            let r = Request::parse(p, rd64(&ud, 72))?;
            require(modl[40..72] == p[..32], 2)?;
            let td = book(t, program, mo, &modl, true)?;
            let cd = book(c, program, mo, &modl, false)?;
            let chosen = select(&td, &cd, &r)?;
            let ti = HEADER_LEN + chosen.slot as usize * TERMS_LEN;
            let ci = HEADER_LEN + chosen.slot as usize * CAPACITY_LEN;
            let active = inc(rd32(&cd, ci + 16))?;
            let obligations = inc(rd32(&cd, ci + 20))?;
            let free = sub(rd64(&ud, 72), chosen.reserve)?;
            // allow generation at the booked speed in addition to startup and settlement time.
            let claim_seconds = CLAIM_SECONDS
                + i64::from(r.max_output.div_ceil(u32::from(rd16(&td, ti + 108).max(1))));
            require(claim_seconds <= 86_400, 3)?;
            let claim_end = Clock::get()?
                .unix_timestamp
                .checked_add(claim_seconds)
                .ok_or(err(1))?;
            let dispute_end = claim_end.checked_add(DISPUTE_SECONDS).ok_or(err(1))?;
            let mut bd = b.try_borrow_mut()?;
            #[cfg(feature = "receipt-prototype")]
            {
                bd.fill(0);
                wr64(&mut bd, 152, generation);
            }
            bd[..8].copy_from_slice(BOOK_MAGIC);
            bd[8..40].copy_from_slice(owner.address().as_ref());
            bd[40..72].copy_from_slice(&td[ti..ti + 32]);
            bd[72..104].copy_from_slice(m.address().as_ref());
            wr64(&mut bd, 104, chosen.reserve);
            bd[112..124].copy_from_slice(&td[ti + 96..ti + 108]);
            wr32(&mut bd, 124, r.max_input);
            wr32(&mut bd, 128, r.max_output);
            wi64(&mut bd, 132, claim_end);
            wi64(&mut bd, 140, dispute_end);
            bd[180..182].copy_from_slice(&p[44..46]);
            bd[192..224].copy_from_slice(u.address().as_ref());
            bd[224..256].copy_from_slice(mo.address().as_ref());
            bd[256..288].copy_from_slice(rent_payer.address().as_ref());
            bd[288..320].copy_from_slice(&td[ti + 32..ti + 64]);
            bd[320..352].copy_from_slice(&td[ti + 64..ti + 96]);
            wr32(&mut bd, 352, chosen.slot);
            bd[356] = td[ti + 113];
            bd[357] = p[54];
            bd[358] = u8::from(chosen.cache);
            wr64(&mut bd, 360, chosen.score);
            drop(bd);
            drop(ud);
            drop(cd);
            wr64(&mut u.try_borrow_mut()?, 72, free);
            #[cfg(feature = "receipt-prototype")]
            {
                if generation > 0 { u.try_borrow_mut()?[113..120].copy_from_slice(&generation.to_le_bytes()[..7]); }
            }
            let mut d = c.try_borrow_mut()?;
            wr64(&mut d, ci + 8, chosen.exposure);
            wr32(&mut d, ci + 16, active);
            wr32(&mut d, ci + 20, obligations);
            Ok(())
        }
        7 => {
            #[cfg(feature = "receipt-prototype")]
            require(accounts.len() == 11 || accounts.len() == 12, 3)?;
            #[cfg(feature = "receipt-prototype")]
            let (accounts, receipt_accounts) = accounts.split_at_mut(11);
            let [m, mo, c, u, b, provider, vault, payout, mint, authority, token] = accounts else {
                return Err(err(3));
            };
            #[cfg(not(feature = "receipt-prototype"))]
            require(p.len() == 12, 3)?;
            #[cfg(feature = "receipt-prototype")]
            require((p.len() == 156 && receipt_accounts.len() == 1) || (p.len() == 12 && receipt_accounts.is_empty()), 3)?;
            require(token.address() == &pinocchio_token::ID, 3)?;
            require(provider.is_signer(), 4)?;
            writable(&[c, u, b, vault, payout])?;
            let md = market(m, program)?;
            let modl = model(mo, program, m.address())?;
            let cd = book(c, program, mo, &modl, false)?;
            let bd = booking(b, program, m.address(), mo.address())?;
            #[cfg(feature = "receipt-prototype")]
            let generation = rd64(&bd, 152);
            #[cfg(feature = "receipt-prototype")]
            require(if generation == 0 { p.len() == 12 } else { p.len() == 156 && generation == rd64(p, 12) }, 7)?;
            let ci = slot_offset(&cd, rd32(&bd, 352), false)?;
            require(
                bd[148] == 0
                    && &bd[40..72] == provider.address().as_ref()
                    && Clock::get()?.unix_timestamp <= ri64(&bd, 132),
                7,
            )?;
            require(&bd[288..320] == payout.address().as_ref(), 2)?;
            let ud = booked_user(u, program, &bd, m.address())?;
            user_vault(vault, mint, &md, u, &ud, program)?;
            token_owned_by(payout, mint.address(), provider.address())?;
            let uncached = rd32(p, 0);
            let cached = rd32(p, 4);
            let output = rd32(p, 8);
            require(
                u64::from(uncached) + u64::from(cached) <= u64::from(rd32(&bd, 124))
                    && output <= rd32(&bd, 128)
                    && (cached == 0 || bd[358] == 1),
                8,
            )?;
            let charge = fee(uncached, rd32(&bd, 112))
                + fee(cached, rd32(&bd, 116))
                + fee(output, rd32(&bd, 120));
            let reserve = rd64(&bd, 104);
            require(charge <= reserve, 8)?;
            let free = add(rd64(&ud, 72), reserve - charge)?;
            let exposure = add(sub(rd64(&cd, ci + 8), reserve)?, charge)?;
            let active = dec(rd32(&cd, ci + 16))?;
            let obligations = if charge == 0 {
                dec(rd32(&cd, ci + 20))?
            } else {
                rd32(&cd, ci + 20)
            };
            let bump = ud[112];
            drop(ud);
            drop(bd);
            drop(cd);
            if charge > 0 {
                user_transfer(
                    [vault, mint, payout, authority],
                    u.address(),
                    bump,
                    program,
                    charge,
                )?;
            }
            let mut d = b.try_borrow_mut()?;
            d[148] = if charge == 0 { 4 } else { 1 };
            wr64(&mut d, 160, charge);
            d[168..180].copy_from_slice(&p[..12]);
            drop(d);
            #[cfg(feature = "receipt-prototype")]
            if generation > 0 { receipt_prototype::settled(program, b, &mut receipt_accounts[0], charge, &p[20..])?; }
            wr64(&mut u.try_borrow_mut()?, 72, free);
            let mut d = c.try_borrow_mut()?;
            wr64(&mut d, ci + 8, exposure);
            wr32(&mut d, ci + 16, active);
            wr32(&mut d, ci + 20, obligations);
            Ok(())
        }
        8 | 15 => {
            #[cfg(feature = "receipt-prototype")]
            require(p.len() == 8 || p.is_empty(), 3)?;
            #[cfg(not(feature = "receipt-prototype"))]
            require(p.is_empty(), 3)?;
            require(accounts.len() == if op == 8 { 5 } else { 6 }, 3)?;
            let (core, extra) = accounts.split_at_mut(5);
            let [m, mo, c, u, b] = core else {
                return Err(err(3));
            };
            writable(&[c, u, b])?;
            market(m, program)?;
            let modl = model(mo, program, m.address())?;
            let cd = book(c, program, mo, &modl, false)?;
            let bd = booking(b, program, m.address(), mo.address())?;
            #[cfg(feature = "receipt-prototype")]
            let generation = rd64(&bd, 152);
            #[cfg(feature = "receipt-prototype")]
            require(if generation == 0 { p.is_empty() } else { p.len() == 8 && generation == rd64(p, 0) }, 7)?;
            let ci = slot_offset(&cd, rd32(&bd, 352), false)?;
            require(bd[148] == 0, 7)?;
            if op == 8 {
                require(Clock::get()?.unix_timestamp > ri64(&bd, 132), 7)?;
            } else {
                require(
                    extra[0].is_signer() && &bd[40..72] == extra[0].address().as_ref(),
                    7,
                )?;
            }
            let ud = booked_user(u, program, &bd, m.address())?;
            let reserve = rd64(&bd, 104);
            let free = add(rd64(&ud, 72), reserve)?;
            let exposure = sub(rd64(&cd, ci + 8), reserve)?;
            let active = dec(rd32(&cd, ci + 16))?;
            let obligations = dec(rd32(&cd, ci + 20))?;
            drop(cd);
            drop(ud);
            drop(bd);
            b.try_borrow_mut()?[148] = 3;
            #[cfg(feature = "receipt-prototype")]
            {
                if generation > 0 { b.try_borrow_mut()?[148] = receipt_prototype::AVAILABLE; }
            }
            wr64(&mut u.try_borrow_mut()?, 72, free);
            let mut d = c.try_borrow_mut()?;
            wr64(&mut d, ci + 8, exposure);
            wr32(&mut d, ci + 16, active);
            wr32(&mut d, ci + 20, obligations);
            Ok(())
        }
        9 => {
            let [m, mo, c, b] = accounts else {
                return Err(err(3));
            };
            require(p.is_empty(), 3)?;
            writable(&[c, b])?;
            market(m, program)?;
            let modl = model(mo, program, m.address())?;
            let cd = book(c, program, mo, &modl, false)?;
            let bd = booking(b, program, m.address(), mo.address())?;
            require(
                bd[148] == 1 && Clock::get()?.unix_timestamp > ri64(&bd, 140),
                7,
            )?;
            let ci = slot_offset(&cd, rd32(&bd, 352), false)?;
            let exposure = sub(rd64(&cd, ci + 8), rd64(&bd, 160))?;
            let obligations = dec(rd32(&cd, ci + 20))?;
            drop(cd);
            drop(bd);
            b.try_borrow_mut()?[148] = 4;
            let mut d = c.try_borrow_mut()?;
            wr64(&mut d, ci + 8, exposure);
            wr32(&mut d, ci + 20, obligations);
            Ok(())
        }
        10 => {
            let [m, u, owner, vault, destination, mint, authority, token] = accounts else {
                return Err(err(3));
            };
            require(p.len() == 8 && token.address() == &pinocchio_token::ID, 3)?;
            require(owner.is_signer(), 4)?;
            writable(&[u, vault, destination])?;
            let md = market(m, program)?;
            let ud = user(u, program, owner.address(), m.address())?;
            user_vault(vault, mint, &md, u, &ud, program)?;
            token_owned_by(destination, mint.address(), owner.address())?;
            let amount = rd64(p, 0);
            require(amount > 0, 3)?;
            let free = sub(rd64(&ud, 72), amount)?;
            let bump = ud[112];
            drop(ud);
            user_transfer(
                [vault, mint, destination, authority],
                u.address(),
                bump,
                program,
                amount,
            )?;
            wr64(&mut u.try_borrow_mut()?, 72, free);
            Ok(())
        }
        11 => {
            let [m, mo, b, owner] = accounts else {
                return Err(err(3));
            };
            require(p.is_empty(), 3)?;
            writable(&[b])?;
            market(m, program)?;
            model(mo, program, m.address())?;
            let bd = booking(b, program, m.address(), mo.address())?;
            require(
                owner.is_signer()
                    && bd[148] == 1
                    && &bd[8..40] == owner.address().as_ref()
                    && Clock::get()?.unix_timestamp <= ri64(&bd, 140),
                7,
            )?;
            drop(bd);
            b.try_borrow_mut()?[148] = 2;
            Ok(())
        }
        12 => {
            let [m, mo, c, u, b, admin, bond, vault, mint, authority, token] = accounts else {
                return Err(err(3));
            };
            require(p.len() == 8 && token.address() == &pinocchio_token::ID, 3)?;
            writable(&[c, u, b, bond, vault])?;
            let md = market(m, program)?;
            require(
                admin.is_signer() && &md[8..40] == admin.address().as_ref(),
                4,
            )?;
            let modl = model(mo, program, m.address())?;
            let cd = book(c, program, mo, &modl, false)?;
            let bd = booking(b, program, m.address(), mo.address())?;
            require(bd[148] == 2, 7)?;
            let slot = rd32(&bd, 352);
            let ci = slot_offset(&cd, slot, false)?;
            let ud = booked_user(u, program, &bd, m.address())?;
            bond_vault(
                [bond, mint],
                &md,
                mo.address(),
                slot,
                &bd[320..352],
                bd[356],
                program,
            )?;
            user_vault(vault, mint, &md, u, &ud, program)?;
            let refund = rd64(p, 0);
            let charge = rd64(&bd, 160);
            require(refund <= charge, 8)?;
            let free = add(rd64(&ud, 72), refund)?;
            let balance = sub(rd64(&cd, ci), refund)?;
            let exposure = sub(rd64(&cd, ci + 8), charge)?;
            require(balance >= collateral(exposure)?, 8)?;
            let obligations = dec(rd32(&cd, ci + 20))?;
            let bump = bd[356];
            drop(cd);
            drop(ud);
            drop(bd);
            if refund > 0 {
                bond_transfer(
                    [bond, mint, vault, authority],
                    mo.address(),
                    slot,
                    bump,
                    program,
                    refund,
                )?;
            }
            b.try_borrow_mut()?[148] = 4;
            wr64(&mut u.try_borrow_mut()?, 72, free);
            let mut d = c.try_borrow_mut()?;
            wr64(&mut d, ci, balance);
            wr64(&mut d, ci + 8, exposure);
            wr32(&mut d, ci + 20, obligations);
            Ok(())
        }
        13 => {
            let [m, mo, t, c, provider, request, rent_payer] = accounts else {
                return Err(err(3));
            };
            require(p.len() == 12, 3)?;
            require(rent_payer.is_signer(), 4)?;
            writable(&[c])?;
            fresh(request, program, UNBOND_LEN)?;
            market(m, program)?;
            let modl = model(mo, program, m.address())?;
            let td = book(t, program, mo, &modl, true)?;
            let cd = book(c, program, mo, &modl, false)?;
            let slot = rd32(p, 0);
            let ti = slot_offset(&td, slot, true)?;
            let ci = slot_offset(&cd, slot, false)?;
            require(
                provider.is_signer() && &td[ti..ti + 32] == provider.address().as_ref(),
                4,
            )?;
            let amount = rd64(p, 4);
            require(amount > 0, 3)?;
            let remaining = sub(rd64(&cd, ci), amount)?;
            require(remaining >= collateral(rd64(&cd, ci + 8))?, 8)?;
            let pending = add(rd64(&cd, ci + 24), amount)?;
            let ready = Clock::get()?
                .unix_timestamp
                .checked_add(UNBOND_SECONDS)
                .ok_or(err(1))?;
            drop(cd);
            let mut d = request.try_borrow_mut()?;
            d[..8].copy_from_slice(UNBOND_MAGIC);
            d[8..40].copy_from_slice(m.address().as_ref());
            d[40..72].copy_from_slice(provider.address().as_ref());
            wr64(&mut d, 72, amount);
            wi64(&mut d, 80, ready);
            wr32(&mut d, 92, slot);
            d[96..128].copy_from_slice(mo.address().as_ref());
            d[128..160].copy_from_slice(rent_payer.address().as_ref());
            drop(d);
            let mut d = c.try_borrow_mut()?;
            wr64(&mut d, ci, remaining);
            wr64(&mut d, ci + 24, pending);
            Ok(())
        }
        14 => {
            let [m, mo, t, c, request, provider, vault, payout, mint, authority, token] = accounts
            else {
                return Err(err(3));
            };
            require(
                p.is_empty() && token.address() == &pinocchio_token::ID && provider.is_signer(),
                3,
            )?;
            writable(&[c, request, vault, payout])?;
            let md = market(m, program)?;
            let modl = model(mo, program, m.address())?;
            let td = book(t, program, mo, &modl, true)?;
            let cd = book(c, program, mo, &modl, false)?;
            let r = state(request, program, UNBOND_LEN, UNBOND_MAGIC)?;
            require(
                &r[8..40] == m.address().as_ref()
                    && &r[40..72] == provider.address().as_ref()
                    && &r[96..128] == mo.address().as_ref()
                    && r[88] == 0
                    && Clock::get()?.unix_timestamp >= ri64(&r, 80),
                7,
            )?;
            let slot = rd32(&r, 92);
            let ti = slot_offset(&td, slot, true)?;
            let ci = slot_offset(&cd, slot, false)?;
            require(
                &td[ti..ti + 32] == provider.address().as_ref()
                    && &td[ti + 32..ti + 64] == payout.address().as_ref(),
                2,
            )?;
            bond_vault(
                [vault, mint],
                &md,
                mo.address(),
                slot,
                &td[ti + 64..ti + 96],
                td[ti + 113],
                program,
            )?;
            token_owned_by(payout, mint.address(), provider.address())?;
            let amount = rd64(&r, 72);
            let pending = sub(rd64(&cd, ci + 24), amount)?;
            let bump = td[ti + 113];
            drop(r);
            drop(cd);
            bond_transfer(
                [vault, mint, payout, authority],
                mo.address(),
                slot,
                bump,
                program,
                amount,
            )?;
            request.try_borrow_mut()?[88] = 1;
            wr64(&mut c.try_borrow_mut()?, ci + 24, pending);
            Ok(())
        }
        17 => {
            let [a, recipient] = accounts else {
                return Err(err(3));
            };
            require(p.is_empty() && a.address() != recipient.address(), 3)?;
            writable(&[a, recipient])?;
            require(a.owner() == program, 2)?;
            let d = a.try_borrow()?;
            if d.len() == BOOKING_LEN && &d[..8] == BOOK_MAGIC {
                #[cfg(feature = "receipt-prototype")]
                if d[359] == receipt_prototype::CASE {
                    require(Clock::get()?.unix_timestamp > ri64(&d, 140), 7)?;
                }
                let terminal = d[148] == 3 || d[148] == 4;
                #[cfg(feature = "receipt-prototype")]
                let terminal = {
                    if d[148] == receipt_prototype::AVAILABLE {
                        require(recipient.is_signer(), 4)?;
                    }
                    terminal || d[148] == receipt_prototype::AVAILABLE
                };
                require(terminal && &d[256..288] == recipient.address().as_ref(), 7)?;
            } else if d.len() == UNBOND_LEN && &d[..8] == UNBOND_MAGIC {
                require(
                    d[88] == 1 && &d[128..160] == recipient.address().as_ref(),
                    7,
                )?;
            } else {
                return Err(err(2));
            }
            drop(d);
            let lamports = add(recipient.lamports(), a.lamports())?;
            recipient.set_lamports(lamports);
            a.close()
        }
        18 => {
            require(accounts.len() == 9 || accounts.len() == 10, 3)?;
            let (accounts, extra) = accounts.split_at_mut(9);
            let [m, mo, t, c, provider, payer, system, profile, metadata] = accounts else {
                return Err(err(3));
            };
            require(
                p.len() >= 36 && system.address() == &pinocchio_system::ID,
                3,
            )?;
            metadata_payload(&p[36..])?;
            let md = market(m, program)?;
            let admin = extra.first().unwrap_or(provider);
            require(admin.is_signer() && &md[8..40] == admin.address().as_ref(), 4)?;
            approved_provider(profile, provider, m.address(), program)?;
            let limit = rd32(p, 32) as usize;
            require((1..=MAX_ENTRIES).contains(&limit), 3)?;
            require(t.address() != c.address(), 2)?;
            // zero headers are sufficient: this program never writes uninitialized book entries or closes books.
            for (account, len) in [
                (t as &AccountView, terms_book_len(limit)),
                (c as &AccountView, capacity_book_len(limit)),
            ] {
                require(
                    account.is_signer()
                        && account.is_writable()
                        && account.owner() == program
                        && account.data_len() == len
                        && account.try_borrow()?[..HEADER_LEN].iter().all(|x| *x == 0),
                    2,
                )?;
            }
            let (address, bump) = Address::derive_program_address(
                &[b"model", m.address().as_ref(), &p[..32]],
                program,
            )
            .ok_or(err(2))?;
            require(mo.address() == &address, 2)?;
            let bump_ref = [bump];
            let signer_seeds = seeds!(b"model", m.address().as_ref(), &p[..32], &bump_ref);
            initialize_pda(mo, payer, program, MODEL_LEN, Signer::from(&signer_seeds))?;
            let (address, meta_bump) = Address::derive_program_address(
                &[b"model_metadata", mo.address().as_ref()],
                program,
            )
            .ok_or(err(2))?;
            require(metadata.address() == &address, 2)?;
            let meta_bump_ref = [meta_bump];
            let meta_seeds = seeds!(b"model_metadata", mo.address().as_ref(), &meta_bump_ref);
            initialize_pda(
                metadata,
                payer,
                program,
                MODEL_METADATA_LEN,
                Signer::from(&meta_seeds),
            )?;
            let mut d = metadata.try_borrow_mut()?;
            d[..8].copy_from_slice(MODEL_METADATA_MAGIC);
            d[8..40].copy_from_slice(m.address().as_ref());
            d[40..72].copy_from_slice(mo.address().as_ref());
            d[72..104].copy_from_slice(provider.address().as_ref());
            d[138] = meta_bump;
            write_metadata(&mut d, 104, 140, &p[36..]);
            drop(d);
            let mut d = mo.try_borrow_mut()?;
            d[..8].copy_from_slice(MODEL_MAGIC);
            d[8..40].copy_from_slice(m.address().as_ref());
            d[40..72].copy_from_slice(&p[..32]);
            d[72..104].copy_from_slice(t.address().as_ref());
            d[104..136].copy_from_slice(c.address().as_ref());
            wr32(&mut d, 136, limit as u32);
            d[140] = bump;
            drop(d);
            for (account, magic) in [(t, TERMS_MAGIC), (c, CAPACITY_MAGIC)] {
                let mut d = account.try_borrow_mut()?;
                d[..8].copy_from_slice(magic);
                d[8..40].copy_from_slice(mo.address().as_ref());
                wr32(&mut d, 40, limit as u32);
            }
            Ok(())
        }
        19 => {
            let [m, profile, provider, payer, system] = accounts else {
                return Err(err(3));
            };
            metadata_payload(p)?;
            require(system.address() == &pinocchio_system::ID, 3)?;
            market(m, program)?;
            require(provider.is_signer(), 4)?;
            let (address, bump) = Address::derive_program_address(
                &[
                    b"provider",
                    m.address().as_ref(),
                    provider.address().as_ref(),
                ],
                program,
            )
            .ok_or(err(2))?;
            require(profile.address() == &address, 2)?;
            let bump_ref = [bump];
            let signer_seeds = seeds!(
                b"provider",
                m.address().as_ref(),
                provider.address().as_ref(),
                &bump_ref
            );
            initialize_pda(
                profile,
                payer,
                program,
                PROVIDER_LEN,
                Signer::from(&signer_seeds),
            )?;
            let mut d = profile.try_borrow_mut()?;
            d[..8].copy_from_slice(PROVIDER_MAGIC);
            d[8..40].copy_from_slice(m.address().as_ref());
            d[40..72].copy_from_slice(provider.address().as_ref());
            d[107] = bump;
            write_metadata(&mut d, 72, 108, p);
            Ok(())
        }
        20 | 21 => {
            let [m, pr, signer] = accounts else {
                return Err(err(3));
            };
            writable(&[pr])?;
            let md = market(m, program)?;
            let pd = profile(pr, program, m.address())?;
            require(signer.is_signer(), 4)?;
            if op == 20 {
                require(p.len() == 1 && p[0] <= 1, 3)?;
                require(&md[8..40] == signer.address().as_ref(), 4)?;
            } else {
                metadata_payload(p)?;
                require(&pd[40..72] == signer.address().as_ref(), 4)?;
            }
            drop(pd);
            let mut d = pr.try_borrow_mut()?;
            if op == 20 {
                d[106] = p[0];
            } else {
                write_metadata(&mut d, 72, 108, p);
            }
            Ok(())
        }
        22 => {
            let [m, mo, metadata, provider] = accounts else {
                return Err(err(3));
            };
            metadata_payload(p)?;
            writable(&[metadata])?;
            market(m, program)?;
            model(mo, program, m.address())?;
            let d = state(metadata, program, MODEL_METADATA_LEN, MODEL_METADATA_MAGIC)?;
            require(
                &d[8..40] == m.address().as_ref()
                    && &d[40..72] == mo.address().as_ref()
                    && metadata.address()
                        == &Address::derive_address(
                            &[b"model_metadata", mo.address().as_ref()],
                            Some(d[138]),
                            program,
                        ),
                2,
            )?;
            require(
                provider.is_signer() && &d[72..104] == provider.address().as_ref(),
                4,
            )?;
            drop(d);
            write_metadata(&mut metadata.try_borrow_mut()?, 104, 140, p);
            Ok(())
        }
        _ => Err(err(3)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::vec;

    fn payload() -> [u8; 107] {
        let mut p = [0; 107];
        for i in [32, 36, 40] {
            wr32(&mut p, i, u32::MAX);
        }
        wr32(&mut p, 46, 1_000_000);
        wr32(&mut p, 50, 1_000_000);
        wr64(&mut p, 55, u64::MAX);
        wr32(&mut p, 63, 1_000_000);
        wr32(&mut p, 71, 1_000_000);
        p
    }
    fn books(rates: &[[u32; 3]]) -> (std::vec::Vec<u8>, std::vec::Vec<u8>) {
        let mut t = vec![0; terms_book_len(rates.len())];
        let mut c = vec![0; capacity_book_len(rates.len())];
        wr32(&mut t, 44, rates.len() as u32);
        wr32(&mut c, 44, rates.len() as u32);
        for (i, rates) in rates.iter().enumerate() {
            let ti = HEADER_LEN + i * TERMS_LEN;
            let ci = HEADER_LEN + i * CAPACITY_LEN;
            t[ti..ti + 32].fill(i as u8 + 1);
            for (j, rate) in rates.iter().enumerate() {
                wr32(&mut t, ti + 96 + j * 4, *rate);
            }
            t[ti + 108] = 100;
            t[ti + 110..ti + 113].fill(1);
            wr32(&mut t, ti + 116, 2);
            wr64(&mut c, ci, u64::MAX);
        }
        (t, c)
    }
    #[test]
    fn split_rounding_and_maximum_arithmetic() {
        for input in 0..50u32 {
            for output in 0..10u32 {
                for rate in [1, 50_000, 999_999, 1_000_000, u32::MAX] {
                    let reserve = fee(input, rate) + fee(output, rate) + 1;
                    for cached in 0..=input {
                        for cached_rate in [0, rate / 2, rate] {
                            let charge = fee(input - cached, rate)
                                + fee(cached, cached_rate)
                                + fee(output, rate);
                            assert!(charge <= reserve);
                        }
                    }
                }
            }
        }
        assert_eq!(
            fee(u32::MAX, u32::MAX),
            (u128::from(u32::MAX).pow(2)).div_ceil(u128::from(UNIT)) as u64
        );
        assert_eq!(collateral(101).unwrap(), 103);
        assert!(collateral(u64::MAX).is_err());
    }
    #[test]
    fn multidimensional_score_cache_provenance_and_ties() {
        let (t, c) = books(&[[100, 1, 800], [300, 2, 50], [100, 1, 800]]);
        let mut p = payload();
        assert_eq!(
            select(&t, &c, &Request::parse(&p, u64::MAX).unwrap())
                .unwrap()
                .slot,
            1
        );
        wr32(&mut p, 71, 0);
        assert_eq!(
            select(&t, &c, &Request::parse(&p, u64::MAX).unwrap())
                .unwrap()
                .slot,
            0
        );
        wr32(&mut p, 67, 1_000_000);
        p[75..107].fill(2);
        let chosen = select(&t, &c, &Request::parse(&p, u64::MAX).unwrap()).unwrap();
        assert_eq!(
            (chosen.slot, chosen.score, chosen.reserve, chosen.cache),
            (1, 2, 351, true)
        );
        p[75..107].fill(9);
        let chosen = select(&t, &c, &Request::parse(&p, u64::MAX).unwrap()).unwrap();
        assert_eq!((chosen.slot, chosen.score, chosen.cache), (0, 100, false));
    }
    #[test]
    fn eligibility_excludes_every_unfunded_or_disallowed_candidate() {
        let (t, c) = books(&[[1, 1, 1], [2, 2, 2]]);
        let p = payload();
        let r = Request::parse(&p, u64::MAX).unwrap();
        for offset in [111, 112] {
            let mut changed = t.clone();
            changed[HEADER_LEN + offset] = 0;
            assert_eq!(select(&changed, &c, &r).unwrap().slot, 1);
        }
        let mut changed = c.clone();
        wr32(&mut changed, HEADER_LEN + 20, u32::MAX);
        assert_eq!(select(&t, &changed, &r).unwrap().slot, 1);
        wr32(&mut changed, HEADER_LEN + 20, 0);
        wr32(&mut changed, HEADER_LEN + 16, 2);
        assert_eq!(select(&t, &changed, &r).unwrap().slot, 1);
        wr32(&mut changed, HEADER_LEN + 16, 0);
        wr64(&mut changed, HEADER_LEN, 3);
        assert_eq!(select(&t, &changed, &r).unwrap().slot, 1);
        wr64(&mut changed, HEADER_LEN, 4);
        assert_eq!(select(&t, &changed, &r).unwrap().slot, 0);
        wr64(&mut changed, HEADER_LEN + 8, u64::MAX);
        assert_eq!(select(&t, &changed, &r).unwrap().slot, 1);
        let mut q = p;
        q[54] = 1;
        let mut changed = t.clone();
        changed[HEADER_LEN + 110] = 0;
        assert_eq!(
            select(&changed, &c, &Request::parse(&q, u64::MAX).unwrap())
                .unwrap()
                .slot,
            1
        );
        q[44] = 101;
        assert!(select(&t, &c, &Request::parse(&q, u64::MAX).unwrap()).is_err());
        q = p;
        wr32(&mut q, 32, 0);
        assert!(select(&t, &c, &Request::parse(&q, u64::MAX).unwrap()).is_err());
        assert!(select(&t, &c, &Request::parse(&p, 2).unwrap()).is_err());
        q = p;
        wr64(&mut q, 55, 2);
        assert!(select(&t, &c, &Request::parse(&q, u64::MAX).unwrap()).is_err());
    }
    #[test]
    fn request_bounds_and_book_count() {
        let mut p = payload();
        wr32(&mut p, 67, 1);
        assert!(Request::parse(&p, 100).is_err());
        p[75] = 1;
        assert!(Request::parse(&p, 100).is_ok());
        wr32(&mut p, 67, 1_000_001);
        assert!(Request::parse(&p, 100).is_err());
        wr32(&mut p, 67, 0);
        wr32(&mut p, 71, 1_000_001);
        assert!(Request::parse(&p, 100).is_err());
        p = payload();
        wr32(&mut p, 46, 0);
        assert!(Request::parse(&p, 100).is_err());
        let (t, mut c) = books(&[[1, 0, 1]]);
        wr32(&mut c, 44, 0);
        assert!(select(&t, &c, &Request::parse(&payload(), 100).unwrap()).is_err());
    }
    #[test]
    fn funding_survives_partial_refunds_and_pending_unbond() {
        for a in 0..150u64 {
            for b in 0..150u64 {
                let funded = collateral(a + b).unwrap();
                for refund in [0, a / 2, a] {
                    assert!(funded - refund >= collateral(b).unwrap());
                }
            }
        }
    }
}

#[cfg(test)]
mod matching_tests;
