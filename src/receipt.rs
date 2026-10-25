use crate::{
    abi::{BOOKING_LEN, BOOK_MAGIC},
    helpers::{err, rd64, require},
    state::{all_zero, fresh, market, user, writable},
};
use pinocchio::{error::ProgramError, AccountView, Address, ProgramResult};

pub const AVAILABLE: u8 = 5;
pub const RECEIPT_LEN: usize = 120;

pub fn prepare_booking(
    b: &AccountView,
    program: &Address,
    owner: &Address,
    market: &Address,
    user: &Address,
    rent_payer: &Address,
    p: &[u8],
) -> Result<u64, ProgramError> {
    require(p.len() == 115, 3)?;
    writable(&[b])?;
    require(b.owner() == program && b.data_len() == BOOKING_LEN, 2)?;
    let data = b.try_borrow()?;
    let next = rd64(p, 107);
    if all_zero(&data) {
        require(b.is_signer() && next > 0, 4)?;
    } else {
        require(
            &data[..8] == BOOK_MAGIC && data[148] == AVAILABLE && data[359] == 0,
            7,
        )?;
        require(
            &data[8..40] == owner.as_ref() && &data[72..104] == market.as_ref(),
            4,
        )?;
        require(
            &data[192..224] == user.as_ref() && &data[256..288] == rent_payer.as_ref(),
            4,
        )?;
        require(next > rd64(&data, 152), 7)?;
    }
    Ok(next)
}

pub fn settled(booking: &AccountView, charge: u64, published: &[u8]) -> ProgramResult {
    // top-level instruction bytes preserve authenticated receipts without optional cpi traces.
    #[cfg(target_os = "solana")]
    require(
        unsafe { pinocchio::syscalls::sol_get_stack_height() } == 1,
        4,
    )?;
    require(published.len() == RECEIPT_LEN, 3)?;
    let bd = booking.try_borrow()?;
    let mut receipt = [0; RECEIPT_LEN];
    receipt[..32].copy_from_slice(&bd[8..40]);
    receipt[32..64].copy_from_slice(&bd[192..224]);
    receipt[64..96].copy_from_slice(booking.address().as_ref());
    receipt[96..104].copy_from_slice(&bd[152..160]);
    receipt[104..112].copy_from_slice(&charge.to_le_bytes());
    receipt[112..120].copy_from_slice(&bd[132..140]);
    require(published == receipt, 3)?;
    #[cfg(target_os = "solana")]
    {
        let log: [&[u8]; 5] = [
            b"fossa/receipt/v2",
            &receipt,
            &bd[224..256],
            &bd[40..72],
            &bd[168..180],
        ];
        unsafe {
            pinocchio::syscalls::sol_log_data(log.as_ptr().cast(), log.len() as u64);
        }
    }
    Ok(())
}

pub fn initialize(program: &Address, accounts: &mut [AccountView], p: &[u8]) -> ProgramResult {
    let [m, u, owner, b, payer] = accounts else {
        return Err(err(3));
    };
    require(p.is_empty() && owner.is_signer() && payer.is_signer(), 4)?;
    market(m, program)?;
    user(u, program, owner.address(), m.address())?;
    fresh(b, program, BOOKING_LEN)?;
    let mut d = b.try_borrow_mut()?;
    d[..8].copy_from_slice(BOOK_MAGIC);
    d[8..40].copy_from_slice(owner.address().as_ref());
    d[72..104].copy_from_slice(m.address().as_ref());
    d[192..224].copy_from_slice(u.address().as_ref());
    d[256..288].copy_from_slice(payer.address().as_ref());
    d[148] = AVAILABLE;
    Ok(())
}
