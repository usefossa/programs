use crate::{
    args::{FeeConfig, ListFees},
    helpers::require,
    state_types::Market,
    token::{payment_mint, payment_token, payment_transfer},
};
use anchor_lang::prelude::{Account, AccountView, Address, ProgramResult};

const MIN_LIST_FEE_BPS: u16 = 10;
const MAX_LIST_FEE_BPS: u16 = 300;
const MIN_PROTOCOL_SHARE_BPS: u16 = 5000;

pub(crate) fn rotate_council(
    ctx: &mut anchor_lang::Context<crate::contexts::RotateCouncil>,
) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    accounts.market.council = *accounts.next.address();
    Ok(())
}

pub(crate) fn validate_terms(fee: u16, protocol_share: u16) -> ProgramResult {
    require(
        (MIN_LIST_FEE_BPS..=MAX_LIST_FEE_BPS).contains(&fee)
            && (MIN_PROTOCOL_SHARE_BPS..=10_000).contains(&protocol_share),
        crate::RouterError::InvalidArgument,
    )
}

pub(crate) fn configure(
    ctx: &mut anchor_lang::Context<crate::contexts::ConfigureFees>,
    config: &FeeConfig,
) -> ProgramResult {
    let mut remaining = ctx.remaining_accounts()?;
    let extra = remaining.as_mut_slice();
    let accounts = &mut ctx.accounts;
    require(
        config.bank != Address::default(),
        crate::RouterError::InvalidArgument,
    )?;
    require(
        extra.len() == if config.creation_enabled { 2 } else { 0 },
        crate::RouterError::InvalidArgument,
    )?;
    if config.creation_enabled {
        require(
            config.creation_net_price > 0
                && config.creation_mint == *extra[0].address()
                && config.creation_token_program == *extra[1].address(),
            crate::RouterError::InvalidArgument,
        )?;
        payment_mint(&extra[0], extra[1].address())?;
    }
    accounts.market.bank = config.bank;
    accounts.market.creation_mint = config.creation_mint;
    accounts.market.creation_token_program = config.creation_token_program;
    accounts.market.creation_net_price = config.creation_net_price.into();
    accounts.market.creation_enabled = u8::from(config.creation_enabled);
    Ok(())
}

pub(crate) fn configure_dex(
    ctx: &mut anchor_lang::Context<crate::contexts::ConfigureDex>,
) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    require(
        accounts.global.council == accounts.market.council
            && accounts.global.usdc_mint == accounts.market.mint
            && accounts.config.quote_mint == accounts.market.mint,
        crate::RouterError::InvalidAccount,
    )?;
    accounts.market.dex_program = *accounts.dex.address();
    accounts.market.dex_config = *accounts.config.address();
    Ok(())
}

pub(crate) fn configure_list(
    ctx: &mut anchor_lang::Context<crate::contexts::ConfigureListFees>,
    fees: ListFees,
) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    validate_terms(fees.fee_bps, fees.protocol_share)?;
    super::list::state(&accounts.list)?;
    accounts.list.fee_bps = fees.fee_bps.into();
    accounts.list.protocol_share = fees.protocol_share.into();
    Ok(())
}

pub(crate) fn creation_payment(
    market: &Account<Market>,
    authority: &AccountView,
    accounts: &[AccountView],
    maximum: u64,
) -> ProgramResult {
    require(
        market.creation_enabled == 1
            && market.creation_mint != Address::default()
            && market.creation_net_price.get() > 0,
        crate::RouterError::Unavailable,
    )?;
    let [source, destination, mint, token] = accounts else {
        return Err(crate::RouterError::InvalidArgument.into());
    };
    require(
        market.creation_mint == *mint.address()
            && market.creation_token_program == *token.address()
            && source.address() != destination.address(),
        crate::RouterError::InvalidAccount,
    )?;
    let (decimals, amount) =
        payment_mint(mint, token.address())?.gross(market.creation_net_price.get())?;
    require(amount <= maximum, crate::RouterError::LimitExceeded)?;
    let before = payment_token(
        destination,
        mint.address(),
        &market.council,
        token.address(),
    )?;
    payment_token(source, mint.address(), authority.address(), token.address())?;
    payment_transfer(
        source,
        mint,
        destination,
        authority,
        token.address(),
        amount,
        decimals,
    )?;
    let after = payment_token(
        destination,
        mint.address(),
        &market.council,
        token.address(),
    )?;
    require(
        after.checked_sub(before) == Some(market.creation_net_price.get()),
        crate::RouterError::LimitExceeded,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::helpers::{bps_ceil, fee_budget};

    #[test]
    fn fee_rounding_preserves_balance_and_protocol_floor() {
        for fee in 10..=300 {
            for base in [
                0,
                1,
                2,
                99,
                100,
                9999,
                10_000,
                1_000_001,
                u32::MAX as u64,
                u64::MAX,
            ] {
                let charged = bps_ceil(base, fee);
                assert_eq!(
                    u128::from(charged),
                    (u128::from(base) * u128::from(fee)).div_ceil(10_000)
                );
                for share in [5000, 5001, 7500, 9999, 10_000] {
                    let protocol = bps_ceil(charged, share);
                    assert!(protocol <= charged);
                    assert!(u128::from(protocol) * 2 >= u128::from(charged));
                    assert_eq!((charged - protocol) + protocol, charged);
                }
                let budget = fee_budget(base, fee);
                assert!(u128::from(budget) + u128::from(bps_ceil(budget, fee)) <= u128::from(base));
                assert!(
                    u128::from(budget + 1) + u128::from(bps_ceil(budget + 1, fee))
                        > u128::from(base)
                );
            }
        }
        assert!(validate_terms(10, 5000).is_ok());
        assert!(validate_terms(300, 10_000).is_ok());
        for terms in [(0, 5000), (9, 5000), (301, 5000), (100, 4999), (100, 10001)] {
            assert!(validate_terms(terms.0, terms.1).is_err());
        }
    }
}
