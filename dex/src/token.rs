use crate::math::{check, fee, gross, sub, Result};
use anchor_lang::prelude::{AccountView, Address};

pub const TOKEN_2022: Address =
    Address::from_str_const("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");

pub fn read_u16(data: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(data[at..at + 2].try_into().unwrap())
}
pub fn read_u64(data: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(data[at..at + 8].try_into().unwrap())
}

#[derive(Clone, Copy, Default)]
pub struct Mint {
    pub decimals: u8,
    pub supply: u64,
    pub fee_bps: u16,
    pub fee_cap: u64,
}
impl Mint {
    pub fn net(self, amount: u64) -> Result<u64> {
        sub(amount, fee(amount, self.fee_bps, self.fee_cap)?)
    }
    pub fn gross(self, net: u64) -> Result<u64> {
        gross(net, self.fee_bps, self.fee_cap)
    }
}

pub fn mint(account: &AccountView, token: &AccountView, epoch: u64, quote: bool) -> Result<Mint> {
    check(token.executable() && account.owner() == token.address(), 2)?;
    check(
        token.address() == &anchor_spl::token::ID || token.address() == &TOKEN_2022,
        2,
    )?;
    parse_mint(
        &account.try_borrow()?,
        token.address() == &TOKEN_2022,
        epoch,
        quote,
    )
}

pub fn parse_mint(data: &[u8], token_2022: bool, epoch: u64, quote: bool) -> Result<Mint> {
    check(data.len() >= 82 && data[45] == 1, 2)?;
    // approved quote mints may retain issuer mint/freeze authorities.
    check(quote || (data[..4] == [0; 4] && data[46..50] == [0; 4]), 2)?;
    let mut result = Mint {
        decimals: data[44],
        supply: read_u64(data, 36),
        ..Mint::default()
    };
    if data.len() == 82 {
        return Ok(result);
    }
    check(
        token_2022 && data.len() >= 166 && data[82..165].iter().all(|b| *b == 0) && data[165] == 1,
        2,
    )?;
    let mut seen = 0u32;
    extensions(&data[166..], |kind, extension| {
        check(kind < 32 && seen & (1 << kind) == 0, 2)?;
        seen |= 1 << kind;
        match kind {
            1 => {
                check(extension.len() == 108, 2)?;
                let at = if epoch >= read_u64(extension, 90) {
                    90
                } else {
                    72
                };
                result.fee_cap = read_u64(extension, at + 8);
                result.fee_bps = read_u16(extension, at + 16);
                check(result.fee_bps <= 10_000, 2)
            }
            18 => check(extension.len() == 64, 2),
            19 => check(extension.len() >= 80, 2),
            _ => check(false, 2),
        }
    })?;
    Ok(result)
}

pub fn extensions(mut bytes: &[u8], mut visit: impl FnMut(u16, &[u8]) -> Result<()>) -> Result<()> {
    while !bytes.is_empty() {
        if bytes.iter().all(|b| *b == 0) {
            return Ok(());
        }
        check(bytes.len() >= 4, 2)?;
        let kind = read_u16(bytes, 0);
        let len = read_u16(bytes, 2) as usize;
        check(kind != 0 && bytes.len() >= 4 + len, 2)?;
        visit(kind, &bytes[4..4 + len])?;
        bytes = &bytes[4 + len..];
    }
    Ok(())
}

pub fn account(
    account: &AccountView,
    mint: &AccountView,
    owner: &Address,
    vault: bool,
) -> Result<u64> {
    check(account.owner() == mint.owner(), 2)?;
    let data = account.try_borrow()?;
    check(
        data.len() >= 165
            && data[..32] == *mint.address().as_ref()
            && data[32..64] == *owner.as_ref(),
        2,
    )?;
    check(data[108] == 1 && data[109..113] == [0; 4], 2)?;
    check(
        !vault || (data[72..76] == [0; 4] && data[129..133] == [0; 4]),
        2,
    )?;
    if data.len() > 165 {
        check(mint.owner() == &TOKEN_2022 && data[165] == 2, 2)?;
        let mut seen = 0u8;
        extensions(&data[166..], |kind, bytes| {
            let mask = match (kind, bytes.len()) {
                (2, 8) => 1,
                (7, 0) => 2,
                _ => return check(false, 2),
            };
            check(seen & mask == 0, 2)?;
            seen |= mask;
            Ok(())
        })?;
    }
    Ok(read_u64(&data, 64))
}

pub fn balance(account: &AccountView) -> Result<u64> {
    Ok(read_u64(&account.try_borrow()?, 64))
}

pub fn transfer(
    from: &AccountView,
    mint: &AccountView,
    to: &AccountView,
    owner: &AccountView,
    amount: u64,
    decimals: u8,
    signers: &[&[&[u8]]],
) -> Result<u64> {
    check(
        from.address() != to.address() && from.is_writable() && to.is_writable(),
        2,
    )?;
    let before_from = balance(from)?;
    let before_to = balance(to)?;
    crate::cross_program::transfer_checked(
        from,
        mint,
        to,
        owner,
        mint.owner(),
        amount,
        decimals,
        signers,
    )?;
    check(sub(before_from, balance(from)?)? == amount, 2)?;
    sub(balance(to)?, before_to)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mint_extension_allowlist_and_epoch_selection() {
        let mut d = [0u8; 278];
        d[45] = 1;
        d[165] = 1;
        d[166..168].copy_from_slice(&1u16.to_le_bytes());
        d[168..170].copy_from_slice(&108u16.to_le_bytes());
        d[170 + 80..170 + 88].copy_from_slice(&100u64.to_le_bytes());
        d[170 + 88..170 + 90].copy_from_slice(&100u16.to_le_bytes());
        d[170 + 90..170 + 98].copy_from_slice(&5u64.to_le_bytes());
        d[170 + 98..170 + 106].copy_from_slice(&500u64.to_le_bytes());
        d[170 + 106..].copy_from_slice(&200u16.to_le_bytes());
        assert_eq!(
            parse_mint(&d, true, 4, false).unwrap().net(1000).unwrap(),
            990
        );
        assert_eq!(
            parse_mint(&d, true, 5, false).unwrap().net(1000).unwrap(),
            980
        );
        d[166..168].copy_from_slice(&14u16.to_le_bytes());
        assert!(parse_mint(&d, true, 5, false).is_err());
        assert!(parse_mint(&d[..81], true, 5, false).is_err());
    }
}
