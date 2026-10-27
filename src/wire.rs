use anchor_lang::wincode::{
    config::ConfigCore,
    error::{ReadResult, WriteResult},
    io::{ReadError, Reader, Writer},
    SchemaRead, SchemaWrite,
};
use core::mem::MaybeUninit;

// legacy instructions contain an unprefixed payload; its bounded tail is not a borsh vec.
#[derive(Clone, Debug)]
pub struct Payload(pub alloc::vec::Vec<u8>);

unsafe impl<'de, C: ConfigCore> SchemaRead<'de, C> for Payload {
    type Dst = Self;

    fn read(mut reader: impl Reader<'de>, dst: &mut MaybeUninit<Self>) -> ReadResult<()> {
        let mut data = alloc::vec::Vec::with_capacity(324);
        loop {
            match reader.take_byte() {
                Ok(byte) if data.len() < 324 => data.push(byte),
                Ok(_) => {
                    return Err(anchor_lang::wincode::error::ReadError::Custom(
                        "payload too long",
                    ))
                }
                Err(ReadError::ReadSizeLimit(_)) => break,
                Err(error) => return Err(error.into()),
            }
        }
        dst.write(Self(data));
        Ok(())
    }
}

unsafe impl<C: ConfigCore> SchemaWrite<C> for Payload {
    type Src = Self;

    fn size_of(src: &Self) -> WriteResult<usize> {
        Ok(src.0.len())
    }

    fn write(mut writer: impl Writer, src: &Self) -> WriteResult<()> {
        writer.write(&src.0)?;
        Ok(())
    }
}

// fixed legacy payloads have no length prefix and reject every trailing byte.
#[derive(Clone, Debug)]
pub struct FixedPayload<const N: usize>(pub [u8; N]);

unsafe impl<'de, C: ConfigCore, const N: usize> SchemaRead<'de, C> for FixedPayload<N> {
    type Dst = Self;

    fn read(mut reader: impl Reader<'de>, dst: &mut MaybeUninit<Self>) -> ReadResult<()> {
        let data = reader.take_array()?;
        match reader.take_byte() {
            Err(ReadError::ReadSizeLimit(_)) => {
                dst.write(Self(data));
                Ok(())
            }
            Err(error) => Err(error.into()),
            Ok(_) => Err(anchor_lang::wincode::error::ReadError::Custom(
                "trailing payload",
            )),
        }
    }
}

unsafe impl<C: ConfigCore, const N: usize> SchemaWrite<C> for FixedPayload<N> {
    type Src = Self;

    fn size_of(_: &Self) -> WriteResult<usize> {
        Ok(N)
    }

    fn write(mut writer: impl Writer, src: &Self) -> WriteResult<()> {
        writer.write(&src.0)?;
        Ok(())
    }
}

// legacy variants either stop after the prefix or contain the complete fixed tail.
#[derive(Clone, Debug)]
pub struct OptionalTail<const PREFIX: usize, const TAIL: usize>(pub alloc::vec::Vec<u8>);

unsafe impl<'de, C: ConfigCore, const PREFIX: usize, const TAIL: usize> SchemaRead<'de, C>
    for OptionalTail<PREFIX, TAIL>
{
    type Dst = Self;

    fn read(mut reader: impl Reader<'de>, dst: &mut MaybeUninit<Self>) -> ReadResult<()> {
        const { assert!(TAIL > 0 && PREFIX + TAIL <= 324) };
        let mut data = alloc::vec::Vec::with_capacity(PREFIX + TAIL);
        data.extend_from_slice(&reader.take_array::<PREFIX>()?);
        match reader.take_byte() {
            Err(ReadError::ReadSizeLimit(_)) => {}
            Err(error) => return Err(error.into()),
            Ok(first) => {
                let mut tail = [0; TAIL];
                tail[0] = first;
                reader.copy_into_slice(&mut tail[1..])?;
                data.extend_from_slice(&tail);
                match reader.take_byte() {
                    Err(ReadError::ReadSizeLimit(_)) => {}
                    Err(error) => return Err(error.into()),
                    Ok(_) => {
                        return Err(anchor_lang::wincode::error::ReadError::Custom(
                            "trailing payload",
                        ))
                    }
                }
            }
        }
        dst.write(Self(data));
        Ok(())
    }
}

unsafe impl<C: ConfigCore, const PREFIX: usize, const TAIL: usize> SchemaWrite<C>
    for OptionalTail<PREFIX, TAIL>
{
    type Src = Self;

    fn size_of(src: &Self) -> WriteResult<usize> {
        Ok(src.0.len())
    }

    fn write(mut writer: impl Writer, src: &Self) -> WriteResult<()> {
        writer.write(&src.0)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anchor_lang::{
        wincode::config::{deserialize, serialize},
        BORSH_CONFIG,
    };

    #[test]
    fn fixed_wire_payload_rejects_truncation_and_trailing_bytes() {
        fn check<const N: usize>() {
            for len in 0..=325 {
                let bytes: alloc::vec::Vec<_> = (0..len).map(|i| i as u8).collect();
                let decoded = deserialize::<FixedPayload<N>, _>(&bytes, BORSH_CONFIG);
                assert_eq!(decoded.is_ok(), len == N);
                if let Ok(decoded) = decoded {
                    assert_eq!(serialize(&decoded, BORSH_CONFIG).unwrap(), bytes);
                    assert_eq!(decoded.0.as_slice(), bytes);
                }
            }
        }
        check::<0>();
        check::<1>();
        check::<4>();
        check::<8>();
        check::<11>();
        check::<16>();
        check::<17>();
        check::<24>();
        check::<33>();
        check::<40>();
        check::<59>();
        check::<64>();
        check::<137>();
    }

    #[test]
    fn variable_wire_payload_preserves_unprefixed_bytes_and_bounds() {
        for len in 0..=325 {
            let bytes: alloc::vec::Vec<_> = (0..len).map(|i| i as u8).collect();
            let decoded = deserialize::<Payload, _>(&bytes, BORSH_CONFIG);
            assert_eq!(decoded.is_ok(), len <= 324);
            if let Ok(decoded) = decoded {
                assert_eq!(serialize(&decoded, BORSH_CONFIG).unwrap(), bytes);
                assert_eq!(decoded.0, bytes);
            }
        }
    }

    #[test]
    fn optional_wire_tail_accepts_only_complete_legacy_variants() {
        fn check<const PREFIX: usize, const TAIL: usize>() {
            for len in 0..=325 {
                let bytes: alloc::vec::Vec<_> = (0..len).map(|i| i as u8).collect();
                let decoded = deserialize::<OptionalTail<PREFIX, TAIL>, _>(&bytes, BORSH_CONFIG);
                assert_eq!(decoded.is_ok(), len == PREFIX || len == PREFIX + TAIL);
                if let Ok(decoded) = decoded {
                    assert_eq!(serialize(&decoded, BORSH_CONFIG).unwrap(), bytes);
                    assert_eq!(decoded.0, bytes);
                }
            }
        }
        check::<0, 8>();
        check::<4, 12>();
        check::<12, 128>();
        check::<38, 32>();
        check::<43, 1>();
        check::<107, 8>();
    }
}
