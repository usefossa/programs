use super::*;
use anchor_lang::bytemuck::Zeroable;
use inference_router_program::{accounts, args, instruction as ix, state_types};

fn initialize_market(svm: &mut LiteSVM, payer: &Keypair, mint: Address) -> Address {
    let market = Keypair::new();
    send(
        svm,
        &[instruction(
            inference_router_program::ID,
            accounts::InitializeMarket {
                council: payer.pubkey(),
                mint,
                market: market.pubkey(),
                system_program: solana_sdk_ids::system_program::ID,
            },
            ix::InitializeMarket {},
        )],
        payer,
        &[&market],
    )
    .unwrap();
    market.pubkey()
}

fn fees(
    market: Address,
    council: Address,
    mint: Address,
    enabled: bool,
    price: u64,
) -> solana_instruction::Instruction {
    let mut configure = instruction(
        inference_router_program::ID,
        accounts::ConfigureFees {
            market,
            signer: council,
        },
        ix::ConfigureFees {
            config: args::FeeConfig {
                bank: council,
                creation_mint: mint,
                creation_token_program: anchor_spl::token::ID,
                creation_net_price: price,
                creation_enabled: enabled,
            },
        },
    );
    if enabled {
        configure.accounts.extend([
            solana_instruction::AccountMeta::new_readonly(mint, false),
            solana_instruction::AccountMeta::new_readonly(anchor_spl::token::ID, false),
        ]);
    }
    configure
}

fn list(
    market: Address,
    owner: Address,
    address: Address,
    bps: u16,
    payment: Option<[Address; 3]>,
) -> solana_instruction::Instruction {
    let mut create = instruction(
        inference_router_program::ID,
        accounts::InitializeList {
            market,
            authority: owner,
            list: address,
            system_program: solana_sdk_ids::system_program::ID,
        },
        ix::InitializeList {
            config: args::ListInit {
                limit: 2,
                fees: args::ListFees {
                    fee_bps: bps,
                    protocol_share: 10_000,
                },
                maximum_payment: 10,
            },
        },
    );
    if let Some([source, destination, mint]) = payment {
        create.accounts.extend([
            solana_instruction::AccountMeta::new(source, false),
            solana_instruction::AccountMeta::new(destination, false),
            solana_instruction::AccountMeta::new_readonly(mint, false),
            solana_instruction::AccountMeta::new_readonly(anchor_spl::token::ID, false),
        ]);
    }
    create
}

#[test]
fn list_creation_requires_enabled_paid_mint_and_minimum_fee() {
    let (mut svm, payer) = setup(inference_router_program::ID, "inference_router_program.so");
    let mint = create_mint(&mut svm, &payer, payer.pubkey(), 6);
    let market = initialize_market(&mut svm, &payer, mint);
    let owner = Keypair::new();
    svm.airdrop(&owner.pubkey(), 100_000_000).unwrap();
    let rejected = Keypair::new();
    assert!(send(
        &mut svm,
        &[list(market, owner.pubkey(), rejected.pubkey(), 10, None)],
        &payer,
        &[&owner, &rejected]
    )
    .is_err());
    for price in [0, 1] {
        let mut configure = fees(market, payer.pubkey(), mint, true, price);
        if price == 1 {
            configure.accounts[2].pubkey = owner.pubkey();
        }
        assert!(send(&mut svm, &[configure], &payer, &[]).is_err());
    }
    send(
        &mut svm,
        &[fees(market, payer.pubkey(), mint, true, 10)],
        &payer,
        &[],
    )
    .unwrap();
    let source = create_token_account(&mut svm, &payer, mint, owner.pubkey());
    let destination = create_token_account(&mut svm, &payer, mint, payer.pubkey());
    mint_to(&mut svm, &payer, mint, source, 100);
    for bps in [0, 9, 301] {
        let rejected = Keypair::new();
        assert!(send(
            &mut svm,
            &[list(
                market,
                owner.pubkey(),
                rejected.pubkey(),
                bps,
                Some([source, destination, mint])
            )],
            &payer,
            &[&owner, &rejected]
        )
        .is_err());
        assert_eq!(token_account(&svm, source).amount, 100);
    }
    let accepted = Keypair::new();
    send(
        &mut svm,
        &[list(
            market,
            owner.pubkey(),
            accepted.pubkey(),
            10,
            Some([source, destination, mint]),
        )],
        &payer,
        &[&owner, &accepted],
    )
    .unwrap();
    assert_eq!(token_account(&svm, destination).amount, 10);
    send(
        &mut svm,
        &[fees(market, payer.pubkey(), mint, false, 10)],
        &payer,
        &[],
    )
    .unwrap();
    let disabled = Keypair::new();
    assert!(send(
        &mut svm,
        &[list(market, owner.pubkey(), disabled.pubkey(), 10, None)],
        &payer,
        &[&owner, &disabled]
    )
    .is_err());
}

