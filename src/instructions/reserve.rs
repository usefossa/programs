use crate::{
    args::{GroupInit, Redeem, ReserveConfig, TokenMetadata},
    helpers::require,
    state_types::ModelGroup,
    token::payment_account,
};
use anchor_lang::prelude::*;
use anchor_lang::{AnchorAccount, CpiContext, ToCpiHandle, ToCpiHandleMut};
use anchor_spl::{
    extensions::TransferFeeConfig,
    token_2022_extensions as fees,
    token_interface::{self as spl, Mint, TokenInterfaceAccountExtensions},
};
use fossa_dex::{MODEL_CURVE_SUPPLY_ATOMS, MODEL_TOKEN_DECIMALS, MODEL_TREASURY_ATOMS};
use spl::spl_token_2022::{
    extension::{BaseStateWithExtensions, ExtensionType, PodStateWithExtensions},
    instruction::AuthorityType,
    pod::PodMint,
};
use spl_token_metadata_interface::state::TokenMetadata as MetadataState;

pub const REDEMPTION_FEE: u64 = 25_000_000;

fn fund_rent(payer: &mut Signer, account: &mut impl AnchorAccount, len: usize) -> ProgramResult {
    let amount = Rent::get()?
        .try_minimum_balance(len)?
        .saturating_sub(account.account().lamports());
    if amount == 0 {
        return Ok(());
    }
    anchor_lang::system_program::transfer(
        CpiContext::new(
            &anchor_lang::system_program::ID,
            anchor_lang::system_program::Transfer {
                from: payer.try_to_cpi_handle_mut()?,
                to: account.try_cpi_handle_mut()?,
            },
        ),
        amount,
    )
}

fn mint_state(mint: &InterfaceAccount<Mint>, group: &Account<ModelGroup>) -> ProgramResult {
    let config = mint.get_extension::<TransferFeeConfig>()?;
    require(
        mint.mint_authority().is_none()
            && mint.freeze_authority().is_none()
            && config.transfer_fee_config_authority == *group.address()
            && config.withdraw_withheld_authority == *group.address(),
        crate::RouterError::InvalidAccount,
    )
}

fn reserve_accounts(
    group: &Account<ModelGroup>,
    mint: &InterfaceAccount<Mint>,
    backing: &Account<anchor_spl::token::TokenAccount>,
    quote: &Account<anchor_spl::mint::Mint>,
) -> Result<(Address, u8)> {
    mint_state(mint, group)?;
    require(
        backing.mint() == quote.address()
            && backing.owner() == group.address()
            && !backing.has_delegate()
            && !backing.has_close_authority()
            && !backing.is_native(),
        crate::RouterError::InvalidAccount,
    )?;
    Ok((group.group_id, group.bump))
}

