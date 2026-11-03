use crate::state_types::{MODALITIES, URI_MAX};
use alloc::string::String;
use anchor_lang::prelude::*;

#[derive(Clone, IdlType, AnchorDeserialize, AnchorSerialize)]
pub struct Metadata {
    pub document_hash: [u8; 32],
    pub uri: String,
}

impl Metadata {
    pub fn validate(&self) -> ProgramResult {
        crate::helpers::require(
            (1..=URI_MAX).contains(&self.uri.len())
                && self
                    .uri
                    .as_bytes()
                    .iter()
                    .all(|byte| !byte.is_ascii_control() && !byte.is_ascii_whitespace()),
            crate::RouterError::InvalidArgument,
        )
    }
}

#[derive(Clone, Copy, IdlType, AnchorDeserialize, AnchorSerialize)]
pub struct Offer {
    pub slot: u32,
    pub input_rate: u32,
    pub cached_rate: u32,
    pub output_rate: u32,
    pub speed: u16,
    pub privacy: bool,
    pub active: bool,
    pub capacity: u32,
    // sha-256 of an immutable checkpoint identity; zero never means all checkpoints.
    pub checkpoint: [u8; 32],
    pub input_modalities: u8,
    pub output_modalities: u8,
    pub context_window: u32,
    pub max_input: u32,
    pub max_output: u32,
}

impl Offer {
    pub fn validate(&self) -> ProgramResult {
        crate::helpers::require(
            self.input_rate > 0
                && self.output_rate > 0
                && self.cached_rate <= self.input_rate
                && self.speed > 0
                && self.capacity > 0
                && self.checkpoint != [0; 32]
                && valid_modalities(self.input_modalities)
                && valid_modalities(self.output_modalities)
                && self.max_input > 0
                && self.max_output > 0
                && self.max_input <= self.context_window
                && self.max_output <= self.context_window,
            crate::RouterError::InvalidArgument,
        )
    }
}

pub(crate) fn valid_modalities(mask: u8) -> bool {
    mask != 0 && mask & !MODALITIES == 0
}

#[derive(Clone, Copy, IdlType, AnchorDeserialize, AnchorSerialize)]
pub struct ReserveRequest {
    pub model_id: Address,
    pub input_rate_cap: u32,
    pub cached_rate_cap: u32,
    pub output_rate_cap: u32,
    pub min_speed: u16,
    pub max_input: u32,
    pub max_output: u32,
    pub private: bool,
    pub spend: u64,
    pub estimated_input: u32,
    pub estimated_cached: u32,
    pub estimated_output: u32,
    pub cache_provider: Address,
    pub checkpoint: [u8; 32],
    pub input_modalities: u8,
    pub output_modalities: u8,
    pub min_context: u32,
    pub limits_configured: bool,
}

#[derive(Clone, Copy, IdlType, AnchorDeserialize, AnchorSerialize, PartialEq)]
pub struct ReceiptInput {
    pub owner: Address,
    pub user: Address,
    pub booking: Address,
    pub generation: u64,
    pub charge: u64,
    pub claim_end: i64,
}

#[derive(Clone, Copy, IdlType, AnchorDeserialize, AnchorSerialize)]
pub struct Claim {
    pub uncached: u32,
    pub cached: u32,
    pub output: u32,
    pub receipt: Option<ReceiptInput>,
}

#[derive(Clone, Copy, IdlType, AnchorDeserialize, AnchorSerialize)]
pub struct ListFees {
    pub fee_bps: u16,
    pub protocol_share: u16,
}

#[derive(Clone, Copy, IdlType, AnchorDeserialize, AnchorSerialize)]
pub struct ListInit {
    pub limit: u32,
    pub fees: ListFees,
    pub maximum_payment: u64,
}

#[derive(Clone, Copy, IdlType, AnchorDeserialize, AnchorSerialize)]
pub struct FeeConfig {
    pub bank: Address,
    pub creation_mint: Address,
    pub creation_token_program: Address,
    pub creation_net_price: u64,
    pub creation_enabled: bool,
}

#[derive(Clone, IdlType, AnchorDeserialize, AnchorSerialize)]
pub struct GroupInit {
    pub tax_bps: u16,
    pub maximum_tax: u64,
    pub metadata: TokenMetadata,
}

#[derive(Clone, IdlType, AnchorDeserialize, AnchorSerialize)]
pub struct TokenMetadata {
    pub name: String,
    pub symbol: String,
    pub uri: String,
}

impl TokenMetadata {
    pub fn validate(&self) -> ProgramResult {
        crate::helpers::require(
            (1..=32).contains(&self.name.len())
                && (1..=10).contains(&self.symbol.len())
                && (1..=URI_MAX).contains(&self.uri.len())
                && !self.name.bytes().any(|byte| byte.is_ascii_control())
                && !self.symbol.bytes().any(|byte| byte.is_ascii_control())
                && !self
                    .uri
                    .bytes()
                    .any(|byte| byte.is_ascii_control() || byte.is_ascii_whitespace()),
            crate::RouterError::InvalidArgument,
        )
    }
}

#[derive(Clone, Copy, IdlType, AnchorDeserialize, AnchorSerialize)]
pub struct ReserveConfig {
    pub route_to_reserve: bool,
    pub tax_bps: u16,
    pub maximum_tax: u64,
}

#[derive(Clone, Copy, IdlType, AnchorDeserialize, AnchorSerialize)]
pub struct Redeem {
    pub amount: u64,
    pub minimum_output: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_types::MODALITY_TEXT;

    #[test]
    fn offer_requires_explicit_checkpoint_modalities_and_consistent_limits() {
        let offer = Offer {
            slot: 0,
            input_rate: 1,
            cached_rate: 0,
            output_rate: 1,
            speed: 1,
            privacy: false,
            active: true,
            capacity: 1,
            checkpoint: [7; 32],
            input_modalities: MODALITY_TEXT,
            output_modalities: MODALITY_TEXT,
            context_window: 200,
            max_input: 200,
            max_output: 100,
        };
        // independent caps need not be attainable simultaneously within the context window.
        assert!(offer.validate().is_ok());
        let changes: &[fn(&mut Offer)] = &[
            |o| o.checkpoint = [0; 32],
            |o| o.input_modalities = 0,
            |o| o.output_modalities = 0,
            |o| o.input_modalities |= 16,
            |o| o.output_modalities |= 128,
            |o| o.context_window = 0,
            |o| o.max_input = 0,
            |o| o.max_output = 0,
            |o| o.max_input = 201,
            |o| o.max_output = 201,
        ];
        for change in changes {
            let mut invalid = offer;
            change(&mut invalid);
            assert!(invalid.validate().is_err());
        }
    }
}

#[derive(Clone, Copy, IdlType, AnchorDeserialize, AnchorSerialize)]
pub struct BookingLimits {
    pub wallet_quota_window_seconds: u32,
    pub wallet_quota: u32,
    pub wallet_concurrency: u32,
}

impl BookingLimits {
    pub fn validate(&self) -> ProgramResult {
        crate::helpers::require(
            (1..=86_400).contains(&self.wallet_quota_window_seconds),
            crate::RouterError::InvalidArgument,
        )
    }
}

#[derive(Clone, Copy, IdlType, AnchorDeserialize, AnchorSerialize)]
pub struct RequestLimits {
    pub concurrency: u32,
    pub quota: u32,
    pub quota_window_seconds: u32,
}

#[derive(Clone, Copy, IdlType, AnchorDeserialize, AnchorSerialize)]
pub struct ProviderLimits {
    pub global: RequestLimits,
    pub wallet: RequestLimits,
}
