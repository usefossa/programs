use super::*;

pub(crate) fn collect(ctx: &mut Context<CollectFees>, args: CollectArgs) -> Result<()> {
    check(
        ctx.remaining_accounts()?.is_empty(),
        crate::DexError::InvalidArgument,
    )?;
    let owner = ctx.accounts.owner.account();
    let pool = &mut ctx.accounts.pool;
    let authority = ctx.accounts.authority.account();
    let mint_a = &ctx.accounts.mint_a;
    let mint_b = &ctx.accounts.mint_b;
    let destination_a = &mut ctx.accounts.destination_a;
    let destination_b = &mut ctx.accounts.destination_b;
    let vault_a = &mut ctx.accounts.vault_a;
    let vault_b = &mut ctx.accounts.vault_b;
    let token_a = ctx.accounts.token_a.account();
    let token_b = ctx.accounts.token_b.account();

    let mut pool_state = **pool;
    let [mint_a_state, mint_b_state] = pool_state.validate(
        [mint_a, mint_b],
        [vault_a, vault_b],
        [token_a, token_b],
        Clock::get()?.epoch,
    )?;
    let protocol = args.protocol;
    check(
        if protocol {
            pool_state.treasury
        } else {
            pool_state.creator
        } == *owner.address(),
        crate::DexError::Unauthorized,
    )?;
    token::account(destination_a, false)?;
    token::account(destination_b, false)?;
    let (x, y) = if protocol {
        (pool_state.protocol_a.get(), pool_state.protocol_b.get())
    } else {
        (pool_state.creator_a.get(), pool_state.creator_b.get())
    };
    check(x > 0 || y > 0, crate::DexError::InvalidArgument)?;
    let bump = [pool_state.bump];
    let seed: &[&[u8]] = &[b"authority", pool.address().as_ref(), &bump];
    let signers = [seed];
    let nx = if x > 0 {
        token::transfer(
            vault_a,
            mint_a,
            destination_a,
            authority,
            x,
            mint_a_state.decimals,
            &signers,
        )?
    } else {
        0
    };
    let ny = if y > 0 {
        token::transfer(
            vault_b,
            mint_b,
            destination_b,
            authority,
            y,
            mint_b_state.decimals,
            &signers,
        )?
    } else {
        0
    };
    check(
        nx >= args.minimum_a && ny >= args.minimum_b,
        crate::DexError::Slippage,
    )?;
    if protocol {
        pool_state.protocol_a = 0.into();
        pool_state.protocol_b = 0.into();
    } else {
        pool_state.creator_a = 0.into();
        pool_state.creator_b = 0.into();
    }
    **pool = pool_state;
    Ok(())
}

pub(crate) fn collect_tax(ctx: &mut Context<CollectTransferTax>) -> Result<()> {
    let mut extra = ctx.remaining_accounts()?;
    let pool = &mut ctx.accounts.pool;
    let authority = ctx.accounts.authority.account();
    let mint = &mut ctx.accounts.mint;
    let destination = &mut ctx.accounts.destination;
    let token = ctx.accounts.token.account();

    use anchor_lang::prelude::Clock;
    use anchor_lang::solana_program::sysvar::Sysvar as _;
    check(extra.len() <= 16, crate::DexError::InvalidArgument)?;
    let sources = extra.as_mut_slice();
    let pool_state = **pool;
    check(
        matches!(pool_state.mode, REWARD | COMMUNITY),
        crate::DexError::InvalidAccount,
    )?;
    let mint_state = token::mint(mint, token, Clock::get()?.epoch, false)?;
    check(
        mint_state.fee_bps == pool_state.tax_bps.get() && mint_state.fee_cap == u64::MAX,
        crate::DexError::InvalidAccount,
    )?;
    token::account(destination, false)?;
    let before = destination.amount();
    let bump = [pool_state.bump];
    let seed: &[&[u8]] = &[b"authority", pool.address().as_ref(), &bump];
    let signers = [seed];
    use anchor_spl::token_2022_extensions as extensions;
    if !sources.is_empty() {
        for (i, source) in sources.iter().enumerate() {
            check(
                source.address() != destination.address()
                    && source.owner() == &anchor_spl::token_2022::ID
                    && source.is_writable()
                    && source.data_len() >= 165,
                crate::DexError::InvalidAccount,
            )?;
            check(
                InterfaceAccount::<anchor_spl::token_interface::TokenAccount>::load(*source)?
                    .mint()
                    == mint.address(),
                crate::DexError::InvalidAccount,
            )?;
            check(
                sources[..i]
                    .iter()
                    .all(|previous| previous.address() != source.address()),
                crate::DexError::InvalidAccount,
            )?;
        }
        let source_handles = sources.iter_mut().map(CpiHandleMut::writable).collect();
        extensions::withdraw_withheld_tokens_from_accounts(
            CpiContext::new_with_signer(
                token.address(),
                extensions::WithdrawWithheldTokensFromAccounts {
                    mint: mint.to_cpi_handle(),
                    destination: destination.try_to_cpi_handle_mut()?,
                    authority: CpiHandle::readonly(authority),
                },
                &signers,
            ),
            source_handles,
        )?;
    }
    extensions::withdraw_withheld_tokens_from_mint(CpiContext::new_with_signer(
        token.address(),
        extensions::WithdrawWithheldTokensFromMint {
            mint: mint.try_to_cpi_handle_mut()?,
            destination: destination.try_to_cpi_handle_mut()?,
            authority: CpiHandle::readonly(authority),
        },
        &signers,
    ))?;
    let collected = sub(destination.amount(), before)?;
    check(collected > 0, crate::DexError::InvalidArgument)?;
    pool.tax_collected = pool
        .tax_collected
        .get()
        .checked_add(u128::from(collected))
        .ok_or(anchor_lang::prelude::ProgramError::ArithmeticOverflow)?
        .into();
    Ok(())
}
