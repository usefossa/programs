use crate::{
    args::ListInit,
    helpers::{add, require},
    state_types::{List, MAX_ENTRIES},
};
use anchor_lang::accounts::Slab;
use anchor_lang::prelude::{Address, ProgramError, ProgramResult};
use anchor_lang::AnchorAccount;

pub(crate) fn position(providers: &[Address], provider: &Address) -> Result<usize, usize> {
    providers.binary_search(provider)
}

pub(crate) fn state<'a>(
    list: &'a Slab<List, Address>,
) -> Result<&'a Slab<List, Address>, ProgramError> {
    let limit = list.limit.get() as usize;
    require(
        (1..=MAX_ENTRIES).contains(&limit)
            && list.account().data_len() == Slab::<List, Address>::space_for(limit as u32)
            && list.len() <= limit
            && list.as_slice().windows(2).all(|pair| pair[0] < pair[1]),
        crate::RouterError::InvalidAccount,
    )?;
    Ok(list)
}

pub(crate) fn initialize(
    ctx: &mut anchor_lang::Context<crate::contexts::InitializeList>,
    config: &ListInit,
) -> ProgramResult {
    let mut remaining = ctx.remaining_accounts()?;
    let payment = remaining.as_mut_slice();
    let accounts = &mut ctx.accounts;
    let authority = accounts.authority.account();
    let fees = config.fees;
    super::fees::validate_terms(fees.fee_bps, fees.protocol_share)?;
    super::fees::creation_payment(&accounts.market, authority, payment, config.maximum_payment)?;
    let limit = config.limit as usize;
    require(
        (1..=MAX_ENTRIES).contains(&limit)
            && accounts.list.account().data_len() == Slab::<List, Address>::space_for(limit as u32),
        crate::RouterError::InvalidArgument,
    )?;
    accounts.list.market = *accounts.market.address();
    accounts.list.authority = *authority.address();
    accounts.list.limit = (limit as u32).into();
    accounts.list.fee_bps = fees.fee_bps.into();
    accounts.list.protocol_share = fees.protocol_share.into();
    Ok(())
}

pub(crate) fn update(
    ctx: &mut anchor_lang::Context<crate::contexts::UpdateList>,
    provider: Address,
    enabled: bool,
) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    require(
        provider != Address::default(),
        crate::RouterError::InvalidArgument,
    )?;
    state(&accounts.list)?;
    let count = accounts.list.len();
    let found = position(accounts.list.as_slice(), &provider);
    if enabled {
        require(
            count < accounts.list.limit.get() as usize,
            crate::RouterError::Unavailable,
        )?;
        let at = found.err().ok_or(crate::RouterError::InvalidState)?;
        accounts.list.try_push(provider)?;
        accounts.list.as_mut_slice()[at..=count].rotate_right(1);
    } else {
        let at = found.map_err(|_| crate::RouterError::InvalidState)?;
        accounts.list.as_mut_slice()[at..count].rotate_left(1);
        accounts.list.pop();
    }
    Ok(())
}

pub(crate) fn close(ctx: &mut anchor_lang::Context<crate::contexts::CloseList>) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    state(&accounts.list)?;
    require(accounts.list.is_empty(), crate::RouterError::InvalidState)?;
    let bump = ctx.bumps.metadata;
    let has_metadata = accounts.metadata.owner() != &anchor_lang::system_program::ID
        || accounts.metadata.data_len() != 0;
    if has_metadata {
        super::list_metadata::validate(
            accounts.metadata.account(),
            &accounts.list,
            ctx.program_id,
            bump,
        )?;
    }
    let refund = add(
        accounts.list.account().lamports(),
        if has_metadata {
            accounts.metadata.lamports()
        } else {
            0
        },
    )?;
    let mut authority = *accounts.authority.account();
    authority.set_lamports(add(authority.lamports(), refund)?);
    if has_metadata {
        let mut metadata = *accounts.metadata.account();
        metadata.close()?;
    }
    let mut list = *accounts.list.account();
    list.close()
}