pub(crate) fn initialize_group(
    ctx: &mut anchor_lang::Context<crate::contexts::InitializeModelGroup>,
    group_id: Address,
    config: &GroupInit,
) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    config.metadata.validate()?;
    require(
        config.tax_bps <= 10_000,
        crate::RouterError::InvalidArgument,
    )?;
    let token_len =
        ExtensionType::try_calculate_account_len::<spl::spl_token_2022::state::Account>(&[
            ExtensionType::TransferFeeAmount,
        ])?;
    require(
        accounts.council_tokens.data_len() == token_len
            && accounts.curve_base_vault.data_len() == token_len,
        crate::RouterError::InvalidAccount,
    )?;
    let group_bump = [ctx.bumps.group];
    let group_seeds: &[&[u8]] = &[
        b"model_group",
        accounts.market.address().as_ref(),
        group_id.as_ref(),
        &group_bump,
    ];
    let mint_bump = [ctx.bumps.mint];
    let mint_seeds: &[&[u8]] = &[
        b"reserve_mint",
        accounts.group.address().as_ref(),
        &mint_bump,
    ];
    let mint_len =
        ExtensionType::try_calculate_account_len::<spl::spl_token_2022::state::Mint>(&[
            ExtensionType::TransferFeeConfig,
            ExtensionType::MetadataPointer,
        ])?;
    anchor_lang::create_account_signed(
        accounts.payer.account(),
        accounts.mint.account(),
        mint_len,
        &anchor_spl::token_2022::ID,
        mint_seeds,
    )?;
    fees::transfer_fee_initialize(
        CpiContext::new(
            &anchor_spl::token_2022::ID,
            fees::TransferFeeInitialize {
                mint: accounts.mint.try_to_cpi_handle_mut()?,
            },
        ),
        Some(accounts.group.address()),
        Some(accounts.group.address()),
        config.tax_bps,
        config.maximum_tax,
    )?;
    let mint_address = *accounts.mint.address();
    fees::metadata_pointer_initialize(
        CpiContext::new(
            &anchor_spl::token_2022::ID,
            fees::MetadataPointerInitialize {
                mint: accounts.mint.try_to_cpi_handle_mut()?,
            },
        ),
        None,
        Some(&mint_address),
    )?;
    spl::initialize_mint2(
        CpiContext::new(
            &anchor_spl::token_2022::ID,
            spl::InitializeMint2 {
                mint: accounts.mint.try_to_cpi_handle_mut()?,
            },
        ),
        MODEL_TOKEN_DECIMALS,
        accounts.group.address(),
        None,
    )?;
    let metadata = MetadataState {
        update_authority: Some(*accounts.group.address()).try_into()?,
        mint: mint_address,
        name: config.metadata.name.clone(),
        symbol: config.metadata.symbol.clone(),
        uri: config.metadata.uri.clone(),
        additional_metadata: alloc::vec::Vec::new(),
    };
    let metadata_len = {
        let data = accounts.mint.try_borrow()?;
        PodStateWithExtensions::<PodMint>::unpack(&data)?
            .try_get_new_account_len_for_variable_len_extension(&metadata)?
    };
    fund_rent(&mut accounts.payer, &mut accounts.mint, metadata_len)?;
    let mint = accounts.mint.try_to_cpi_handle_mut()?;
    fees::token_metadata_initialize(
        CpiContext::new_with_signer(
            &anchor_spl::token_2022::ID,
            fees::TokenMetadataInitialize {
                metadata: mint,
                update_authority: accounts.group.to_cpi_handle(),
                mint: mint.into_readonly(),
                mint_authority: accounts.group.to_cpi_handle(),
            },
            &[group_seeds],
        ),
        metadata.name,
        metadata.symbol,
        metadata.uri,
    )?;
    spl::initialize_account3(CpiContext::new(
        &anchor_spl::token_2022::ID,
        spl::InitializeAccount3 {
            account: accounts.council_tokens.try_to_cpi_handle_mut()?,
            mint: accounts.mint.to_cpi_handle(),
            authority: accounts.council.to_cpi_handle(),
        },
    ))?;
    spl::initialize_account3(CpiContext::new(
        &anchor_spl::token_2022::ID,
        spl::InitializeAccount3 {
            account: accounts.curve_base_vault.try_to_cpi_handle_mut()?,
            mint: accounts.mint.to_cpi_handle(),
            authority: accounts.curve_authority.to_cpi_handle(),
        },
    ))?;
    // mint both allocations directly so transfer tax cannot reduce their launch balances.
    spl::mint_to_checked(
        CpiContext::new_with_signer(
            &anchor_spl::token_2022::ID,
            spl::MintToChecked {
                mint: accounts.mint.try_to_cpi_handle_mut()?,
                to: accounts.council_tokens.try_to_cpi_handle_mut()?,
                authority: accounts.group.to_cpi_handle(),
            },
            &[group_seeds],
        ),
        MODEL_TREASURY_ATOMS,
        MODEL_TOKEN_DECIMALS,
    )?;
    spl::mint_to_checked(
        CpiContext::new_with_signer(
            &anchor_spl::token_2022::ID,
            spl::MintToChecked {
                mint: accounts.mint.try_to_cpi_handle_mut()?,
                to: accounts.curve_base_vault.try_to_cpi_handle_mut()?,
                authority: accounts.group.to_cpi_handle(),
            },
            &[group_seeds],
        ),
        MODEL_CURVE_SUPPLY_ATOMS,
        MODEL_TOKEN_DECIMALS,
    )?;
    spl::set_authority(
        CpiContext::new_with_signer(
            &anchor_spl::token_2022::ID,
            spl::SetAuthority {
                account_or_mint: accounts.mint.try_to_cpi_handle_mut()?,
                current_authority: accounts.group.to_cpi_handle(),
            },
            &[group_seeds],
        ),
        AuthorityType::MintTokens,
        None,
    )?;
    let council = accounts.council.try_to_cpi_handle_mut()?;
    fossa_dex::cpi::initialize_model_curve(CpiContext::new(
        accounts.dex.address(),
        fossa_dex::cpi::accounts::InitializeModelCurve {
            launch: fossa_dex::__cpi_accounts_launch::Launch {
                payer: council,
                global: accounts.dex_global.to_cpi_handle(),
                treasury: accounts.dex_treasury.try_to_cpi_handle_mut()?,
                config: accounts.curve_config.to_cpi_handle(),
                token_a: accounts.t22.to_cpi_handle(),
                token_b: accounts.token_program.to_cpi_handle(),
                token_program: accounts.token_program.to_cpi_handle(),
                mint_a: accounts.mint.try_to_cpi_handle_mut()?,
                mint_b: accounts.quote.to_cpi_handle(),
                pool: accounts.curve_pool.try_to_cpi_handle_mut()?,
                authority: accounts.curve_authority.to_cpi_handle(),
                source_a: accounts.council_tokens.try_to_cpi_handle_mut()?,
                source_b: accounts.curve_quote_source.try_to_cpi_handle_mut()?,
                vault_a: accounts.curve_base_vault.try_to_cpi_handle_mut()?,
                vault_b: accounts.curve_quote_vault.try_to_cpi_handle_mut()?,
                lp_mint: accounts.curve_lp_mint.try_to_cpi_handle_mut()?,
                user_lp: accounts.curve_lp.try_to_cpi_handle_mut()?,
                system_program: accounts.system_program.to_cpi_handle(),
            },
            council: council.into_readonly(),
        },
    ))?;
    accounts.group.market = *accounts.market.address();
    accounts.group.group_id = group_id;
    accounts.group.bump = ctx.bumps.group;
    accounts.group.route_to_reserve = 1;
    accounts.group.backing_vault = *accounts.backing.address();
    accounts.group.fixed_token_mint = *accounts.mint.address();
    Ok(())
}

