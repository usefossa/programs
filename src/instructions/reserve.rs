use crate::{
    args::{Redeem, ReserveActivation, ReserveConfig},
    helpers::require,
    state_types::Model,
    token::payment_token,
};
use anchor_lang::prelude::*;
use anchor_lang::{AnchorAccount, CpiContext, ToCpiHandle, ToCpiHandleMut};
use anchor_spl::{
    extensions::TransferFeeConfig,
    token_2022_extensions as fees,
    token_interface::{self as spl, Mint, TokenInterfaceAccountExtensions},
};
use spl::spl_token_2022::instruction::AuthorityType;

fn initialize_pda(
    account: &AccountView,
    payer: &AccountView,
    program: &Address,
    len: usize,
    signer: &[&[u8]],
) -> ProgramResult {
    anchor_lang::create_account_signed(payer, account, len, program, signer)
}

fn mint_state(mint: &InterfaceAccount<Mint>, model: &Account<Model>) -> ProgramResult {
    let config = mint.get_extension::<TransferFeeConfig>()?;
    require(
        mint.mint_authority().is_none()
            && mint.freeze_authority().is_none()
            && config.transfer_fee_config_authority == *model.address()
            && config.withdraw_withheld_authority == *model.address(),
        crate::RouterError::InvalidAccount,
    )
}

fn reserve_accounts(
    model: &Account<Model>,
    mint: &InterfaceAccount<Mint>,
    backing: &Account<anchor_spl::token::TokenAccount>,
    quote: &Account<anchor_spl::mint::Mint>,
) -> Result<(Address, u8)> {
    mint_state(mint, model)?;
    require(
        backing.mint() == quote.address()
            && backing.owner() == model.address()
            && !backing.has_delegate()
            && !backing.has_close_authority()
            && !backing.is_native(),
        crate::RouterError::InvalidAccount,
    )?;
    Ok((model.model_id, model.bump))
}

