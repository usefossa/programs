use anchor_lang::{prelude::Address, Discriminator, InstructionData};
use inference_router_program::{args, instruction, state_types};

fn main() {
    let offer = args::Offer {
        slot: 0,
        input_rate: 1_000,
        cached_rate: 100,
        output_rate: 2_000,
        speed: 32,
        privacy: true,
        active: true,
        capacity: 10,
        checkpoint: [7; 32],
        input_modalities: 1,
        output_modalities: 1,
        context_window: 1_000_000,
        max_input: 1_000_000,
        max_output: 1_000_000,
    };
    let request = args::ReserveRequest {
        model_id: Address::new_from_array([6; 32]),
        input_rate_cap: 10_000,
        cached_rate_cap: 1_000,
        output_rate_cap: 20_000,
        min_speed: 32,
        max_input: 100,
        max_output: 200,
        private: true,
        spend: 50_000,
        estimated_input: 10,
        estimated_cached: 3,
        estimated_output: 20,
        cache_provider: Address::new_from_array([10; 32]),
        checkpoint: [7; 32],
        input_modalities: 1,
        output_modalities: 1,
        min_context: 0,
        limits_configured: false,
    };
    println!(
        "{{\"register_offer\":{:?},\"update_offer\":{:?},\"reserve_booking\":{:?},\"terms_discriminator\":{:?},\"booking_discriminator\":{:?},\"terms_entry_bytes\":{},\"booking_bytes\":{}}}",
        instruction::RegisterOffer { offer }.data(),
        instruction::UpdateOffer { offer }.data(),
        instruction::ReserveBooking {
            request,
            generation: Some(72_057_594_037_927_943),
        }.data(),
        state_types::Terms::DISCRIMINATOR,
        state_types::Booking::DISCRIMINATOR,
        std::mem::size_of::<state_types::TermsEntry>(),
        state_types::Booking::DISCRIMINATOR.len() + std::mem::size_of::<state_types::Booking>(),
    );
}
