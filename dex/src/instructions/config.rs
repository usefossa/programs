use super::*;

pub(crate) fn initialize_config(
    ctx: &mut Context<InitializeConfig>,
    input: ConfigInit,
) -> Result<()> {
    check(
        ctx.remaining_accounts()?.is_empty(),
        crate::DexError::InvalidArgument,
    )?;
    let council = &ctx.accounts.council;
    let config = &mut ctx.accounts.config;
    let mut mint = *ctx.accounts.mint.account();
    let mint = &mut mint;
    let token = ctx.accounts.token.account();
    let global = &ctx.accounts.global;

    token::mint(&ctx.accounts.mint, token, Clock::get()?.epoch, true)?;
    rates(
        input.rates.trade_bps,
        input.rates.protocol_share,
        input.rates.creator_share,
    )?;
    check(
        input.rates.trade_bps == 125
            && input.rates.protocol_share == 10_000
            && input.rates.creator_share == 0,
        crate::DexError::InvalidArgument,
    )?;
    check(
        input.treasury != Address::default(),
        crate::DexError::InvalidArgument,
    )?;
    check(
        input.treasury == global.treasury,
        crate::DexError::InvalidArgument,
    )?;
    let distributor = input.distributor.unwrap_or(input.treasury);
    check(
        distributor == global.distributor,
        crate::DexError::InvalidArgument,
    )?;
    config.council = *council.address();
    config.treasury = input.treasury;
    config.quote_mint = *mint.address();
    config.trade_bps = input.rates.trade_bps.into();
    config.protocol_share = input.rates.protocol_share.into();
    config.creator_share = input.rates.creator_share.into();
    config.distributor = distributor;
    config.enabled = 1;
    if mint.address() == &global.usdc_mint {
        check(
            ctx.accounts.reference_pool.is_none() && ctx.accounts.mint.decimals() == 6,
            crate::DexError::InvalidAccount,
        )?;
    } else if let Some(reference) = ctx.accounts.reference_pool.as_ref() {
        check(
            reference.mode == MODEL
                && reference.status == AMM
                && reference.locked_lp.get() > 0
                && reference.mint_a == *mint.address()
                && reference.mint_b == global.usdc_mint,
            crate::DexError::InvalidAccount,
        )?;
        config.reference_pool = *reference.address();
        oracle::checkpoint(config, reference, oracle::timestamp(&Clock::get()?)?)?;
    }
    Ok(())
}

pub(crate) fn initialize_global(
    ctx: &mut Context<InitializeGlobal>,
    config: GlobalConfig,
) -> Result<()> {
    check(
        ctx.remaining_accounts()?.is_empty(),
        crate::DexError::InvalidArgument,
    )?;
    let council = &ctx.accounts.council;
    let global = &mut ctx.accounts.global;
    use anchor_lang::prelude::Address;

    check(
        config.treasury != Address::default()
            && config.distributor != Address::default()
            && config.usdc_mint != Address::default()
            && config.treasury != *ctx.accounts.council.address(),
        crate::DexError::InvalidArgument,
    )?;
    global.council = *council.address();
    global.treasury = config.treasury;
    global.distributor = config.distributor;
    global.usdc_mint = config.usdc_mint;
    Ok(())
}

pub(crate) fn update_global(ctx: &mut Context<UpdateGlobal>, config: GlobalConfig) -> Result<()> {
    check(
        ctx.remaining_accounts()?.is_empty(),
        crate::DexError::InvalidArgument,
    )?;
    check(
        config.treasury != Address::default()
            && config.distributor != Address::default()
            && config.usdc_mint == ctx.accounts.global.usdc_mint
            && config.treasury != *ctx.accounts.new_council.address(),
        crate::DexError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    accounts.global.council = *accounts.new_council.address();
    accounts.global.treasury = config.treasury;
    accounts.global.distributor = config.distributor;
    Ok(())
}

pub(crate) fn update_config(ctx: &mut Context<UpdateConfig>, enabled: bool) -> Result<()> {
    check(
        ctx.remaining_accounts()?.is_empty(),
        crate::DexError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    token::mint(
        &accounts.mint,
        accounts.token.account(),
        Clock::get()?.epoch,
        true,
    )?;
    accounts.config.council = *accounts.council.address();
    accounts.config.quote_mint = *accounts.mint.address();
    accounts.config.treasury = accounts.global.treasury;
    accounts.config.distributor = accounts.global.distributor;
    if accounts.config.enabled == 0 && enabled {
        accounts.config.oracle_timestamp = 0.into();
        accounts.config.quote_per_usdc_x32 = 0.into();
        accounts.config.quote_price_timestamp = 0.into();
        accounts.config.oracle_cumulative = 0.into();
    }
    accounts.config.enabled = u8::from(enabled);
    Ok(())
}

pub(crate) fn refresh_quote(ctx: &mut Context<RefreshQuote>) -> Result<()> {
    check(
        ctx.remaining_accounts()?.is_empty(),
        crate::DexError::InvalidArgument,
    )?;
    check(
        ctx.accounts.config.enabled == 1,
        crate::DexError::Unauthorized,
    )?;
    let now = oracle::timestamp(&Clock::get()?)?;
    oracle::refresh(&mut ctx.accounts.config, &ctx.accounts.reference_pool, now)
}

pub(crate) fn update_recipients(ctx: &mut Context<UpdateRecipients>) -> Result<()> {
    check(
        ctx.remaining_accounts()?.is_empty(),
        crate::DexError::InvalidArgument,
    )?;
    let pool = &mut ctx.accounts.pool;
    let global = &ctx.accounts.global;
    pool.treasury = global.treasury;
    pool.distributor = global.distributor;
    Ok(())
}