pub(crate) fn activate(
    ctx: &mut anchor_lang::Context<crate::contexts::ActivateReserve>,
    config: &ReserveActivation,
) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    require(
        config.supply > 0
            && config.tax_bps <= 10_000
            && config.curve.supply > 0
            && config.curve.supply <= config.supply,
        crate::RouterError::InvalidArgument,
    )?;
    require(
        accounts.model.route_to_reserve == 0
            && accounts.model.backing_vault == Address::default()
            && accounts.model.fixed_token_mint == Address::default(),
        crate::RouterError::InvalidState,
    )?;
    let model_id = accounts.model.model_id;
    let model_bump = [accounts.model.bump];
    let model_seeds: &[&[u8]] = &[
        b"model",
        accounts.market.address().as_ref(),
        model_id.as_ref(),
        &model_bump,
    ];
    let mint_bump = ctx.bumps.mint;
    let mint_bump = [mint_bump];
    let mint_seeds: &[&[u8]] = &[
        b"reserve_mint",
        accounts.model.address().as_ref(),
        &mint_bump,
    ];
    initialize_pda(
        accounts.mint.account(),
        accounts.payer.account(),
        &anchor_spl::token_2022::ID,
        278,
        mint_seeds,
    )?;
    fees::transfer_fee_initialize(
        CpiContext::new(
            &anchor_spl::token_2022::ID,
            fees::TransferFeeInitialize {
                mint: accounts.mint.try_to_cpi_handle_mut()?,
            },
        ),
        Some(accounts.model.address()),
        Some(accounts.model.address()),
        config.tax_bps,
        config.maximum_tax,
    )?;
    spl::initialize_mint2(
        CpiContext::new(
            &anchor_spl::token_2022::ID,
            spl::InitializeMint2 {
                mint: accounts.mint.try_to_cpi_handle_mut()?,
            },
        ),
        config.decimals,
        accounts.model.address(),
        None,
    )?;
    spl::initialize_account3(CpiContext::new(
        &anchor_spl::token::ID,
        spl::InitializeAccount3 {
            account: accounts.backing.try_to_cpi_handle_mut()?,
            mint: accounts.quote.to_cpi_handle(),
            authority: accounts.model.to_cpi_handle(),
        },
    ))?;
    spl::initialize_account3(CpiContext::new(
        &anchor_spl::token_2022::ID,
        spl::InitializeAccount3 {
            account: accounts.allocation.try_to_cpi_handle_mut()?,
            mint: accounts.mint.to_cpi_handle(),
            authority: accounts.curve_payer.to_cpi_handle(),
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
    spl::mint_to_checked(
        CpiContext::new_with_signer(
            &anchor_spl::token_2022::ID,
            spl::MintToChecked {
                mint: accounts.mint.try_to_cpi_handle_mut()?,
                to: accounts.allocation.try_to_cpi_handle_mut()?,
                authority: accounts.model.to_cpi_handle(),
            },
            &[model_seeds],
        ),
        config.supply,
        config.decimals,
    )?;
    spl::set_authority(
        CpiContext::new_with_signer(
            &anchor_spl::token_2022::ID,
            spl::SetAuthority {
                account_or_mint: accounts.mint.try_to_cpi_handle_mut()?,
                current_authority: accounts.model.to_cpi_handle(),
            },
            &[model_seeds],
        ),
        AuthorityType::MintTokens,
        None,
    )?;
    fossa_dex::cpi::initialize_model_curve(
        CpiContext::new(
            accounts.dex.address(),
            fossa_dex::cpi::accounts::InitializeModelCurve {
                launch: fossa_dex::__cpi_accounts_launch::Launch {
                    payer: accounts.curve_payer.try_to_cpi_handle_mut()?,
                    config: accounts.curve_config.to_cpi_handle(),
                    token_a: accounts.t22.to_cpi_handle(),
                    token_b: accounts.token.to_cpi_handle(),
                    token_program: accounts.token.to_cpi_handle(),
                    mint_a: accounts.mint.try_to_cpi_handle_mut()?,
                    mint_b: accounts.quote.to_cpi_handle(),
                    pool: accounts.curve_pool.try_to_cpi_handle_mut()?,
                    authority: accounts.curve_authority.to_cpi_handle(),
                    source_a: accounts.allocation.try_to_cpi_handle_mut()?,
                    source_b: accounts.curve_quote_source.try_to_cpi_handle_mut()?,
                    vault_a: accounts.curve_base_vault.try_to_cpi_handle_mut()?,
                    vault_b: accounts.curve_quote_vault.try_to_cpi_handle_mut()?,
                    lp_mint: accounts.curve_lp_mint.try_to_cpi_handle_mut()?,
                    user_lp: accounts.curve_lp.try_to_cpi_handle_mut()?,
                    system_program: accounts.system_program.to_cpi_handle(),
                },
                admin: accounts.admin.to_cpi_handle(),
                global: accounts.dex_global.to_cpi_handle(),
            },
        ),
        fossa_dex::args::ModelCurve {
            supply: config.curve.supply,
            virtual_a: config.curve.virtual_a,
            virtual_b: config.curve.virtual_b,
            target_b: config.curve.target_b,
            minimum_a: config.curve.minimum_a,
        },
    )?;
    accounts.model.route_to_reserve = 1;
    accounts.model.backing_vault = *accounts.backing.address();
    accounts.model.fixed_token_mint = *accounts.mint.address();
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
    mint_state(&accounts.mint, &accounts.model)?;
    require(
        !config.route_to_reserve || accounts.mint.supply() > 0,
        crate::RouterError::InvalidState,
    )?;
    let model_id = accounts.model.model_id;
    let bump = [accounts.model.bump];
    let signer: &[&[u8]] = &[
        b"model",
        accounts.market.address().as_ref(),
        model_id.as_ref(),
        &bump,
    ];
    fees::transfer_fee_set(
        CpiContext::new_with_signer(
            &anchor_spl::token_2022::ID,
            fees::TransferFeeSetTransferFee {
                mint: accounts.mint.try_to_cpi_handle_mut()?,
                authority: accounts.model.to_cpi_handle(),
            },
            &[signer],
        ),
        config.tax_bps,
        config.maximum_tax,
    )?;
    accounts.model.route_to_reserve = u8::from(config.route_to_reserve);
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
    let (model_id, bump) = reserve_accounts(
        &accounts.model,
        &accounts.mint,
        &accounts.backing,
        &accounts.quote,
    )?;
    payment_token(
        accounts.source.account(),
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
        b"model",
        accounts.market.address().as_ref(),
        model_id.as_ref(),
        &bump,
    ];
    anchor_spl::token::transfer_checked(
        CpiContext::new_with_signer(
            &anchor_spl::token::ID,
            anchor_spl::token::TransferChecked {
                from: accounts.backing.try_to_cpi_handle_mut()?,
                mint: accounts.quote.to_cpi_handle(),
                to: accounts.destination.try_to_cpi_handle_mut()?,
                authority: accounts.model.to_cpi_handle(),
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
    mint_state(&accounts.mint, &accounts.model)?;
    payment_token(
        accounts.destination.account(),
        accounts.mint.address(),
        &accounts.market.treasury2,
        &anchor_spl::token_2022::ID,
    )?;
    let model_id = accounts.model.model_id;
    let bump = [accounts.model.bump];
    let signer: &[&[u8]] = &[
        b"model",
        accounts.market.address().as_ref(),
        model_id.as_ref(),
        &bump,
    ];
    fees::withdraw_withheld_tokens_from_mint(CpiContext::new_with_signer(
        &anchor_spl::token_2022::ID,
        fees::WithdrawWithheldTokensFromMint {
            mint: accounts.mint.try_to_cpi_handle_mut()?,
            destination: accounts.destination.try_to_cpi_handle_mut()?,
            authority: accounts.model.to_cpi_handle(),
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
    let (model_id, bump) = reserve_accounts(
        &accounts.model,
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
        b"model",
        accounts.market.address().as_ref(),
        model_id.as_ref(),
        &bump,
    ];
    anchor_spl::token::transfer_checked(
        CpiContext::new_with_signer(
            &anchor_spl::token::ID,
            anchor_spl::token::TransferChecked {
                from: accounts.backing.try_to_cpi_handle_mut()?,
                mint: accounts.quote.to_cpi_handle(),
                to: accounts.destination.try_to_cpi_handle_mut()?,
                authority: accounts.model.to_cpi_handle(),
            },
            &[signer],
        ),
        amount,
        6,
    )
}
