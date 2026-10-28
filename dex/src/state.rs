use crate::{
    math::{add, check, Result},
    state_types::Pool,
    token::{self, Mint},
};
use anchor_lang::prelude::{Account, AccountView, Address};
use anchor_lang::AnchorAccount;

pub fn lp(mint: &AccountView, authority: &Address) -> Result<u64> {
    let mint = Account::<anchor_spl::token::Mint>::load(*mint)?;
    check(
        mint.mint_authority() == Some(authority)
            && mint.decimals() == 9
            && mint.is_initialized()
            && !mint.has_freeze_authority(),
        crate::DexError::InvalidAccount,
    )?;
    Ok(mint.supply())
}
pub fn rates(trade: u16, protocol: u16, creator: u16) -> Result<()> {
    check(
        trade < 10_000 && u32::from(protocol) + u32::from(creator) <= 10_000,
        crate::DexError::InvalidArgument,
    )
}

impl Pool {
    #[allow(
        clippy::too_many_arguments,
        reason = "account roles match the checked instruction boundary"
    )]
    pub fn validate(
        &self,
        authority_account: &AccountView,
        mints: [&AccountView; 2],
        vaults: [&AccountView; 2],
        tokens: [&AccountView; 2],
        epoch: u64,
    ) -> Result<[Mint; 2]> {
        let balance_a = token::account(vaults[0], mints[0], authority_account.address(), true)?;
        let balance_b = token::account(vaults[1], mints[1], authority_account.address(), true)?;
        check(
            balance_a
                >= add(
                    add(
                        self.reserve_a.get(),
                        add(self.protocol_a.get(), self.creator_a.get())?,
                    )?,
                    add(self.vested_base.get(), self.locked_base.get())?,
                )?
                && balance_b
                    >= add(
                        self.reserve_b.get(),
                        add(self.protocol_b.get(), self.creator_b.get())?,
                    )?,
            crate::DexError::InvalidAccount,
        )?;
        Ok([
            token::mint(mints[0], tokens[0], epoch, false)?,
            token::mint(mints[1], tokens[1], epoch, true)?,
        ])
    }
}