struct GroupFixture {
    f: DexFixture,
    market: Address,
    group: Address,
    mint: Address,
    backing: Address,
    council_tokens: Address,
    trade: fossa_dex::contexts::__client_accounts_trade::Trade,
}

fn uninitialized_token(svm: &mut LiteSVM) -> Address {
    use anchor_spl::token_interface::spl_token_2022::{extension::ExtensionType, state::Account};
    let len =
        ExtensionType::try_calculate_account_len::<Account>(&[ExtensionType::TransferFeeAmount])
            .unwrap();
    let address = Keypair::new().pubkey();
    svm.set_account(
        address,
        solana_account::Account {
            lamports: svm.minimum_balance_for_rent_exemption(len),
            data: vec![0; len],
            owner: anchor_spl::token_2022::ID,
            executable: false,
            rent_epoch: 0,
        },
    )
    .unwrap();
    address
}

fn setup_group(tax_bps: u16) -> GroupFixture {
    use anchor_spl::token_interface::spl_token_2022::{
        extension::{
            transfer_fee::TransferFeeAmount, BaseStateWithExtensions, PodStateWithExtensions,
        },
        pod::{PodAccount, PodMint},
    };
    let mut f = setup_dex();
    f.svm
        .add_program_from_file(
            inference_router_program::ID,
            program_path("inference_router_program.so"),
        )
        .unwrap();
    let market = initialize_market(&mut f.svm, &f.payer, f.quote);
    send(
        &mut f.svm,
        &[instruction(
            inference_router_program::ID,
            accounts::ConfigureDex {
                market,
                council: f.payer.pubkey(),
                dex: fossa_dex::ID,
                config: f.config,
                global: f.global,
            },
            ix::ConfigureDex {},
        )],
        &f.payer,
        &[],
    )
    .unwrap();
    let group_id = Keypair::new().pubkey();
    let group = Address::find_program_address(
        &[b"model_group", market.as_ref(), group_id.as_ref()],
        &inference_router_program::ID,
    )
    .0;
    let mint = Address::find_program_address(
        &[b"reserve_mint", group.as_ref()],
        &inference_router_program::ID,
    )
    .0;
    let pool = pair_address(mint, f.quote);
    let authority = Address::find_program_address(&[b"authority", pool.as_ref()], &fossa_dex::ID).0;
    let lp_mint = Address::find_program_address(&[b"lp_mint", pool.as_ref()], &fossa_dex::ID).0;
    let backing = Keypair::new();
    let council_tokens = uninitialized_token(&mut f.svm);
    let base_vault = uninitialized_token(&mut f.svm);
    let quote_source = create_token_account(&mut f.svm, &f.payer, f.quote, f.payer.pubkey());
    let quote_vault = create_token_account(&mut f.svm, &f.payer, f.quote, authority);
    let user_lp = Keypair::new();
    let before = f.svm.get_balance(&f.treasury).unwrap();
    let create = instruction(
        inference_router_program::ID,
        accounts::InitializeModelGroup {
            market,
            council: f.payer.pubkey(),
            payer: f.payer.pubkey(),
            group,
            mint,
            backing: backing.pubkey(),
            council_tokens,
            quote: f.quote,
            token_program: anchor_spl::token::ID,
            t22: anchor_spl::token_2022::ID,
            dex: fossa_dex::ID,
            curve_config: f.config,
            curve_pool: pool,
            curve_authority: authority,
            curve_quote_source: quote_source,
            curve_base_vault: base_vault,
            curve_quote_vault: quote_vault,
            curve_lp_mint: lp_mint,
            curve_lp: user_lp.pubkey(),
            dex_global: f.global,
            dex_treasury: f.treasury,
            system_program: solana_sdk_ids::system_program::ID,
        },
        ix::InitializeModelGroup {
            group_id,
            config: args::GroupInit {
                tax_bps,
                maximum_tax: u64::MAX,
                metadata: args::TokenMetadata {
                    name: "Model Group".into(),
                    symbol: "MODEL".into(),
                    uri: "https://example.com/model.json".into(),
                },
            },
        },
    );
    send(&mut f.svm, &[create], &f.payer, &[&backing, &user_lp]).unwrap();
    assert_eq!(f.svm.get_balance(&f.treasury).unwrap() - before, 25_000_000);
    assert_eq!(token_account(&f.svm, base_vault).amount, 90_000_000_000_000);
    assert_eq!(
        token_account(&f.svm, council_tokens).amount,
        10_000_000_000_000
    );
    assert_eq!(
        token_account(&f.svm, council_tokens).owner,
        f.payer.pubkey()
    );
    let mint_account = f.svm.get_account(&mint).unwrap();
    let minted = PodStateWithExtensions::<PodMint>::unpack(&mint_account.data).unwrap();
    assert_eq!(u64::from(minted.base.supply), 100_000_000_000_000);
    assert_eq!(minted.base.decimals, 6);
    for address in [base_vault, council_tokens] {
        let stored = f.svm.get_account(&address).unwrap();
        let state = PodStateWithExtensions::<PodAccount>::unpack(&stored.data).unwrap();
        assert_eq!(
            u64::from(
                state
                    .get_extension::<TransferFeeAmount>()
                    .unwrap()
                    .withheld_amount
            ),
            0
        );
    }
    let state: state_types::ModelGroup = account(&f.svm, group);
    assert_eq!(
        (state.fixed_token_mint, state.backing_vault),
        (mint, backing.pubkey())
    );
    let trade = fossa_dex::contexts::__client_accounts_trade::Trade {
        user: f.payer.pubkey(),
        pool,
        authority,
        token_a: anchor_spl::token_2022::ID,
        token_b: anchor_spl::token::ID,
        mint_a: mint,
        mint_b: f.quote,
        user_a: council_tokens,
        user_b: quote_source,
        vault_a: base_vault,
        vault_b: quote_vault,
    };
    GroupFixture {
        f,
        market,
        group,
        mint,
        backing: backing.pubkey(),
        council_tokens,
        trade,
    }
}

