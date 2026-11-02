use crate::{
    helpers::{dec, require},
    state_types::{Booking, WalletBookingEntry, WalletBookings},
    RouterError,
};
use anchor_lang::{accounts::Slab, prelude::*};
fn lookup(
    wallet: &Slab<WalletBookings, WalletBookingEntry>,
    index: u32,
) -> core::result::Result<usize, usize> {
    if wallet
        .get(index as usize)
        .is_some_and(|entry| entry.provider_policy.get() == index)
    {
        Ok(index as usize)
    } else {
        wallet
            .as_slice()
            .binary_search_by_key(&index, |entry| entry.provider_policy.get())
    }
}

pub(crate) fn settle(
    wallet: Option<&mut Slab<WalletBookings, WalletBookingEntry>>,
    booking: &Booking,
) -> ProgramResult {
    require(
        booking.wallet_booking_counted <= 1,
        RouterError::InvalidState,
    )?;
    if booking.wallet_booking_counted == 0 {
        return Ok(());
    }
    let wallet = wallet.ok_or(RouterError::InvalidAccount)?;
    let slot =
        lookup(wallet, booking.provider_policy.get()).map_err(|_| RouterError::InvalidAccount)?;
    wallet[slot].outstanding = dec(wallet[slot].outstanding.get())?.into();
    Ok(())
}
