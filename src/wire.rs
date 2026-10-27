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
