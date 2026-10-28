use crate::math::{check, fee, gross, sub, Result};
use anchor_lang::prelude::{AccountView, Address, InterfaceAccount};
use anchor_lang::{AnchorAccount, CpiContext, ToCpiHandle, ToCpiHandleMut};
use anchor_spl::{
    extensions::TransferFeeConfig,
    token_interface::{
        spl_token_2022::{
            extension::{BaseStateWithExtensions, ExtensionType, PodStateWithExtensions},
            pod::{PodAccount, PodMint},
        },
        Mint as SplMint, TokenAccount, TokenInterfaceAccountExtensions,
    },
};

#[derive(Clone, Copy, Default)]
pub struct Mint {
    pub decimals: u8,
    pub supply: u64,
    pub fee_bps: u16,
    pub fee_cap: u64,
    pub has_transfer_fee: bool,
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
    check(
        token.executable() && account.owner() == token.address(),
        crate::DexError::InvalidAccount,
    )?;
    check(
        token.address() == &anchor_spl::token::ID || token.address() == &anchor_spl::token_2022::ID,
        crate::DexError::InvalidAccount,
    )?;
    let mint = InterfaceAccount::<SplMint>::load(*account)?;
    check(
        quote || (!mint.has_mint_authority() && !mint.has_freeze_authority()),
        crate::DexError::InvalidAccount,
    )?;
    let mut result = Mint {
        decimals: mint.decimals(),
        supply: mint.supply(),
        ..Mint::default()
    };
    if account.owner() == &anchor_spl::token_2022::ID {
        let data = account.try_borrow()?;
        let state = PodStateWithExtensions::<PodMint>::unpack(&data)?;
        let extensions = state.get_extension_types()?;
        check(
            extensions.iter().all(|extension| {
                matches!(
                    extension,
                    ExtensionType::TransferFeeConfig
                        | ExtensionType::MetadataPointer
                        | ExtensionType::TokenMetadata
                )
            }),
            crate::DexError::InvalidAccount,
        )?;
        if extensions.contains(&ExtensionType::TransferFeeConfig) {
            result.has_transfer_fee = true;
            let config = mint.get_extension::<TransferFeeConfig>()?;
            let fee = if epoch >= config.newer_transfer_fee.epoch() {
                &config.newer_transfer_fee
            } else {
                &config.older_transfer_fee
            };
            result.fee_bps = fee.basis_points();
            result.fee_cap = fee.maximum_fee();
            check(result.fee_bps <= 10_000, crate::DexError::InvalidAccount)?;
        }
    }
    Ok(result)
}

pub fn curve_mint(
    account: &AccountView,
    authority: &Address,
    tax_bps: u16,
    taxed: bool,
) -> Result<()> {
    let mint = InterfaceAccount::<SplMint>::load(*account)?;
    check(
        mint.mint_authority() == Some(authority) && !mint.has_freeze_authority(),
        crate::DexError::InvalidAccount,
    )?;
    let data = account.try_borrow()?;
    let state = PodStateWithExtensions::<PodMint>::unpack(&data)?;
    let has_fee = state
        .get_extension_types()?
        .contains(&ExtensionType::TransferFeeConfig);
    check(has_fee == taxed, crate::DexError::InvalidAccount)?;
    if has_fee {
        let config = mint.get_extension::<TransferFeeConfig>()?;
        check(
            !anchor_spl::extensions::is_some_address(&config.transfer_fee_config_authority)
                && config.withdraw_withheld_authority == *authority
                && config.older_transfer_fee.maximum_fee() == u64::MAX
                && config.newer_transfer_fee.maximum_fee() == u64::MAX
                && config.older_transfer_fee.basis_points() == tax_bps
                && config.newer_transfer_fee.basis_points() == tax_bps,
            crate::DexError::InvalidAccount,
        )?;
    }
    Ok(())
}

pub fn account(
    account: &AccountView,
    mint: &AccountView,
    owner: &Address,
    vault: bool,
) -> Result<u64> {
    check(
        account.owner() == mint.owner(),
        crate::DexError::InvalidAccount,
    )?;
    let token = InterfaceAccount::<TokenAccount>::load(*account)?;
    check(
        token.mint() == mint.address() && token.owner() == owner,
        crate::DexError::InvalidAccount,
    )?;
    if vault {
        check(
            !token.has_delegate() && !token.has_close_authority() && !token.is_native(),
            crate::DexError::InvalidAccount,
        )?;
    }
    if account.owner() == &anchor_spl::token_2022::ID {
        let data = account.try_borrow()?;
        let state = PodStateWithExtensions::<PodAccount>::unpack(&data)?;
        check(
            state.get_extension_types()?.iter().all(|extension| {
                matches!(
                    extension,
                    ExtensionType::TransferFeeAmount | ExtensionType::ImmutableOwner
                )
            }),
            crate::DexError::InvalidAccount,
        )?;
    }
    Ok(token.amount())
}

pub fn balance(account: &AccountView) -> Result<u64> {
    Ok(InterfaceAccount::<TokenAccount>::load(*account)?.amount())
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
        crate::DexError::InvalidAccount,
    )?;
    let before_from = balance(from)?;
    let before_to = balance(to)?;
    let (mut from, mut to) = (*from, *to);
    anchor_spl::token_interface::transfer_checked(
        CpiContext::new_with_signer(
            mint.owner(),
            anchor_spl::token_interface::TransferChecked {
                from: from.try_to_cpi_handle_mut()?,
                mint: mint.to_cpi_handle(),
                to: to.try_to_cpi_handle_mut()?,
                authority: owner.to_cpi_handle(),
            },
            signers,
        ),
        amount,
        decimals,
    )?;
    check(
        sub(before_from, balance(&from)?)? == amount,
        crate::DexError::InvalidAccount,
    )?;
    sub(balance(&to)?, before_to)
}
