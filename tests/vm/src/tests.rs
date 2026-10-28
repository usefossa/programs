use anchor_lang::{prelude::Address, AccountDeserialize, InstructionData, ToAccountMetas};
use litesvm::{types::TransactionResult, LiteSVM};
use litesvm_token::{CreateAssociatedTokenAccount, CreateMint, MintTo, SetAuthority};
use solana_keypair::Keypair;
use solana_loader_v3_interface::state::UpgradeableLoaderState;
use solana_message::Message;
use solana_signer::Signer;
use solana_transaction::Transaction;

fn instruction<A, D>(program_id: Address, accounts: A, data: D) -> solana_instruction::Instruction
where
    A: ToAccountMetas,
    D: InstructionData,
{
    solana_instruction::Instruction {
        program_id,
        accounts: accounts.to_account_metas(None),
        data: data.data(),
    }
}

fn send(
    svm: &mut LiteSVM,
    instructions: &[solana_instruction::Instruction],
    payer: &Keypair,
    signers: &[&Keypair],
) -> TransactionResult {
    svm.expire_blockhash();
    let mut all_signers = vec![payer];
    all_signers.extend_from_slice(signers);
    svm.send_transaction(Transaction::new(
        &all_signers,
        Message::new(instructions, Some(&payer.pubkey())),
        svm.latest_blockhash(),
    ))
}

fn program_path(name: &str) -> std::path::PathBuf {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    if name == "fossa_dex.so" {
        root.join("dex/target/deploy").join(name)
    } else {
        root.join("target/deploy").join(name)
    }
}

fn setup(program_id: Address, artifact: &str) -> (LiteSVM, Keypair) {
    let mut svm = LiteSVM::new();
    svm.add_program_from_file(program_id, program_path(artifact))
        .unwrap();
    let payer = Keypair::new();
    svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
    (svm, payer)
}

fn create_mint(svm: &mut LiteSVM, payer: &Keypair, authority: Address, decimals: u8) -> Address {
    CreateMint::new(svm, payer)
        .authority(&authority)
        .decimals(decimals)
        .send()
        .unwrap()
}

fn create_token_account(
    svm: &mut LiteSVM,
    payer: &Keypair,
    mint: Address,
    owner: Address,
) -> Address {
    CreateAssociatedTokenAccount::new(svm, payer, &mint)
        .owner(&owner)
        .send()
        .unwrap()
}

fn mint_to(svm: &mut LiteSVM, payer: &Keypair, mint: Address, destination: Address, amount: u64) {
    MintTo::new(svm, payer, &mint, &destination, amount)
        .send()
        .unwrap();
}

fn account<T: AccountDeserialize>(svm: &LiteSVM, address: Address) -> T {
    let stored = svm.get_account(&address).unwrap();
    T::try_deserialize(&mut stored.data.as_slice()).unwrap()
}

fn token_account(svm: &LiteSVM, address: Address) -> litesvm_token::spl_token::state::Account {
    litesvm_token::get_spl_account(svm, &address).unwrap()
}

#[test]
fn router_initializes_typed_market_and_rejects_bad_mint() {
    use inference_router_program::{accounts, instruction as router_instruction, state_types};

    let (mut svm, payer) = setup(inference_router_program::ID, "inference_router_program.so");
    let mint = create_mint(&mut svm, &payer, payer.pubkey(), 6);
    let market = Keypair::new();
    let initialize = instruction(
        inference_router_program::ID,
        accounts::InitializeMarket {
            admin: payer.pubkey(),
            mint,
            market: market.pubkey(),
            system_program: solana_sdk_ids::system_program::ID,
        },
        router_instruction::InitializeMarket {},
    );
    send(&mut svm, &[initialize], &payer, &[&market]).unwrap();

    let state: state_types::Market = account(&svm, market.pubkey());
    assert_eq!(state.admin, payer.pubkey());
    assert_eq!(state.mint, mint);

    let bad_mint = create_mint(&mut svm, &payer, payer.pubkey(), 5);
    let rejected_market = Keypair::new();
    let rejected = instruction(
        inference_router_program::ID,
        accounts::InitializeMarket {
            admin: payer.pubkey(),
            mint: bad_mint,
            market: rejected_market.pubkey(),
            system_program: solana_sdk_ids::system_program::ID,
        },
        router_instruction::InitializeMarket {},
    );
    assert!(send(&mut svm, &[rejected], &payer, &[&rejected_market]).is_err());
    assert!(svm.get_account(&rejected_market.pubkey()).is_none());
}

