use crate::helpers::{add, err, rd16, rd64, require};
use anchor_lang::prelude::Clock;
use anchor_lang::prelude::{Account, AnchorAccount};
use anchor_lang::prelude::{AccountView, Address, ProgramResult};
#[allow(unused_imports)]
use anchor_lang::solana_program::sysvar::Sysvar as _;
use anchor_spl::token::TokenAccount;

pub(crate) const TOKEN_2022: Address =
    Address::from_str_const("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
pub(crate) struct PaymentMint {
    pub decimals: u8,
    pub bps: u16,
    pub cap: u64,
}
impl PaymentMint {
    pub(crate) fn gross(&self, net: u64) -> Result<(u8, u64), anchor_lang::prelude::ProgramError> {
        let gross = if self.bps == 0 || net == 0 {
            net
        } else {
            let uncapped = if self.bps == 10_000 {
                u128::MAX
            } else {
                (u128::from(net) * 10_000).div_ceil(u128::from(10_000 - self.bps))
            };
            let capped = u128::from(net) + u128::from(self.cap);
            u64::try_from(uncapped.min(capped)).map_err(|_| err(1))?
        };
        Ok((self.decimals, gross))
    }
}

fn extensions(
    data: &[u8],
    mint: bool,
    mut visit: impl FnMut(u16, &[u8]) -> ProgramResult,
) -> ProgramResult {
    require(
        data.len() >= 166 && data[165] == if mint { 1 } else { 2 },
        2,
    )?;
    if mint {
        require(data[82..165].iter().all(|b| *b == 0), 2)?;
    }
    let mut at = 166;
    let mut seen = 0u64;
    while at < data.len() {
        if data[at..].iter().all(|b| *b == 0) {
            break;
        }
        require(data.len() - at >= 4, 2)?;
        let kind = rd16(data, at);
        let len = usize::from(rd16(data, at + 2));
        require(
            kind > 0 && kind < 64 && seen & (1 << kind) == 0 && len <= data.len() - at - 4,
            2,
        )?;
        seen |= 1 << kind;
        visit(kind, &data[at + 4..at + 4 + len])?;
        at += 4 + len;
    }
    Ok(())
}

pub(crate) fn payment_mint(
    mint: &AccountView,
    token: &Address,
) -> Result<PaymentMint, anchor_lang::prelude::ProgramError> {
    require(
        (token == &anchor_spl::token::ID || token == &TOKEN_2022) && mint.owner() == token,
        2,
    )?;
    let d = mint.try_borrow()?;
    require(d.len() >= 82 && d[45] == 1, 2)?;
    let mut info = PaymentMint {
        decimals: d[44],
        bps: 0,
        cap: 0,
    };
    if token == &anchor_spl::token::ID || d.len() == 82 {
        require(d.len() == 82, 2)?;
    } else {
        extensions(&d, true, |kind, ext| match kind {
            1 => {
                require(
                    ext.len() == 108 && rd16(ext, 88) <= 10_000 && rd16(ext, 106) <= 10_000,
                    2,
                )?;
                let at = if Clock::get()?.epoch >= rd64(ext, 90) {
                    90
                } else {
                    72
                };
                info.bps = rd16(ext, at + 16);
                info.cap = rd64(ext, at + 8);
                Ok(())
            }
            3 => require(ext.len() == 32, 2),
            18 => require(ext.len() == 64, 2),
            19 => Ok(()),
            _ => Err(err(2)),
        })?;
    }
    Ok(info)
}

pub(crate) fn payment_token(
    account: &AccountView,
    mint: &Address,
    owner: &Address,
    token: &Address,
) -> ProgramResult {
    require(
        (token == &anchor_spl::token::ID || token == &TOKEN_2022) && account.owner() == token,
        2,
    )?;
    let d = account.try_borrow()?;
    require(
        d.len() >= 165 && &d[..32] == mint.as_ref() && &d[32..64] == owner.as_ref() && d[108] == 1,
        2,
    )?;
    if token == &anchor_spl::token::ID || d.len() == 165 {
        require(d.len() == 165, 2)
    } else {
        extensions(&d, false, |kind, ext| match kind {
            2 => require(ext.len() == 8, 2),
            7 => require(ext.is_empty(), 2),
            _ => Err(err(2)),
        })
    }
}

pub(crate) fn payment_transfer(
    from: &AccountView,
    mint: &AccountView,
    to: &AccountView,
    authority: &AccountView,
    token: &Address,
    amount: u64,
    decimals: u8,
) -> ProgramResult {
    require(from.address() != to.address(), 2)?;
    crate::cross_program::transfer_checked(from, mint, to, authority, token, amount, decimals, &[])
}

pub(crate) fn token_owned_by(
    token: &AccountView,
    mint: &Address,
    owner: &Address,
) -> ProgramResult {
    let d = Account::<TokenAccount>::load(*token)?;
    require(
        d.is_initialized() && d.mint() == mint && d.owner() == owner,
        2,
    )
}
pub(crate) fn vault_owned_by(
    vault: &AccountView,
    mint: &Address,
    authority: &Address,
) -> ProgramResult {
    token_owned_by(vault, mint, authority)?;
    let d = Account::<TokenAccount>::load(*vault)?;
    require(
        !d.has_delegate() && !d.has_close_authority() && !d.is_native(),
        2,
    )
}
pub(crate) fn user_vault(
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
#[cfg(feature = "legacy-migration")]
pub(crate) fn bond_vault(
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
pub(crate) fn user_transfer(
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
    let signer_seeds: &[&[u8]] = &[b"vault", lane.as_ref(), &bump_ref];
    crate::cross_program::transfer_checked(
        from,
        mint,
        to,
        authority,
        &anchor_spl::token::ID,
        amount,
        6,
        &[signer_seeds],
    )
}
pub(crate) fn user_payments(
    accounts: [&AccountView; 3],
    mut payments: [(&AccountView, u64); 3],
    lane: &Address,
    bump: u8,
    program: &Address,
) -> ProgramResult {
    let [from, mint, authority] = accounts;
    require(
        authority.address()
            == &Address::derive_address(&[b"vault", lane.as_ref()], Some(bump), program),
        2,
    )?;
    let bump_ref = [bump];
    let signer_seeds: &[&[u8]] = &[b"vault", lane.as_ref(), &bump_ref];
    for i in 0..payments.len() {
        for j in i + 1..payments.len() {
            if payments[i].0.address() == payments[j].0.address() {
                payments[i].1 = add(payments[i].1, payments[j].1)?;
                payments[j].1 = 0;
            }
        }
        if payments[i].1 > 0 {
            crate::cross_program::transfer_checked(
                from,
                mint,
                payments[i].0,
                authority,
                &anchor_spl::token::ID,
                payments[i].1,
                6,
                &[signer_seeds],
            )?;
        }
    }
    Ok(())
}
#[cfg(feature = "legacy-migration")]
pub(crate) fn bond_transfer(
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
    let signer_seeds: &[&[u8]] = &[b"bond", model.as_ref(), &slot_ref, &bump_ref];
    crate::cross_program::transfer_checked(
        from,
        mint,
        to,
        authority,
        &anchor_spl::token::ID,
        amount,
        6,
        &[signer_seeds],
    )
}