pub(crate) fn assign(
    ctx: &mut anchor_lang::Context<crate::contexts::AssignModelGroup>,
) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    ctx.accounts.model.group = ctx
        .accounts
        .group
        .as_ref()
        .map(|group| *group.address())
        .unwrap_or_default();
    Ok(())
}

pub(crate) fn update_metadata(
    ctx: &mut anchor_lang::Context<crate::contexts::UpdateGroupMetadata>,
    metadata: &TokenMetadata,
) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    metadata.validate()?;
    let accounts = &mut ctx.accounts;
    mint_state(&accounts.mint, &accounts.group)?;
    let len = {
        // the typed mint owns its borrow; this shared read ends before mutable cpi handles.
        let data = unsafe { accounts.mint.account().borrow_unchecked() };
        let state = PodStateWithExtensions::<PodMint>::unpack(data)?;
        let mut value = state.get_variable_len_extension::<MetadataState>()?;
        value.name = metadata.name.clone();
        let mut len = state.try_get_new_account_len_for_variable_len_extension(&value)?;
        value.symbol = metadata.symbol.clone();
        len = len.max(state.try_get_new_account_len_for_variable_len_extension(&value)?);
        value.uri = metadata.uri.clone();
        len.max(state.try_get_new_account_len_for_variable_len_extension(&value)?)
    };
    fund_rent(&mut accounts.payer, &mut accounts.mint, len)?;
    let bump = [accounts.group.bump];
    let group_id = accounts.group.group_id;
    let seeds: &[&[u8]] = &[
        b"model_group",
        accounts.market.address().as_ref(),
        group_id.as_ref(),
        &bump,
    ];
    for (field, value) in [
        (fees::Field::Name, &metadata.name),
        (fees::Field::Symbol, &metadata.symbol),
        (fees::Field::Uri, &metadata.uri),
    ] {
        fees::token_metadata_update_field(
            CpiContext::new_with_signer(
                &anchor_spl::token_2022::ID,
                fees::TokenMetadataUpdateField {
                    metadata: accounts.mint.try_to_cpi_handle_mut()?,
                    update_authority: accounts.group.to_cpi_handle(),
                },
                &[seeds],
            ),
            field,
            value.clone(),
        )?;
    }
    Ok(())
}

pub(crate) fn configure(
    ctx: &mut anchor_lang::Context<crate::contexts::ConfigureReserve>,
    config: ReserveConfig,
) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    require(
        config.tax_bps <= 10_000,
        crate::RouterError::InvalidArgument,
    )?;
    mint_state(&accounts.mint, &accounts.group)?;
    require(
        !config.route_to_reserve || accounts.mint.supply() > 0,
        crate::RouterError::InvalidState,
    )?;
    let group_id = accounts.group.group_id;
    let bump = [accounts.group.bump];
    let signer: &[&[u8]] = &[
        b"model_group",
        accounts.market.address().as_ref(),
        group_id.as_ref(),
        &bump,
    ];
    fees::transfer_fee_set(
        CpiContext::new_with_signer(
            &anchor_spl::token_2022::ID,
            fees::TransferFeeSetTransferFee {
                mint: accounts.mint.try_to_cpi_handle_mut()?,
                authority: accounts.group.to_cpi_handle(),
            },
            &[signer],
        ),
        config.tax_bps,
        config.maximum_tax,
    )?;
    accounts.group.route_to_reserve = u8::from(config.route_to_reserve);
    Ok(())
}