#[test]
fn router_deposit_uses_generated_accounts_and_spl_state() {
    use inference_router_program::{accounts, instruction as router_instruction, state_types};

    let (mut svm, payer) = setup(inference_router_program::ID, "inference_router_program.so");
    let mint = create_mint(&mut svm, &payer, payer.pubkey(), 6);
    let market = Keypair::new();
    send(
        &mut svm,
        &[instruction(
            inference_router_program::ID,
            accounts::InitializeMarket {
                admin: payer.pubkey(),
                mint,
                market: market.pubkey(),
                system_program: solana_sdk_ids::system_program::ID,
            },
            router_instruction::InitializeMarket {},
        )],
        &payer,
        &[&market],
    )
    .unwrap();

    let user = Keypair::new();
    let authority = Address::find_program_address(
        &[b"vault", user.pubkey().as_ref()],
        &inference_router_program::ID,
    )
    .0;
    let vault = create_token_account(&mut svm, &payer, mint, authority);
    send(
        &mut svm,
        &[instruction(
            inference_router_program::ID,
            accounts::InitializeUser {
                owner: payer.pubkey(),
                market: market.pubkey(),
                user: user.pubkey(),
                authority,
                vault,
                system_program: solana_sdk_ids::system_program::ID,
            },
            router_instruction::InitializeUser {},
        )],
        &payer,
        &[&user],
    )
    .unwrap();

    let source = create_token_account(&mut svm, &payer, mint, payer.pubkey());
    mint_to(&mut svm, &payer, mint, source, 1_000);
    let deposit_accounts = accounts::Deposit {
        market: market.pubkey(),
        user: user.pubkey(),
        owner: payer.pubkey(),
        source,
        authority,
        vault,
        mint,
        token: anchor_spl::token::ID,
    };
    send(
        &mut svm,
        &[instruction(
            inference_router_program::ID,
            deposit_accounts,
            router_instruction::Deposit { amount: 400 },
        )],
        &payer,
        &[],
    )
    .unwrap();

    let user_state: state_types::User = account(&svm, user.pubkey());
    assert_eq!(user_state.free.get(), 400);
    assert_eq!(token_account(&svm, source).amount, 600);
    assert_eq!(token_account(&svm, vault).amount, 400);

    let wrong_authority = Keypair::new();
    svm.airdrop(&wrong_authority.pubkey(), 1_000_000).unwrap();
    let rejected = instruction(
        inference_router_program::ID,
        accounts::Deposit {
            market: market.pubkey(),
            user: user.pubkey(),
            owner: payer.pubkey(),
            source,
            authority: wrong_authority.pubkey(),
            vault,
            mint,
            token: anchor_spl::token::ID,
        },
        router_instruction::Deposit { amount: 1 },
    );
    assert!(send(&mut svm, &[rejected], &payer, &[]).is_err());
}

fn set_upgrade_authority(svm: &mut LiteSVM, program: Address, authority: Address) -> Address {
    let program_data = Address::find_program_address(
        &[program.as_ref()],
        &solana_sdk_ids::bpf_loader_upgradeable::ID,
    )
    .0;
    let mut stored = svm.get_account(&program_data).unwrap();
    bincode::serialize_into(
        stored.data.as_mut_slice(),
        &UpgradeableLoaderState::ProgramData {
            slot: 0,
            upgrade_authority_address: Some(authority),
        },
    )
    .unwrap();
    svm.set_account(program_data, stored).unwrap();
    program_data
}

struct DexFixture {
    svm: LiteSVM,
    payer: Keypair,
    global: Address,
    config: Address,
    quote: Address,
    treasury: Address,
    distributor: Address,
}

fn setup_dex() -> DexFixture {
    use fossa_dex::{accounts, args, instruction as dex_instruction};

    let (mut svm, payer) = setup(fossa_dex::ID, "fossa_dex.so");
    let program_data = set_upgrade_authority(&mut svm, fossa_dex::ID, payer.pubkey());
    let global = Address::find_program_address(&[b"protocol"], &fossa_dex::ID).0;
    let treasury = Keypair::new().pubkey();
    let distributor = Keypair::new().pubkey();
    send(
        &mut svm,
        &[instruction(
            fossa_dex::ID,
            accounts::InitializeGlobal {
                admin: payer.pubkey(),
                global,
                executable: fossa_dex::ID,
                program_data,
                system_program: solana_sdk_ids::system_program::ID,
            },
            dex_instruction::InitializeGlobal {
                config: args::GlobalConfig {
                    treasury,
                    distributor,
                },
            },
        )],
        &payer,
        &[],
    )
    .unwrap();

    let quote = create_mint(&mut svm, &payer, payer.pubkey(), 6);
    let config = Address::find_program_address(&[b"config", quote.as_ref()], &fossa_dex::ID).0;
    send(
        &mut svm,
        &[instruction(
            fossa_dex::ID,
            accounts::InitializeConfig {
                global,
                admin: payer.pubkey(),
                token: anchor_spl::token::ID,
                mint: quote,
                config,
                system_program: solana_sdk_ids::system_program::ID,
            },
            dex_instruction::InitializeConfig {
                config: args::ConfigInit {
                    treasury,
                    rates: args::Rates {
                        trade_bps: 125,
                        protocol_share: 10_000,
                        creator_share: 0,
                    },
                    distributor: Some(distributor),
                },
            },
        )],
        &payer,
        &[],
    )
    .unwrap();

    DexFixture {
        svm,
        payer,
        global,
        config,
        quote,
        treasury,
        distributor,
    }
}

