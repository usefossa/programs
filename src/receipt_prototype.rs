use super::*;

pub const AVAILABLE: u8 = 5;
pub const CASE: u8 = 0xa5;
pub const DEPTH: usize = 16;
pub const HEADER: usize = 224;
pub const BUCKET_LEN: usize = HEADER + DEPTH * 32;
pub const RECEIPT_LEN: usize = 120;
const MAGIC: &[u8; 8] = b"RECEIPT1";

#[cfg(target_os = "solana")]
fn hash(parts: &[&[u8]]) -> [u8; 32] {
    let mut output = [0; 32];
    // slice descriptors are the sha256 syscall's pointer/length ABI.
    unsafe {
        pinocchio::syscalls::sol_sha256(
            parts.as_ptr().cast(),
            parts.len() as u64,
            output.as_mut_ptr(),
        );
    }
    output
}

#[cfg(not(target_os = "solana"))]
fn hash(parts: &[&[u8]]) -> [u8;32] {
    use sha2::{Digest,Sha256};
    let mut hash=Sha256::new();
    for part in parts { hash.update(part); }
    hash.finalize().into()
}

fn leaf(
    program: &Address,
    bucket: &Address,
    epoch: &[u8],
    index: &[u8],
    receipt: &[u8],
) -> [u8; 32] {
    hash(&[
        b"fossa/receipt/v1",
        program.as_ref(),
        bucket.as_ref(),
        epoch,
        index,
        receipt,
    ])
}

fn node(left: &[u8], right: &[u8]) -> [u8; 32] {
    hash(&[b"fossa/node/v1", left, right])
}

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
            &data[..8] == BOOK_MAGIC && data[148] == AVAILABLE && data[359] != CASE,
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

pub fn settled(
    program: &Address,
    booking: &mut AccountView,
    bucket: &mut AccountView,
    charge: u64,
    published: &[u8],
) -> ProgramResult {
    // top-level instruction bytes remain available independently of optional cpi traces.
    #[cfg(target_os = "solana")]
    require(unsafe { pinocchio::syscalls::sol_get_stack_height() } == 1, 4)?;
    require(published.len() == 16 + RECEIPT_LEN, 3)?;
    let bd = booking.try_borrow()?;
    let mut receipt = [0; RECEIPT_LEN];
    receipt[..32].copy_from_slice(&bd[8..40]);
    receipt[32..64].copy_from_slice(&bd[192..224]);
    receipt[64..96].copy_from_slice(booking.address().as_ref());
    receipt[96..104].copy_from_slice(&bd[152..160]);
    receipt[104..112].copy_from_slice(&charge.to_le_bytes());
    receipt[112..120].copy_from_slice(&bd[140..148]);
    require(published[16..] == receipt, 3)?;
    if charge == 0 {
        drop(bd);
        booking.try_borrow_mut()?[148] = AVAILABLE;
        return Ok(());
    }
    writable(&[bucket])?;
    let hd = state(bucket, program, BUCKET_LEN, MAGIC)?;
    require(
        hd[173] == 0
            && hd[8..40] == bd[72..104]
            && hd[40..72] == bd[224..256]
            && hd[72..104] == bd[40..72]
            && hd[104..136] == bd[320..352]
            && rd32(&hd, 168) == rd32(&bd, 352)
            && hd[172] == bd[356],
        2,
    )?;
    let now = Clock::get()?.unix_timestamp;
    require(now >= ri64(&hd, 176) && now < ri64(&hd, 184), 7)?;
    let count = rd64(&hd, 200);
    require(count < (1u64 << DEPTH) - 1, 6)?;
    require(published[..8] == hd[176..184] && rd64(published, 8) == count, 7)?;
    let total = add(rd64(&hd, 208), charge)?;
    let outstanding = add(rd64(&hd, 216), 1)?;
    let deadline = ri64(&hd, 192).max(ri64(&bd, 140));
    let epoch = ri64(&hd, 176).to_le_bytes();
    let mut details = [0u8; 76];
    details[..32].copy_from_slice(&bd[224..256]);
    details[32..64].copy_from_slice(&bd[40..72]);
    details[64..76].copy_from_slice(&bd[168..180]);
    drop(bd);
    drop(hd);
    let index = count.to_le_bytes();
    let mut digest = leaf(program, bucket.address(), &epoch, &index, &receipt);
    let mut hd = bucket.try_borrow_mut()?;
    let mut height = 0;
    while count & (1 << height) != 0 {
        let at = HEADER + height * 32;
        digest = node(&hd[at..at + 32], &digest);
        height += 1;
    }
    hd[HEADER + height * 32..HEADER + (height + 1) * 32].copy_from_slice(&digest);
    wr64(&mut hd, 200, count + 1);
    wr64(&mut hd, 208, total);
    wr64(&mut hd, 216, outstanding);
    wi64(&mut hd, 192, deadline);
    drop(hd);
    booking.try_borrow_mut()?[148] = AVAILABLE;
    // instruction data preserves canonical receipts even when runtime logs are truncated.
    #[cfg(target_os = "solana")]
    let log: [&[u8]; 6] = [
        b"fossa/receipt/v1",
        bucket.address().as_ref(),
        &epoch,
        &index,
        &receipt,
        &details,
    ];
    #[cfg(target_os = "solana")]
    unsafe {
        pinocchio::syscalls::sol_log_data(log.as_ptr().cast(), log.len() as u64);
    }
    Ok(())
}