pub(crate) fn redeem(
    ctx: &mut anchor_lang::Context<crate::contexts::RedeemReserve>,
    redeem: Redeem,
) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    require(redeem.amount > 0, crate::RouterError::InvalidArgument)?;
    let (group_id, bump) = reserve_accounts(
        &accounts.group,
        &accounts.mint,
        &accounts.backing,
        &accounts.quote,
    )?;
    payment_account(
        &accounts.source,
        accounts.mint.address(),
        accounts.holder.address(),
        &anchor_spl::token_2022::ID,
    )?;
    let supply = accounts.mint.supply();
    let decimals = accounts.mint.decimals();
    let amount = redeem.amount;
    require(
        amount <= supply && supply > 0,
        crate::RouterError::LimitExceeded,
    )?;
    let available = accounts.backing.amount();
    let out = (u128::from(available) * u128::from(amount) / u128::from(supply)) as u64;
    require(
        out > 0 && out >= redeem.minimum_output,
        crate::RouterError::LimitExceeded,
    )?;
    if accounts.holder.address() != accounts.council.address() {
        anchor_lang::system_program::transfer(
            CpiContext::new(
                &anchor_lang::system_program::ID,
                anchor_lang::system_program::Transfer {
                    from: accounts.holder.try_to_cpi_handle_mut()?,
                    to: accounts.council.try_to_cpi_handle_mut()?,
                },
            ),
            REDEMPTION_FEE,
        )?;
    }
    spl::burn_checked(
        CpiContext::new(
            &anchor_spl::token_2022::ID,
            spl::BurnChecked {
                from: accounts.source.try_to_cpi_handle_mut()?,
                mint: accounts.mint.try_to_cpi_handle_mut()?,
                authority: accounts.holder.to_cpi_handle(),
            },
        ),
        amount,
        decimals,
    )?;
    let bump = [bump];
    let signer: &[&[u8]] = &[
        b"model_group",
        accounts.market.address().as_ref(),
        group_id.as_ref(),
        &bump,
    ];
    anchor_spl::token::transfer_checked(
        CpiContext::new_with_signer(
            &anchor_spl::token::ID,
            anchor_spl::token::TransferChecked {
                from: accounts.backing.try_to_cpi_handle_mut()?,
                mint: accounts.quote.to_cpi_handle(),
                to: accounts.destination.try_to_cpi_handle_mut()?,
                authority: accounts.group.to_cpi_handle(),
            },
            &[signer],
        ),
        out,
        6,
    )
}

pub(crate) fn collect(
    ctx: &mut anchor_lang::Context<crate::contexts::CollectReserveTax>,
) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    mint_state(&accounts.mint, &accounts.group)?;
    payment_account(
        &accounts.destination,
        accounts.mint.address(),
        &accounts.market.bank,
        &anchor_spl::token_2022::ID,
    )?;
    let group_id = accounts.group.group_id;
    let bump = [accounts.group.bump];
    let signer: &[&[u8]] = &[
        b"model_group",
        accounts.market.address().as_ref(),
        group_id.as_ref(),
        &bump,
    ];
    fees::withdraw_withheld_tokens_from_mint(CpiContext::new_with_signer(
        &anchor_spl::token_2022::ID,
        fees::WithdrawWithheldTokensFromMint {
            mint: accounts.mint.try_to_cpi_handle_mut()?,
            destination: accounts.destination.try_to_cpi_handle_mut()?,
            authority: accounts.group.to_cpi_handle(),
        },
        &[signer],
    ))
}

pub(crate) fn sweep_empty(
    ctx: &mut anchor_lang::Context<crate::contexts::SweepReserve>,
) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    let (group_id, bump) = reserve_accounts(
        &accounts.group,
        &accounts.mint,
        &accounts.backing,
        &accounts.quote,
    )?;
    require(
        accounts.mint.supply() == 0,
        crate::RouterError::InvalidState,
    )?;
    let amount = accounts.backing.amount();
    if amount == 0 {
        return Ok(());
    }
    let bump = [bump];
    let signer: &[&[u8]] = &[
        b"model_group",
        accounts.market.address().as_ref(),
        group_id.as_ref(),
        &bump,
    ];
    anchor_spl::token::transfer_checked(
        CpiContext::new_with_signer(
            &anchor_spl::token::ID,
            anchor_spl::token::TransferChecked {
                from: accounts.backing.try_to_cpi_handle_mut()?,
                mint: accounts.quote.to_cpi_handle(),
                to: accounts.destination.try_to_cpi_handle_mut()?,
                authority: accounts.group.to_cpi_handle(),
            },
            &[signer],
        ),
        amount,
        6,
    )
}
