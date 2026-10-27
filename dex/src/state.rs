use crate::{
    abi::*,
    math::{add, check, Result},
    token::{self, read_u64, Mint},
};
use anchor_lang::prelude::Rent;
use anchor_lang::prelude::{AccountView, Address};
use anchor_lang::solana_program::sysvar::Sysvar as _;

pub fn write_u64(data: &mut [u8], at: usize, value: u64) {
    data[at..at + 8].copy_from_slice(&value.to_le_bytes());
}
pub fn address(data: &[u8], at: usize) -> Address {
    Address::new_from_array(data[at..at + 32].try_into().unwrap())
}
pub fn key(data: &mut [u8], at: usize, address: &Address) {
    data[at..at + 32].copy_from_slice(address.as_ref());
}
pub fn writable(accounts: &[&AccountView]) -> Result<()> {
    check(accounts.iter().all(|a| a.is_writable()), 2)
}
pub fn distinct(accounts: &[&AccountView]) -> Result<()> {
    for (i, a) in accounts.iter().enumerate() {
        check(
            accounts[i + 1..].iter().all(|b| a.address() != b.address()),
            2,
        )?;
    }
    Ok(())
}
pub fn fresh(account: &AccountView, program: &Address, len: usize) -> Result<()> {
    check(
        account.is_signer()
            && account.is_writable()
            && account.owner() == program
            && account.data_len() == len,
        2,
    )?;
    check(account.try_borrow()?.iter().all(|b| *b == 0), 2)?;
    check(
        account.lamports() >= Rent::get()?.try_minimum_balance(len)?,
        2,
    )
}
pub fn state<'a>(
    account: &'a AccountView,
    program: &Address,
    len: usize,
    magic: &[u8; 8],
) -> Result<impl core::ops::Deref<Target = [u8]> + 'a> {
    check(account.owner() == program && account.data_len() == len, 2)?;
    let data = account.try_borrow()?;
    check(&data[..8] == magic, 2)?;
    Ok(data)
}
pub fn authority(
    pool: &AccountView,
    authority: &AccountView,
    program: &Address,
    bump: u8,
) -> Result<()> {
    check(
        authority.address()
            == &Address::derive_address(
                &[b"authority", pool.address().as_ref()],
                Some(bump),
                program,
            ),
        2,
    )
}
pub fn lp(mint: &AccountView, authority: &Address) -> Result<u64> {
    check(
        mint.owner() == &anchor_spl::token::ID && mint.data_len() == 82,
        2,
    )?;
    let d = mint.try_borrow()?;
    check(
        d[..4] == [1, 0, 0, 0]
            && d[4..36] == *authority.as_ref()
            && d[44] == 9
            && d[45] == 1
            && d[46..50] == [0; 4],
        2,
    )?;
    Ok(read_u64(&d, 36))
}
pub fn rates(trade: u16, protocol: u16, creator: u16) -> Result<()> {
    check(
        trade < 10_000 && u32::from(protocol) + u32::from(creator) <= 10_000,
        3,
    )
}

pub struct Pool {
    pub a: u64,
    pub b: u64,
    pub protocol_a: u64,
    pub protocol_b: u64,
    pub creator_a: u64,
    pub creator_b: u64,
    pub virtual_a: u64,
    pub virtual_b: u64,
    pub target_b: u64,
    pub minimum_a: u64,
    pub locked_lp: u64,
    pub bump: u8,
    pub status: u8,
}
impl Pool {
    pub fn load(pool: &AccountView, program: &Address) -> Result<Self> {
        let d = state(pool, program, POOL_LEN, POOL_MAGIC)?;
        Ok(Self {
            a: read_u64(&d, RESERVE_A),
            b: read_u64(&d, RESERVE_B),
            protocol_a: read_u64(&d, PROTOCOL_A),
            protocol_b: read_u64(&d, PROTOCOL_B),
            creator_a: read_u64(&d, CREATOR_A),
            creator_b: read_u64(&d, CREATOR_B),
            virtual_a: read_u64(&d, VIRTUAL_A),
            virtual_b: read_u64(&d, VIRTUAL_B),
            target_b: read_u64(&d, TARGET_B),
            minimum_a: read_u64(&d, MINIMUM_A),
            locked_lp: read_u64(&d, LOCKED_LP),
            bump: d[BUMP],
            status: d[STATUS],
        })
    }
    pub fn save(&self, pool: &mut AccountView) -> Result<()> {
        let mut d = pool.try_borrow_mut()?;
        for (offset, value) in [
            (RESERVE_A, self.a),
            (RESERVE_B, self.b),
            (PROTOCOL_A, self.protocol_a),
            (PROTOCOL_B, self.protocol_b),
            (CREATOR_A, self.creator_a),
            (CREATOR_B, self.creator_b),
            (VIRTUAL_A, self.virtual_a),
            (VIRTUAL_B, self.virtual_b),
            (TARGET_B, self.target_b),
            (MINIMUM_A, self.minimum_a),
            (LOCKED_LP, self.locked_lp),
        ] {
            write_u64(&mut d, offset, value);
        }
        d[STATUS] = self.status;
        Ok(())
    }
    #[allow(
        clippy::too_many_arguments,
        reason = "account roles match the checked instruction boundary"
    )]
    pub fn validate(
        &self,
        pool: &AccountView,
        program: &Address,
        authority_account: &AccountView,
        mints: [&AccountView; 2],
        vaults: [&AccountView; 2],
        tokens: [&AccountView; 2],
        epoch: u64,
    ) -> Result<[Mint; 2]> {
        authority(pool, authority_account, program, self.bump)?;
        writable(&[pool, vaults[0], vaults[1]])?;
        distinct(&[
            pool,
            authority_account,
            mints[0],
            mints[1],
            vaults[0],
            vaults[1],
        ])?;
        let d = pool.try_borrow()?;
        check(
            address(&d, MINT_A) == *mints[0].address()
                && address(&d, MINT_B) == *mints[1].address()
                && address(&d, VAULT_A) == *vaults[0].address()
                && address(&d, VAULT_B) == *vaults[1].address(),
            2,
        )?;
        let a = token::account(vaults[0], mints[0], authority_account.address(), true)?;
        let b = token::account(vaults[1], mints[1], authority_account.address(), true)?;
        check(
            a >= add(
                add(self.a, add(self.protocol_a, self.creator_a)?)?,
                add(read_u64(&d, VESTED_BASE), read_u64(&d, LOCKED_BASE))?,
            )? && b >= add(self.b, add(self.protocol_b, self.creator_b)?)?,
            2,
        )?;
        Ok([
            token::mint(mints[0], tokens[0], epoch, false)?,
            token::mint(mints[1], tokens[1], epoch, true)?,
        ])
    }
}