fn metadata(
    svm: &LiteSVM,
    mint: Address,
) -> anchor_spl::token_2022_extensions::spl_token_metadata_interface::state::TokenMetadata {
    use anchor_spl::token_interface::spl_token_2022::{
        extension::{BaseStateWithExtensions, PodStateWithExtensions},
        pod::PodMint,
    };
    let stored = svm.get_account(&mint).unwrap();
    PodStateWithExtensions::<PodMint>::unpack(&stored.data)
        .unwrap()
        .get_variable_len_extension()
        .unwrap()
}

#[test]
fn group_activation_metadata_authority_and_redemption_fee_are_atomic() {
    let GroupFixture {
        mut f,
        market,
        group,
        mint,
        backing,
        council_tokens,
        trade,
    } = setup_group(0);
    mint_to(&mut f.svm, &f.payer, f.quote, trade.user_b, 100_000_000_000);
    let buy = instruction(
        fossa_dex::ID,
        fossa_dex::accounts::SwapExactOut { trade },
        fossa_dex::instruction::SwapExactOut {
            args: fossa_dex::args::SwapArgs {
                a_to_b: false,
                amount: 10_000_000_000_000,
                limit: u64::MAX,
            },
        },
    );
    send(&mut f.svm, &[buy], &f.payer, &[]).unwrap();
    assert_eq!(
        token_account(&f.svm, council_tokens).amount,
        20_000_000_000_000
    );
    assert_eq!(metadata(&f.svm, mint).name, "Model Group");
    let holder = Keypair::new();
    f.svm.airdrop(&holder.pubkey(), 24_999_999).unwrap();
    let update = |council| {
        instruction(
            inference_router_program::ID,
            accounts::UpdateGroupMetadata {
                market,
                council,
                payer: f.payer.pubkey(),
                group,
                mint,
                t22: anchor_spl::token_2022::ID,
                system_program: solana_sdk_ids::system_program::ID,
            },
            ix::UpdateGroupMetadata {
                metadata: args::TokenMetadata {
                    name: "Updated Group".into(),
                    symbol: "GROUP".into(),
                    uri: "https://example.com/updated-group.json".into(),
                },
            },
        )
    };
    let unauthorized = update(holder.pubkey());
    assert!(send(&mut f.svm, &[unauthorized], &f.payer, &[&holder]).is_err());
    let authorized = update(f.payer.pubkey());
    send(&mut f.svm, &[authorized], &f.payer, &[]).unwrap();
    assert_eq!(metadata(&f.svm, mint).name, "Updated Group");
    let source = CreateAssociatedTokenAccount::new(&mut f.svm, &f.payer, &mint)
        .owner(&holder.pubkey())
        .token_program_id(&anchor_spl::token_2022::ID)
        .send()
        .unwrap();
    let destination = create_token_account(&mut f.svm, &f.payer, f.quote, holder.pubkey());
    let transfer = anchor_spl::token_interface::spl_token_2022::instruction::transfer_checked(
        &anchor_spl::token_2022::ID,
        &council_tokens,
        &mint,
        &source,
        &f.payer.pubkey(),
        &[],
        5_000_000_000_000,
        6,
    )
    .unwrap();
    send(&mut f.svm, &[transfer], &f.payer, &[]).unwrap();
    mint_to(&mut f.svm, &f.payer, f.quote, backing, 1_000_000);
    let redeem = instruction(
        inference_router_program::ID,
        accounts::RedeemReserve {
            market,
            group,
            mint,
            source,
            holder: holder.pubkey(),
            backing,
            destination,
            quote: f.quote,
            council: f.payer.pubkey(),
            token: anchor_spl::token::ID,
            t22: anchor_spl::token_2022::ID,
            system_program: solana_sdk_ids::system_program::ID,
        },
        ix::RedeemReserve {
            redeem: args::Redeem {
                amount: 5_000_000_000_000,
                minimum_output: 50_000,
            },
        },
    );
    let mint_before = f.svm.get_account(&mint).unwrap().data;
    assert!(send(&mut f.svm, &[redeem.clone()], &f.payer, &[&holder]).is_err());
    assert_eq!(f.svm.get_account(&mint).unwrap().data, mint_before);
    assert_eq!(token_account(&f.svm, source).amount, 5_000_000_000_000);
    assert_eq!(token_account(&f.svm, backing).amount, 1_000_000);
    f.svm.airdrop(&holder.pubkey(), 1_000_000).unwrap();
    let before = f.svm.get_balance(&holder.pubkey()).unwrap();
    send(&mut f.svm, &[redeem], &f.payer, &[&holder]).unwrap();
    assert_eq!(
        before - f.svm.get_balance(&holder.pubkey()).unwrap(),
        25_000_000
    );
    assert_eq!(token_account(&f.svm, source).amount, 0);
    assert_eq!(token_account(&f.svm, destination).amount, 50_000);

    let destination = create_token_account(&mut f.svm, &f.payer, f.quote, f.payer.pubkey());
    let destination_before = token_account(&f.svm, destination).amount;
    let self_redeem = instruction(
        inference_router_program::ID,
        accounts::RedeemReserve {
            market,
            group,
            mint,
            source: council_tokens,
            holder: f.payer.pubkey(),
            backing,
            destination,
            quote: f.quote,
            council: f.payer.pubkey(),
            token: anchor_spl::token::ID,
            t22: anchor_spl::token_2022::ID,
            system_program: solana_sdk_ids::system_program::ID,
        },
        ix::RedeemReserve {
            redeem: args::Redeem {
                amount: 5_000_000_000_000,
                minimum_output: 50_000,
            },
        },
    );
    let before = f.svm.get_balance(&f.payer.pubkey()).unwrap();
    send(&mut f.svm, &[self_redeem], &holder, &[&f.payer]).unwrap();
    assert_eq!(f.svm.get_balance(&f.payer.pubkey()).unwrap(), before);
    assert_eq!(
        token_account(&f.svm, council_tokens).amount,
        10_000_000_000_000
    );
    assert_eq!(
        token_account(&f.svm, destination).amount - destination_before,
        50_000
    );
    assert_eq!(token_account(&f.svm, backing).amount, 900_000);
}

