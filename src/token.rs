use crate::helpers::{add, require};
use anchor_lang::prelude::Clock;
use anchor_lang::prelude::{AccountView, Address, InterfaceAccount, ProgramResult};
#[allow(unused_imports)]
use anchor_lang::solana_program::sysvar::Sysvar as _;
use anchor_lang::{
    AnchorAccount, CpiContext, CpiHandle, CpiHandleMut, ToCpiHandle, ToCpiHandleMut,
};
use anchor_spl::{
    extensions::TransferFeeConfig,
    token_interface::{
        spl_token_2022::{
            extension::{BaseStateWithExtensions, ExtensionType, PodStateWithExtensions},
            pod::{PodAccount, PodMint},
        },
        Mint, TokenAccount, TokenInterfaceAccountExtensions,
    },
};

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
            u64::try_from(uncapped.min(capped)).map_err(|_| crate::RouterError::Arithmetic)?
        };
        Ok((self.decimals, gross))
    }
}

pub(crate) fn payment_mint(
    mint: &AccountView,
    token: &Address,
) -> Result<PaymentMint, anchor_lang::prelude::ProgramError> {
    require(
        (token == &anchor_spl::token::ID || token == &anchor_spl::token_2022::ID)
            && mint.owner() == token,
        crate::RouterError::InvalidAccount,
    )?;
    let mint = InterfaceAccount::<Mint>::load(*mint)?;
    let mut info = PaymentMint {
        decimals: mint.decimals(),
        bps: 0,
        cap: 0,
    };
    if token == &anchor_spl::token_2022::ID {
        let data = mint.account().try_borrow()?;
        let state = PodStateWithExtensions::<PodMint>::unpack(&data)?;
        let extensions = state.get_extension_types()?;
        require(
            extensions.iter().all(|extension| {
                matches!(
                    extension,
                    ExtensionType::TransferFeeConfig
                        | ExtensionType::MintCloseAuthority
                        | ExtensionType::MetadataPointer
                        | ExtensionType::TokenMetadata
                )
            }),
            crate::RouterError::InvalidAccount,
        )?;
        if extensions.contains(&ExtensionType::TransferFeeConfig) {
            let config = mint.get_extension::<TransferFeeConfig>()?;
            let epoch = Clock::get()?.epoch;
            let fee = if epoch >= config.newer_transfer_fee.epoch() {
                &config.newer_transfer_fee
            } else {
                &config.older_transfer_fee
            };
            info.bps = fee.basis_points();
            info.cap = fee.maximum_fee();
        }
    }
    Ok(info)
}

pub(crate) fn payment_token(
    account: &AccountView,
    mint: &Address,
    owner: &Address,
    token: &Address,
) -> Result<u64, anchor_lang::prelude::ProgramError> {
    payment_account(
        &InterfaceAccount::<TokenAccount>::load(*account)?,
        mint,
        owner,
        token,
    )
}

pub(crate) fn payment_account(
    account: &InterfaceAccount<TokenAccount>,
    mint: &Address,
    owner: &Address,
    token: &Address,
) -> Result<u64, anchor_lang::prelude::ProgramError> {
    require(
        (token == &anchor_spl::token::ID || token == &anchor_spl::token_2022::ID)
            && account.account().owner() == token,
        crate::RouterError::InvalidAccount,
    )?;
    require(
        account.mint() == mint && account.owner() == owner && !account.is_frozen(),
        crate::RouterError::InvalidAccount,
    )?;
    if token == &anchor_spl::token_2022::ID {
        // the typed account owns its borrow; this sdk read cannot outlive the shared reference.
        let data = unsafe { account.account().borrow_unchecked() };
        let state = PodStateWithExtensions::<PodAccount>::unpack(data)?;
        require(
            state.get_extension_types()?.iter().all(|extension| {
                matches!(
                    extension,
                    ExtensionType::TransferFeeAmount | ExtensionType::ImmutableOwner
                )
            }),
            crate::RouterError::InvalidAccount,
        )?;
    }
    Ok(account.amount())
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
    require(
        from.address() != to.address(),
        crate::RouterError::InvalidAccount,
    )?;
    let (mut from, mut to) = (*from, *to);
    anchor_spl::token_interface::transfer_checked(
        CpiContext::new(
            token,
            anchor_spl::token_interface::TransferChecked {
                from: from.try_to_cpi_handle_mut()?,
                mint: mint.to_cpi_handle(),
                to: to.try_to_cpi_handle_mut()?,
                authority: authority.to_cpi_handle(),
            },
        ),
        amount,
        decimals,
    )
}

pub(crate) fn token_owned_by(
    token: &AccountView,
    mint: &Address,
    owner: &Address,
) -> ProgramResult {
    let d = InterfaceAccount::<TokenAccount>::load(*token)?;
    require(
        d.is_initialized() && d.mint() == mint && d.owner() == owner,
        crate::RouterError::InvalidAccount,
    )
}
pub(crate) fn vault_owned_by(
    vault: &AccountView,
    mint: &Address,
    authority: &Address,
) -> ProgramResult {
    token_owned_by(vault, mint, authority)?;
    let d = InterfaceAccount::<TokenAccount>::load(*vault)?;
    require(
        !d.has_delegate() && !d.has_close_authority() && !d.is_native(),
        crate::RouterError::InvalidAccount,
    )
}
pub(crate) fn user_transfer<'a>(
    accounts: (
        CpiHandleMut<'a>,
        CpiHandle<'a>,
        CpiHandleMut<'a>,
        CpiHandle<'a>,
    ),
    lane: &Address,
    bump: u8,
    program: &Address,
    amount: u64,
) -> ProgramResult {
    let (from, mint, to, authority) = accounts;
    require(
        authority.address()
            == &Address::derive_address(&[b"vault", lane.as_ref()], Some(bump), program),
        crate::RouterError::InvalidAccount,
    )?;
    let bump_ref = [bump];
    let signer_seeds: &[&[u8]] = &[b"vault", lane.as_ref(), &bump_ref];
    anchor_spl::token::transfer_checked(
        CpiContext::new_with_signer(
            &anchor_spl::token::ID,
            anchor_spl::token::TransferChecked {
                from,
                mint,
                to,
                authority,
            },
            &[signer_seeds],
        ),
        amount,
        6,
    )
}
pub(crate) fn user_payments<'a>(
    accounts: (CpiHandleMut<'a>, CpiHandle<'a>, CpiHandle<'a>),
    mut payments: [(CpiHandleMut<'a>, u64); 3],
    lane: &Address,
    bump: u8,
    program: &Address,
) -> ProgramResult {
    let (from, mint, authority) = accounts;
    require(
        authority.address()
            == &Address::derive_address(&[b"vault", lane.as_ref()], Some(bump), program),
        crate::RouterError::InvalidAccount,
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
            anchor_spl::token::transfer_checked(
                CpiContext::new_with_signer(
                    &anchor_spl::token::ID,
                    anchor_spl::token::TransferChecked {
                        from,
                        mint,
                        to: payments[i].0,
                        authority,
                    },
                    &[signer_seeds],
                ),
                payments[i].1,
                6,
            )?;
        }
    }
    Ok(())
}
