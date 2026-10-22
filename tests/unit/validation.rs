use crate::{
    abi::*,
    helpers::{add, collateral, rd16, sub, wr32, wr64},
    matching::{select, Request},
    state::{metadata_payload, rates, write_metadata},
};
use std::vec;

fn request() -> [u8; 107] {
    let mut data = [0; 107];
    for offset in [46, 50, 63, 71] {
        wr32(&mut data, offset, 10);
    }
    wr64(&mut data, 55, 100);
    data
}

#[test]
fn requests_reject_every_wrong_length_and_invalid_usage_bound() {
    let data = request();
    for length in 0..=128 {
        let mut malformed = data.to_vec();
        malformed.resize(length, 0);
        assert_eq!(Request::parse(&malformed, 100).is_ok(), length == 107);
    }
    for (offset, value) in [(46, 0), (50, 0), (63, 11), (67, 11), (71, 11)] {
        let mut malformed = data;
        wr32(&mut malformed, offset, value);
        assert!(Request::parse(&malformed, 100).is_err(), "offset {offset}");
    }
    for flag in 2..=u8::MAX {
        let mut malformed = data;
        malformed[54] = flag;
        assert!(Request::parse(&malformed, 100).is_err());
    }
    let mut cached = data;
    wr32(&mut cached, 67, 1);
    assert!(Request::parse(&cached, 100).is_err());
    cached[75] = 1;
    assert!(Request::parse(&cached, 100).is_ok());
    assert_eq!(Request::parse(&data, 99).unwrap().spend, 99);
    assert_eq!(Request::parse(&data, 101).unwrap().spend, 100);
}

#[test]
fn metadata_accepts_utf8_byte_boundaries_and_rejects_all_ascii_controls() {
    for size in 0..=URI_MAX + 1 {
        let payload = vec![b'a'; 32 + size];
        assert_eq!(
            metadata_payload(&payload).is_ok(),
            (1..=URI_MAX).contains(&size)
        );
    }
    for byte in (0..=32).chain(std::iter::once(127)) {
        let mut payload = vec![b'a'; 64];
        payload[47] = byte;
        assert!(metadata_payload(&payload).is_err(), "byte {byte}");
    }
    for bytes in [
        &[0xff][..],
        &[0xc3][..],
        &[0xc0, 0xaf][..],
        &[0xed, 0xa0, 0x80][..],
    ] {
        let mut payload = vec![0; 32];
        payload.extend_from_slice(bytes);
        assert!(metadata_payload(&payload).is_err());
    }
    let mut payload = vec![0; 32];
    payload.extend_from_slice("é".repeat(128).as_bytes());
    assert!(metadata_payload(&payload).is_ok());
    payload.extend_from_slice("é".as_bytes());
    assert!(metadata_payload(&payload).is_err());
}

#[test]
fn shorter_metadata_updates_clear_stale_bytes_and_preserve_account_identity() {
    for (size, hash, uri) in [(PROVIDER_LEN, 72, 108), (MODEL_METADATA_LEN, 104, 140)] {
        let mut account = vec![0x55; size];
        for length in (1..=URI_MAX).rev() {
            let mut payload = vec![0x99; 32];
            payload.extend(vec![b'x'; length]);
            write_metadata(&mut account, hash, uri, &payload);
            assert!(account[..hash].iter().all(|byte| *byte == 0x55));
            assert_eq!(&account[hash..hash + 32], &payload[..32]);
            assert_eq!(rd16(&account, hash + 32) as usize, length);
            assert!(account[hash + 34..uri].iter().all(|byte| *byte == 0x55));
            assert_eq!(&account[uri..uri + length], &payload[32..]);
            assert!(account[uri + length..].iter().all(|byte| *byte == 0));
        }
    }
}

#[test]
fn rates_validate_exact_numeric_and_boolean_limits() {
    let mut valid = [0; 20];
    wr32(&mut valid, 0, u32::MAX);
    wr32(&mut valid, 4, u32::MAX);
    wr32(&mut valid, 8, u32::MAX);
    valid[12..14].copy_from_slice(&u16::MAX.to_le_bytes());
    valid[14] = 1;
    valid[15] = 1;
    wr32(&mut valid, 16, u32::MAX);
    assert!(rates(&valid).is_ok());
    for offset in [0, 8, 16] {
        let mut invalid = valid;
        wr32(&mut invalid, offset, 0);
        assert!(rates(&invalid).is_err());
    }
    let mut invalid = valid;
    invalid[12..14].fill(0);
    assert!(rates(&invalid).is_err());
    for offset in [14, 15] {
        for value in 2..=u8::MAX {
            let mut invalid = valid;
            invalid[offset] = value;
            assert!(rates(&invalid).is_err());
        }
    }
    invalid = valid;
    wr32(&mut invalid, 0, u32::MAX - 1);
    assert!(rates(&invalid).is_err());
}

#[test]
fn matching_rejects_truncated_books_and_inconsistent_registered_counts() {
    let request = Request::parse(&request(), 100).unwrap();
    let mut terms = vec![0; terms_book_len(2)];
    let mut capacity = vec![0; capacity_book_len(2)];
    wr32(&mut terms, 44, 2);
    wr32(&mut capacity, 44, 2);
    for length in HEADER_LEN..terms.len() {
        assert!(select(&terms[..length], &capacity, &request).is_err());
    }
    for length in HEADER_LEN..capacity.len() {
        assert!(select(&terms, &capacity[..length], &request).is_err());
    }
    for count in [0, 1, 3, MAX_ENTRIES as u32 + 1, u32::MAX] {
        wr32(&mut capacity, 44, count);
        assert!(select(&terms, &capacity, &request).is_err());
    }
}

#[test]
fn checked_accounting_matches_u128_at_overflow_boundaries() {
    let values = [0, 1, 99, 100, 101, u64::MAX / 2, u64::MAX - 1, u64::MAX];
    for a in values {
        let wide = u128::from(a) + u128::from(a).div_ceil(100);
        assert_eq!(collateral(a).ok(), u64::try_from(wide).ok());
        for b in values {
            assert_eq!(
                add(a, b).ok(),
                u64::try_from(u128::from(a) + u128::from(b)).ok()
            );
            assert_eq!(sub(a, b).ok(), a.checked_sub(b));
        }
    }
}