#[test]
fn dex_bootstrap_and_config_use_generated_anchor_clients() {
    use fossa_dex::{accounts, instruction as dex_instruction, state_types};

    let DexFixture {
        mut svm,
        payer,
        global,
        config,
        quote,
        treasury,
        distributor,
    } = setup_dex();

    let global_state: state_types::Global = account(&svm, global);
    assert_eq!(global_state.admin, payer.pubkey());
    assert_eq!(global_state.treasury, treasury);
    assert_eq!(global_state.distributor, distributor);

    let config_state: state_types::Config = account(&svm, config);
    assert_eq!(config_state.admin, payer.pubkey());
    assert_eq!(config_state.quote_mint, quote);
    assert_eq!(config_state.enabled, 1);

    let unauthorized = Keypair::new();
    svm.airdrop(&unauthorized.pubkey(), 1_000_000).unwrap();
    let rejected = instruction(
        fossa_dex::ID,
        accounts::UpdateConfig {
            global,
            admin: unauthorized.pubkey(),
            token: anchor_spl::token::ID,
            mint: quote,
            config,
        },
        dex_instruction::UpdateConfig { enabled: false },
    );
    assert!(send(&mut svm, &[rejected], &payer, &[&unauthorized]).is_err());
}

#[test]
fn dex_pool_and_swap_use_typed_anchor_accounts() {
    use fossa_dex::{accounts, args, instruction as dex_instruction, state_types};

    let DexFixture {
        mut svm,
        payer,
        config,
        quote,
        ..
    } = setup_dex();
    let mint_a = create_mint(&mut svm, &payer, payer.pubkey(), 6);
    let source_a = create_token_account(&mut svm, &payer, mint_a, payer.pubkey());
    let source_b = create_token_account(&mut svm, &payer, quote, payer.pubkey());
    mint_to(&mut svm, &payer, mint_a, source_a, 5_000_000);
    mint_to(&mut svm, &payer, quote, source_b, 10_000_000);
    SetAuthority::new(
        &mut svm,
        &payer,
        &mint_a,
        litesvm_token::spl_token::instruction::AuthorityType::MintTokens,
    )
    .send()
    .unwrap();

    let pool = Address::find_program_address(&[b"pool", mint_a.as_ref()], &fossa_dex::ID).0;
    let authority = Address::find_program_address(&[b"authority", pool.as_ref()], &fossa_dex::ID).0;
    let lp_mint = Address::find_program_address(&[b"lp_mint", pool.as_ref()], &fossa_dex::ID).0;
    let vault_a = create_token_account(&mut svm, &payer, mint_a, authority);
    let vault_b = create_token_account(&mut svm, &payer, quote, authority);
    let user_lp = Keypair::new();
    send(
        &mut svm,
        &[instruction(
            fossa_dex::ID,
            accounts::InitializePool {
                launch: fossa_dex::contexts::__client_accounts_launch::Launch {
                    payer: payer.pubkey(),
                    config,
                    token_a: anchor_spl::token::ID,
                    token_b: anchor_spl::token::ID,
                    token_program: anchor_spl::token::ID,
                    mint_a,
                    mint_b: quote,
                    pool,
                    authority,
                    source_a,
                    source_b,
                    vault_a,
                    vault_b,
                    lp_mint,
                    user_lp: user_lp.pubkey(),
                    system_program: solana_sdk_ids::system_program::ID,
                },
            },
            dex_instruction::InitializePool {
                config: args::PoolInit {
                    amount_a: 4_000_000,
                    amount_b: 9_000_000,
                    minimum_lp: 5_999_000,
                },
            },
        )],
        &payer,
        &[&user_lp],
    )
    .unwrap();

    let initialized: state_types::Pool = account(&svm, pool);
    assert_eq!(initialized.mint_a, mint_a);
    assert_eq!(initialized.mint_b, quote);
    assert_eq!(initialized.reserve_a.get(), 4_000_000);
    assert_eq!(initialized.reserve_b.get(), 9_000_000);
    assert_eq!(token_account(&svm, user_lp.pubkey()).amount, 5_999_000);

    let before_b = token_account(&svm, source_b).amount;
    send(
        &mut svm,
        &[instruction(
            fossa_dex::ID,
            accounts::Swap {
                trade: fossa_dex::contexts::__client_accounts_trade::Trade {
                    user: payer.pubkey(),
                    pool,
                    authority,
                    token_a: anchor_spl::token::ID,
                    token_b: anchor_spl::token::ID,
                    mint_a,
                    mint_b: quote,
                    user_a: source_a,
                    user_b: source_b,
                    vault_a,
                    vault_b,
                },
            },
            dex_instruction::Swap {
                args: args::SwapArgs {
                    a_to_b: true,
                    amount: 100_000,
                    limit: 1,
                },
            },
        )],
        &payer,
        &[],
    )
    .unwrap();

    let swapped: state_types::Pool = account(&svm, pool);
    assert!(swapped.reserve_a.get() > initialized.reserve_a.get());
    assert!(swapped.reserve_b.get() < initialized.reserve_b.get());
    assert!(token_account(&svm, source_b).amount > before_b);
}