pub fn process(program: &Address, accounts: &mut [AccountView], op: u8, p: &[u8]) -> ProgramResult {
    match op {
        23 => {
            let [m, mo, t, bucket, payer] = accounts else {
                return Err(err(3));
            };
            require((p.len() == 8 || p.len() == 16) && payer.is_signer(), 3)?;
            writable(&[bucket])?;
            market(m, program)?;
            let modl = model(mo, program, m.address())?;
            let td = book(t, program, mo, &modl, true)?;
            let slot = rd32(p, 0);
            let at = slot_offset(&td, slot, true)?;
            let duration = i64::from(rd32(p, 4));
            require(duration > 0 && duration <= DISPUTE_SECONDS / 2, 3)?;
            let now = Clock::get()?.unix_timestamp;
            let current = now.div_euclid(duration) * duration;
            let epoch = if p.len() == 16 { ri64(p, 8) } else { current };
            require(epoch == current || epoch == current.checked_add(duration).ok_or(err(1))?, 3)?;
            require(
                bucket.owner() == program && bucket.data_len() == BUCKET_LEN,
                2,
            )?;
            let old = bucket.try_borrow()?;
            if all_zero(&old) {
                require(bucket.is_signer(), 4)?;
            } else {
                require(
                    &old[..8] == MAGIC
                        && old[173] == 1
                        && rd64(&old, 208) == 0
                        && rd64(&old, 216) == 0,
                    7,
                )?;
                require(
                    &old[136..168] == payer.address().as_ref() && epoch > ri64(&old, 176),
                    4,
                )?;
            }
            drop(old);
            let mut d = bucket.try_borrow_mut()?;
            d.fill(0);
            d[..8].copy_from_slice(MAGIC);
            d[8..40].copy_from_slice(m.address().as_ref());
            d[40..72].copy_from_slice(mo.address().as_ref());
            d[72..104].copy_from_slice(&td[at..at + 32]);
            d[104..136].copy_from_slice(&td[at + 64..at + 96]);
            d[136..168].copy_from_slice(payer.address().as_ref());
            wr32(&mut d, 168, slot);
            d[172] = td[at + 113];
            wi64(&mut d, 176, epoch);
            wi64(&mut d, 184, epoch.checked_add(duration).ok_or(err(1))?);
            Ok(())
        }
        24 => {
            let [m, mo, c, bucket] = accounts else {
                return Err(err(3));
            };
            require(p.is_empty(), 3)?;
            writable(&[c, bucket])?;
            market(m, program)?;
            let modl = model(mo, program, m.address())?;
            let cd = book(c, program, mo, &modl, false)?;
            let hd = state(bucket, program, BUCKET_LEN, MAGIC)?;
            require(
                &hd[8..40] == m.address().as_ref()
                    && &hd[40..72] == mo.address().as_ref()
                    && hd[173] == 0,
                2,
            )?;
            let now = Clock::get()?.unix_timestamp;
            require(now >= ri64(&hd, 184) && now > ri64(&hd, 192), 7)?;
            let at = slot_offset(&cd, rd32(&hd, 168), false)?;
            let exposure = sub(rd64(&cd, at + 8), rd64(&hd, 208))?;
            let obligations = rd32(&cd, at + 20)
                .checked_sub(u32::try_from(rd64(&hd, 216)).map_err(|_| err(1))?)
                .ok_or(err(1))?;
            drop(cd);
            drop(hd);
            let mut cd = c.try_borrow_mut()?;
            wr64(&mut cd, at + 8, exposure);
            wr32(&mut cd, at + 20, obligations);
            let mut hd = bucket.try_borrow_mut()?;
            hd[173] = 1;
            wr64(&mut hd, 208, 0);
            wr64(&mut hd, 216, 0);
            Ok(())
        }
        25 => {
            let [m, mo, bucket, case, owner, payer, system] = accounts else {
                return Err(err(3));
            };
            require(
                p.len() >= 16 + RECEIPT_LEN && (p.len() - 16 - RECEIPT_LEN) % 32 == 0,
                3,
            )?;
            require(
                owner.is_signer() && payer.is_signer() && system.address() == &pinocchio_system::ID,
                4,
            )?;
            writable(&[bucket, case, payer])?;
            market(m, program)?;
            model(mo, program, m.address())?;
            let hd = state(bucket, program, BUCKET_LEN, MAGIC)?;
            require(
                &hd[8..40] == m.address().as_ref()
                    && &hd[40..72] == mo.address().as_ref()
                    && hd[173] == 0,
                2,
            )?;
            let epoch = &p[..8];
            let index = rd64(p, 8);
            let receipt = &p[16..16 + RECEIPT_LEN];
            require(
                hd[176..184] == *epoch && &receipt[..32] == owner.address().as_ref(),
                4,
            )?;
            require(Clock::get()?.unix_timestamp <= ri64(receipt, 112), 7)?;
            let count = rd64(&hd, 200);
            require(index < count, 3)?;
            let mut base = 0;
            let mut height = 0;
            for level in (0..DEPTH).rev() {
                if count & (1 << level) == 0 {
                    continue;
                }
                if index < base + (1 << level) {
                    height = level;
                    break;
                }
                base += 1 << level;
            }
            require(p.len() == 16 + RECEIPT_LEN + height * 32, 3)?;
            let mut digest = leaf(program, bucket.address(), epoch, &p[8..16], receipt);
            for level in 0..height {
                let at = 16 + RECEIPT_LEN + level * 32;
                digest = if (index - base) & (1 << level) == 0 {
                    node(&digest, &p[at..at + 32])
                } else {
                    node(&p[at..at + 32], &digest)
                };
            }
            require(
                hd[HEADER + height * 32..HEADER + (height + 1) * 32] == digest,
                2,
            )?;
            let (address, bump) = Address::derive_program_address(
                &[b"receipt-case", bucket.address().as_ref(), epoch, &p[8..16]],
                program,
            )
            .ok_or(err(2))?;
            require(case.address() == &address, 2)?;
            let charge = rd64(receipt, 104);
            let total = sub(rd64(&hd, 208), charge)?;
            let outstanding = sub(rd64(&hd, 216), 1)?;
            let bump_seed = [bump];
            let signer_seeds = seeds!(
                b"receipt-case",
                bucket.address().as_ref(),
                epoch,
                &p[8..16],
                &bump_seed
            );
            initialize_pda(
                case,
                payer,
                program,
                BOOKING_LEN,
                Signer::from(&signer_seeds),
            )?;
            let mut d = case.try_borrow_mut()?;
            d[..8].copy_from_slice(BOOK_MAGIC);
            d[8..40].copy_from_slice(owner.address().as_ref());
            d[40..72].copy_from_slice(&hd[72..104]);
            d[72..104].copy_from_slice(m.address().as_ref());
            d[140..148].copy_from_slice(&receipt[112..120]);
            d[148] = 2;
            d[152..160].copy_from_slice(&receipt[96..104]);
            wr64(&mut d, 160, charge);
            d[192..224].copy_from_slice(&receipt[32..64]);
            d[224..256].copy_from_slice(mo.address().as_ref());
            d[256..288].copy_from_slice(payer.address().as_ref());
            d[320..352].copy_from_slice(&hd[104..136]);
            d[352..356].copy_from_slice(&hd[168..172]);
            d[356] = hd[172];
            d[359] = CASE;
            drop(d);
            drop(hd);
            let mut hd = bucket.try_borrow_mut()?;
            wr64(&mut hd, 208, total);
            wr64(&mut hd, 216, outstanding);
            Ok(())
        }
        26 => {
            let [m, u, owner, b, payer] = accounts else { return Err(err(3)); };
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
        27 => {
            let [bucket, recipient, authority] = accounts else { return Err(err(3)); };
            require(p.is_empty() && bucket.address() != recipient.address(), 3)?;
            writable(&[bucket, recipient])?;
            let d = state(bucket, program, BUCKET_LEN, MAGIC)?;
            require(
                authority.is_signer()
                    && (&d[136..168] == authority.address().as_ref()
                        || &d[72..104] == authority.address().as_ref())
                    && &d[136..168] == recipient.address().as_ref(),
                4,
            )?;
            let now = Clock::get()?.unix_timestamp;
            require(
                d[173] == 1 && rd64(&d, 208) == 0 && rd64(&d, 216) == 0
                    && now >= ri64(&d, 184) && now > ri64(&d, 192),
                7,
            )?;
            drop(d);
            recipient.set_lamports(add(recipient.lamports(), bucket.lamports())?);
            bucket.close()
        }
        _ => Err(err(3)),
    }
}
