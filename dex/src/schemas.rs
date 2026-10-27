use crate::abi::*;
use anchor_lang::bytemuck::{Pod, Zeroable};
use anchor_lang::{Address, Discriminator, Owner};

// explicit legacy discriminators preserve deployed account bytes; each field is alignment one.
macro_rules! schema {
    ($name:ident, $magic:ident, {$($field:ident: $ty:ty),* $(,)?}) => {
        #[repr(C)]
        #[derive(Clone, Copy)]
        pub struct $name { $(pub $field: $ty),* }
        unsafe impl Zeroable for $name {}
        unsafe impl Pod for $name {}
        impl Owner for $name { const OWNER: Address = crate::ID; }
        impl Discriminator for $name { const DISCRIMINATOR: &'static [u8] = $magic; }
    };
}

schema!(GlobalState, GLOBAL_MAGIC, {
    admin: Address, treasury: Address, distributor: Address, reserved: [u8; 8],
});
schema!(ConfigState, CONFIG_MAGIC, {
    admin: Address, treasury: Address, quote_mint: Address,
    trade_bps: [u8; 2], protocol_share: [u8; 2], creator_share: [u8; 2],
    reserved: [u8; 2], distributor: Address, enabled: u8, padding: [u8; 47],
});
const _: () = {
    assert!(core::mem::size_of::<GlobalState>() + 8 == GLOBAL_LEN);
    assert!(core::mem::size_of::<ConfigState>() + 8 == CONFIG_LEN);
};