#[test]
fn two_models_share_group_backing_and_claim_retains_assignment_snapshot() {
    let GroupFixture {
        mut f,
        market,
        group,
        backing,
        ..
    } = setup_group(100);
    let provider = f.payer.pubkey();
    let profile = Address::find_program_address(
        &[b"provider", market.as_ref(), provider.as_ref()],
        &inference_router_program::ID,
    )
    .0;
    send(
        &mut f.svm,
        &[instruction(
            inference_router_program::ID,
            accounts::InitializeProvider {
                market,
                provider,
                payer: provider,
                profile,
                system_program: solana_sdk_ids::system_program::ID,
            },
            ix::InitializeProvider {
                metadata: args::Metadata {
                    document_hash: [1; 32],
                    uri: "https://example.com/provider.json".into(),
                },
            },
        )],
        &f.payer,
        &[],
    )
    .unwrap();
    let payout = create_token_account(&mut f.svm, &f.payer, f.quote, provider);
    let mut models = Vec::new();
    let mut offers = Vec::new();
    for _ in 0..2 {
        let model_id = Keypair::new().pubkey();
        let model = Address::find_program_address(
            &[b"model", market.as_ref(), model_id.as_ref()],
            &inference_router_program::ID,
        )
        .0;
        let model_metadata = Address::find_program_address(
            &[b"model_metadata", model.as_ref()],
            &inference_router_program::ID,
        )
        .0;
        let terms = Keypair::new();
        let capacity = Keypair::new();
        let create = instruction(
            inference_router_program::ID,
            accounts::InitializeModel {
                market,
                provider,
                payer: provider,
                profile,
                model,
                terms: terms.pubkey(),
                capacity: capacity.pubkey(),
                metadata: model_metadata,
                system_program: solana_sdk_ids::system_program::ID,
            },
            ix::InitializeModel {
                model_id,
                limit: 1,
                metadata: args::Metadata {
                    document_hash: [2; 32],
                    uri: "https://example.com/inference.json".into(),
                },
            },
        );
        let assign = instruction(
            inference_router_program::ID,
            accounts::AssignModelGroup {
                market,
                council: provider,
                model,
                group: Some(group),
            },
            ix::AssignModelGroup {},
        );
        let offer = instruction(
            inference_router_program::ID,
            accounts::RegisterOffer {
                market,
                model,
                terms: terms.pubkey(),
                capacity: capacity.pubkey(),
                profile,
                provider,
                payout,
            },
            ix::RegisterOffer {
                offer: args::Offer {
                    slot: 0,
                    input_rate: 1_000_000,
                    cached_rate: 1_000_000,
                    output_rate: 1_000_000,
                    speed: 100,
                    privacy: false,
                    active: true,
                    capacity: 10,
                },
            },
        );
        send(
            &mut f.svm,
            &[create, assign],
            &f.payer,
            &[&terms, &capacity],
        )
        .unwrap();
        assert_eq!(account::<state_types::Model>(&f.svm, model).group, group);
        models.push((model_id, model, terms.pubkey(), capacity.pubkey()));
        offers.push(offer);
    }
    let books: Vec<_> = models
        .iter()
        .flat_map(|(_, _, terms, capacity)| [*terms, *capacity])
        .map(|address| (address, f.svm.get_account(&address).unwrap().data))
        .collect();
    for indices in [&[2][..], &[3][..], &[2, 3][..]] {
        let mut mismatched = offers[0].clone();
        for &index in indices {
            mismatched.accounts[index] = offers[1].accounts[index].clone();
        }
        assert!(send(&mut f.svm, &[mismatched], &f.payer, &[]).is_err());
        for (address, data) in &books {
            assert_eq!(&f.svm.get_account(address).unwrap().data, data);
        }
    }
    for offer in offers {
        send(&mut f.svm, &[offer], &f.payer, &[]).unwrap();
    }
    send(
        &mut f.svm,
        &[fees(market, provider, f.quote, true, 1)],
        &f.payer,
        &[],
    )
    .unwrap();
    let payment = litesvm_token::CreateAccount::new(&mut f.svm, &f.payer, &f.quote)
        .send()
        .unwrap();
    mint_to(&mut f.svm, &f.payer, f.quote, payment, 1);
    let provider_list = Keypair::new();
    let create = list(
        market,
        provider,
        provider_list.pubkey(),
        100,
        Some([payment, payout, f.quote]),
    );
    let enable = instruction(
        inference_router_program::ID,
        accounts::UpdateList {
            market,
            list: provider_list.pubkey(),
            authority: provider,
        },
        ix::UpdateList {
            provider,
            enabled: true,
        },
    );
    send(&mut f.svm, &[create, enable], &f.payer, &[&provider_list]).unwrap();
    let user = Keypair::new().pubkey();
    let (authority, bump) =
        Address::find_program_address(&[b"vault", user.as_ref()], &inference_router_program::ID);
    let vault = create_token_account(&mut f.svm, &f.payer, f.quote, authority);
    mint_to(&mut f.svm, &f.payer, f.quote, vault, 100_000);
    let mut user_state = state_types::User::zeroed();
    user_state.owner = provider;
    user_state.market = market;
    user_state.vault = vault;
    user_state.free = 100_000.into();
    user_state.bump = bump;
    store_account(&mut f.svm, user, &user_state);
    for (model_id, model, terms, capacity) in models {
        let booking = Keypair::new().pubkey();
        let mut empty = state_types::Booking::zeroed();
        empty.owner = provider;
        empty.market = market;
        empty.user = user;
        empty.rent_payer = provider;
        empty.status = 5;
        store_account(&mut f.svm, booking, &empty);
        let reserve = instruction(
            inference_router_program::ID,
            accounts::ReserveBooking {
                market,
                model,
                terms,
                capacity,
                user,
                owner: provider,
                booking,
                rent_payer: provider,
                list: provider_list.pubkey(),
                group: Some(group),
            },
            ix::ReserveBooking {
                request: args::ReserveRequest {
                    model_id,
                    input_rate_cap: 1_000_000,
                    cached_rate_cap: 1_000_000,
                    output_rate_cap: 1_000_000,
                    min_speed: 1,
                    max_input: 100,
                    max_output: 100,
                    private: false,
                    spend: 1_000,
                    estimated_input: 100,
                    estimated_cached: 0,
                    estimated_output: 100,
                    cache_provider: Address::default(),
                },
                generation: None,
            },
        );
        send(&mut f.svm, &[reserve], &f.payer, &[]).unwrap();
        let booked: state_types::Booking = account(&f.svm, booking);
        assert_eq!(
            (booked.protocol_destination, booked.reserve_authority),
            (backing, group)
        );
        let detach = instruction(
            inference_router_program::ID,
            accounts::AssignModelGroup {
                market,
                council: provider,
                model,
                group: None,
            },
            ix::AssignModelGroup {},
        );
        send(&mut f.svm, &[detach], &f.payer, &[]).unwrap();
        let mut claim = instruction(
            inference_router_program::ID,
            accounts::ClaimBooking {
                market,
                model,
                capacity,
                user,
                booking,
                provider,
                vault,
                payout,
                mint: f.quote,
                authority,
                token: anchor_spl::token::ID,
            },
            ix::ClaimBooking {
                claim: args::Claim {
                    uncached: 100,
                    cached: 0,
                    output: 100,
                    receipt: None,
                },
            },
        );
        claim.accounts.extend([
            solana_instruction::AccountMeta::new(payout, false),
            solana_instruction::AccountMeta::new(backing, false),
        ]);
        send(&mut f.svm, &[claim], &f.payer, &[]).unwrap();
    }
    assert_eq!(token_account(&f.svm, backing).amount, 4);
}
