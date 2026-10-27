use crate::abi::*;
use anchor_lang::bytemuck::{Pod, Zeroable};
use anchor_lang::prelude::{PodU32, PodU64};
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

schema!(MarketState, MARKET_MAGIC, {admin: Address, mint: Address});
schema!(ModelState, MODEL_MAGIC, {
    market: Address, model_id: Address, terms: Address, capacity: Address,
    limit: PodU32, bump: u8, reserved: [u8; 3],
});
schema!(ProviderState, PROVIDER_MAGIC, {
    market: Address, authority: Address, document_hash: [u8; 32],
    uri_len: [u8; 2], reserved: u8, bump: u8, uri: [u8; URI_MAX],
});
schema!(UserState, USER_MAGIC, {
    owner: Address, market: Address, free: PodU64, vault: Address,
    bump: u8, generation: [u8; 7],
});
schema!(TermsState, TERMS_MAGIC, {model: Address, limit: PodU32, count: PodU32});
schema!(ListState, LIST_MAGIC, {market: Address, authority: Address, limit: PodU32, count: PodU32});

const _: () = {
    assert!(core::mem::size_of::<MarketState>() + 8 == MARKET_LEN);
    assert!(core::mem::size_of::<ModelState>() + 8 == MODEL_LEN);
    assert!(core::mem::size_of::<ProviderState>() + 8 == PROVIDER_LEN);
    assert!(core::mem::size_of::<UserState>() + 8 == USER_LEN);
    assert!(core::mem::size_of::<TermsState>() + 8 == HEADER_LEN);
    assert!(core::mem::size_of::<ListState>() + 8 == LIST_HEADER);
};
