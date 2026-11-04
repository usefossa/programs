use crate::{args::ReceiptInput, helpers::require, state_types::Booking};
use anchor_lang::prelude::{Account, Address, ProgramError, ProgramResult};

pub const AVAILABLE: u8 = 5;
pub fn prepare_booking(
    booking: &Account<Booking>,
    owner: &Address,
    market: &Address,
    user: &Address,
    rent_payer: &Address,
    generation: Option<u64>,
) -> Result<u64, ProgramError> {
    require(
        booking.status == AVAILABLE
            && booking.reserved5 == 0
            && booking.owner == *owner
            && booking.market == *market
            && booking.user == *user
            && booking.rent_payer == *rent_payer,
        crate::RouterError::Unauthorized,
    )?;
    if let Some(next) = generation {
        require(
            next > booking.generation.get(),
            crate::RouterError::InvalidState,
        )?;
        Ok(next)
    } else {
        require(
            booking.generation.get() == 0,
            crate::RouterError::InvalidState,
        )?;
        Ok(0)
    }
}

pub fn settled(booking: &Account<Booking>, charge: u64, published: ReceiptInput) -> ProgramResult {
    // top-level instruction bytes preserve authenticated receipts without optional cpi traces.
    #[cfg(target_os = "solana")]
    require(
        unsafe { anchor_lang::solana_program::instruction::syscalls::sol_get_stack_height() } == 1,
        crate::RouterError::Unauthorized,
    )?;
    let receipt = ReceiptInput {
        owner: booking.owner,
        user: booking.user,
        booking: *booking.address(),
        generation: booking.generation.get(),
        charge,
        claim_end: booking.claim_end.get(),
    };
    require(published == receipt, crate::RouterError::InvalidArgument)?;
    #[cfg(target_os = "solana")]
    {
        let receipt_data = crate::state_types::Receipt {
            owner: receipt.owner,
            user: receipt.user,
            booking: receipt.booking,
            generation: receipt.generation.into(),
            charge: receipt.charge.into(),
            claim_end: receipt.claim_end.into(),
        };
        let usage = [
            booking.input.get().to_le_bytes(),
            booking.cached.get().to_le_bytes(),
            booking.output.get().to_le_bytes(),
        ]
        .concat();
        let log: [&[u8]; 5] = [
            b"fossa/receipt/v2",
            anchor_lang::bytemuck::bytes_of(&receipt_data),
            booking.model.as_ref(),
            booking.provider.as_ref(),
            &usage,
        ];
        anchor_lang::sol_log_data(&log);
    }
    Ok(())
}

pub fn initialize(
    ctx: &mut anchor_lang::Context<crate::contexts::InitializeReceipt>,
) -> ProgramResult {
    require(
        ctx.remaining_accounts()?.is_empty(),
        crate::RouterError::InvalidArgument,
    )?;
    let accounts = &mut ctx.accounts;
    accounts.booking.owner = *accounts.owner.address();
    accounts.booking.market = *accounts.market.address();
    accounts.booking.user = *accounts.user.address();
    accounts.booking.rent_payer = *accounts.payer.address();
    accounts.booking.status = AVAILABLE;
    Ok(())
}
