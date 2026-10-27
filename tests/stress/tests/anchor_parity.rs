use litesvm::LiteSVM;
use solana_account::Account;
use solana_address::{address, Address};
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_message::Message;
use solana_signer::Signer;
use solana_transaction::Transaction;

const ROUTER: Address = Address::new_from_array([8; 32]);
const DEX: Address = Address::new_from_array([9; 32]);
const TOKEN: Address = address!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const T22: Address = address!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
const PROBE: Address = Address::new_from_array([10; 32]);

fn meta(key: Address, writable: bool, signer: bool) -> AccountMeta {
    AccountMeta {
        pubkey: key,
        is_writable: writable,
        is_signer: signer,
    }
}
fn key() -> Address {
    Keypair::new().pubkey()
}
fn mint() -> Vec<u8> {
    let mut data = vec![0; 82];
    data[44] = 6;
    data[45] = 1;
    data
}
fn market(admin: Address, mint: Address) -> Vec<u8> {
    let mut data = b"FOSSAV20".to_vec();
    data.extend(admin.to_bytes());
    data.extend(mint.to_bytes());
    data
}

fn compare(
    program: Address,
    data: Vec<u8>,
    accounts: Vec<AccountMeta>,
    signers: &[&Keypair],
    fixtures: Vec<(Address, Address, Vec<u8>)>,
) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let old = std::path::PathBuf::from(std::env::var("FOSSA_PARITY_ORIGINAL").unwrap());
    let artifact = if program == ROUTER {
        "target/deploy/inference_router_program.so"
    } else {
        "dex/target/deploy/fossa_dex.so"
    };
    let payer = Keypair::new();
    let mut outcomes = Vec::new();
    let mut logs = Vec::new();
    let mut states = Vec::new();
    for (label, base) in [("original", old), ("anchor", root)] {
        let mut vm = LiteSVM::new();
        vm.add_program_from_file(program, base.join(artifact))
            .unwrap();
        vm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
        for (address, owner, bytes) in &fixtures {
            vm.set_account(
                *address,
                Account {
                    lamports: vm.minimum_balance_for_rent_exemption(bytes.len()),
                    owner: *owner,
                    data: bytes.clone(),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        let mut keys = vec![&payer];
        keys.extend_from_slice(signers);
        let tx = Transaction::new(
            &keys,
            Message::new(
                &[Instruction {
                    program_id: program,
                    accounts: accounts.clone(),
                    data: data.clone(),
                }],
                Some(&payer.pubkey()),
            ),
            vm.latest_blockhash(),
        );
        let result = vm.send_transaction(tx);
        println!(
            "{label}: {:?}",
            result.as_ref().map(|m| m.compute_units_consumed)
        );
        logs.push(
            result
                .as_ref()
                .map(|m| {
                    m.logs
                        .iter()
                        .filter(|l| l.starts_with("Program data: "))
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
        );
        outcomes.push(result.is_ok());
        states.push(
            accounts
                .iter()
                .map(|a| vm.get_account(&a.pubkey))
                .chain([vm.get_account(&payer.pubkey())])
                .collect::<Vec<_>>(),
        );
    }
    assert!(outcomes[0], "probe must succeed against original");
    assert_eq!(outcomes[0], outcomes[1], "accepted instruction parity");
    assert_eq!(
        logs[0], logs[1],
        "data log and forwarded CPI privilege parity"
    );
    assert_eq!(
        states[0], states[1],
        "business account and payer state parity"
    );
}

#[test]
#[ignore = "requires FOSSA_PARITY_ORIGINAL pointing to freshly built original programs"]
fn router_market_can_be_its_admin() {
    let m = Keypair::new();
    let token_mint = key();
    compare(
        ROUTER,
        vec![0],
        vec![
            meta(m.pubkey(), true, true),
            meta(m.pubkey(), false, true),
            meta(token_mint, false, false),
        ],
        &[&m],
        vec![
            (m.pubkey(), ROUTER, vec![0; 72]),
            (token_mint, TOKEN, mint()),
        ],
    );
}

#[test]
#[ignore = "requires FOSSA_PARITY_ORIGINAL pointing to freshly built original programs"]
fn router_user_lane_can_be_its_owner() {
    let user = Keypair::new();
    let m = key();
    let token_mint = key();
    let vault = key();
    let authority = Address::find_program_address(&[b"vault", user.pubkey().as_ref()], &ROUTER).0;
    let mut token = vec![0; 165];
    token[..32].copy_from_slice(token_mint.as_ref());
    token[32..64].copy_from_slice(authority.as_ref());
    token[108] = 1;
    compare(
        ROUTER,
        vec![1],
        vec![
            meta(user.pubkey(), true, true),
            meta(user.pubkey(), false, true),
            meta(m, false, false),
            meta(vault, false, false),
        ],
        &[&user],
        vec![
            (user.pubkey(), ROUTER, vec![0; 120]),
            (m, ROUTER, market(key(), token_mint)),
            (vault, TOKEN, token),
        ],
    );
}

fn self_owned_user_transfer(op: u8) {
    let user = Keypair::new();
    let m = key();
    let token_mint = key();
    let vault = key();
    let external = key();
    let (authority, bump) =
        Address::find_program_address(&[b"vault", user.pubkey().as_ref()], &ROUTER);
    let mut lane = vec![0; 120];
    lane[..8].copy_from_slice(b"USER0V20");
    lane[8..40].copy_from_slice(user.pubkey().as_ref());
    lane[40..72].copy_from_slice(m.as_ref());
    lane[72..80].copy_from_slice(&500u64.to_le_bytes());
    lane[80..112].copy_from_slice(vault.as_ref());
    lane[112] = bump;
    let token = |owner: Address| {
        let mut d = vec![0; 165];
        d[..32].copy_from_slice(token_mint.as_ref());
        d[32..64].copy_from_slice(owner.as_ref());
        d[64..72].copy_from_slice(&500u64.to_le_bytes());
        d[108] = 1;
        d
    };
    let mut accounts = vec![
        meta(m, false, false),
        meta(user.pubkey(), true, false),
        meta(user.pubkey(), false, true),
    ];
    if op == 3 {
        accounts.extend([
            meta(external, true, false),
            meta(vault, true, false),
            meta(token_mint, false, false),
        ]);
    } else {
        accounts.extend([
            meta(vault, true, false),
            meta(external, true, false),
            meta(token_mint, false, false),
            meta(authority, false, false),
        ]);
    }
    accounts.push(meta(TOKEN, false, false));
    let mut payload = vec![op];
    payload.extend(100u64.to_le_bytes());
    compare(
        ROUTER,
        payload,
        accounts,
        &[&user],
        vec![
            (user.pubkey(), ROUTER, lane),
            (m, ROUTER, market(key(), token_mint)),
            (token_mint, TOKEN, mint()),
            (vault, TOKEN, token(authority)),
            (external, TOKEN, token(user.pubkey())),
        ],
    );
}

#[test]
#[ignore = "requires FOSSA_PARITY_ORIGINAL pointing to freshly built original programs"]
fn router_self_owned_user_deposit() {
    self_owned_user_transfer(3);
}

#[test]
#[ignore = "requires FOSSA_PARITY_ORIGINAL pointing to freshly built original programs"]
fn router_self_owned_user_withdraw() {
    self_owned_user_transfer(10);
}

#[test]
#[ignore = "requires FOSSA_PARITY_ORIGINAL pointing to freshly built original programs"]
fn router_list_can_be_its_authority() {
    let list = Keypair::new();
    let m = key();
    compare(
        ROUTER,
        vec![28, 1, 0, 0, 0],
        vec![
            meta(m, false, false),
            meta(list.pubkey(), true, true),
            meta(list.pubkey(), false, true),
        ],
        &[&list],
        vec![
            (m, ROUTER, market(key(), key())),
            (list.pubkey(), ROUTER, vec![0; 144]),
        ],
    );
}

#[test]
#[ignore = "requires FOSSA_PARITY_ORIGINAL pointing to freshly built original programs"]
fn router_receipt_can_be_its_rent_payer() {
    let booking = Keypair::new();
    let owner = Keypair::new();
    let m = key();
    let u = key();
    let mut user = vec![0; 120];
    user[..8].copy_from_slice(b"USER0V20");
    user[8..40].copy_from_slice(owner.pubkey().as_ref());
    user[40..72].copy_from_slice(m.as_ref());
    compare(
        ROUTER,
        vec![26],
        vec![
            meta(m, false, false),
            meta(u, false, false),
            meta(owner.pubkey(), false, true),
            meta(booking.pubkey(), true, true),
            meta(booking.pubkey(), false, true),
        ],
        &[&owner, &booking],
        vec![
            (m, ROUTER, market(key(), key())),
            (u, ROUTER, user),
            (booking.pubkey(), ROUTER, vec![0; 368]),
        ],
    );
}

#[test]
#[ignore = "requires FOSSA_PARITY_ORIGINAL pointing to freshly built original programs"]
fn dex_config_can_be_its_admin() {
    let config = Keypair::new();
    let token_mint = key();
    let global = Address::find_program_address(&[b"protocol"], &DEX).0;
    let mut state = vec![0; 112];
    state[..8].copy_from_slice(b"FDEXGLO1");
    state[8..40].copy_from_slice(config.pubkey().as_ref());
    let treasury = key();
    state[40..72].copy_from_slice(treasury.as_ref());
    state[72..104].copy_from_slice(treasury.as_ref());
    let mut payload = vec![0];
    payload.extend(treasury.to_bytes());
    payload.extend(125u16.to_le_bytes());
    payload.extend(10000u16.to_le_bytes());
    payload.extend(0u16.to_le_bytes());
    compare(
        DEX,
        payload,
        vec![
            meta(config.pubkey(), false, true),
            meta(config.pubkey(), true, true),
            meta(token_mint, false, false),
            meta(TOKEN, false, false),
            meta(global, false, false),
        ],
        &[&config],
        vec![
            (config.pubkey(), DEX, vec![0; 192]),
            (token_mint, TOKEN, mint()),
            (global, DEX, state),
        ],
    );
}

fn activation(relaxed: bool) {
    let admin = Keypair::new();
    let pool = Keypair::new();
    let m = key();
    let quote = key();
    let model_id = [7; 32];
    let (model, bump) = Address::find_program_address(&[b"model", m.as_ref(), &model_id], &ROUTER);
    let reserve_mint = Address::find_program_address(&[b"reserve_mint", model.as_ref()], &ROUTER).0;
    let backing = key();
    let allocation = key();
    let config = key();
    let authority = key();
    let source_quote = key();
    let base_vault = key();
    let quote_vault = key();
    let lp_mint = key();
    let lp = key();
    let global = key();
    let mut md = market(admin.pubkey(), quote);
    md.resize(280, 0);
    md[72..104].copy_from_slice(admin.pubkey().as_ref());
    md[104..136].copy_from_slice(admin.pubkey().as_ref());
    md[216..248].copy_from_slice(PROBE.as_ref());
    md[248..280].copy_from_slice(config.as_ref());
    let mut model_data = vec![0; 144];
    model_data[..8].copy_from_slice(b"MODELV20");
    model_data[8..40].copy_from_slice(m.as_ref());
    model_data[40..72].copy_from_slice(&model_id);
    model_data[140] = bump;
    let mut accounts = vec![
        meta(m, false, false),
        meta(model, true, false),
        meta(admin.pubkey(), false, true),
        meta(admin.pubkey(), true, true),
        meta(Address::default(), false, false),
        meta(reserve_mint, true, false),
        meta(backing, true, false),
        meta(allocation, true, false),
        meta(quote, false, false),
        meta(TOKEN, false, false),
        meta(T22, false, false),
        meta(PROBE, false, false),
    ];
    accounts.extend([
        meta(admin.pubkey(), true, true),
        meta(config, false, false),
        meta(pool.pubkey(), !relaxed, !relaxed),
        meta(authority, false, false),
        meta(reserve_mint, false, false),
        meta(quote, false, false),
        meta(allocation, true, false),
        meta(source_quote, true, false),
        meta(base_vault, true, false),
        meta(quote_vault, !relaxed, false),
        meta(lp_mint, !relaxed, false),
        meta(lp, !relaxed, false),
        meta(T22, false, false),
        meta(TOKEN, false, false),
        meta(TOKEN, false, false),
        meta(global, false, false),
    ]);
    let mut payload = vec![37];
    payload.extend(1_000_000u64.to_le_bytes());
    payload.push(6);
    payload.extend(100u16.to_le_bytes());
    payload.extend(100_000u64.to_le_bytes());
    for amount in [100_000u64, 0, 25_000_000_000, 2_202_970_298, 10_000] {
        payload.extend(amount.to_le_bytes());
    }
    let mut fixtures = vec![
        (m, ROUTER, md),
        (model, ROUTER, model_data),
        (quote, TOKEN, mint()),
        (backing, TOKEN, vec![0; 165]),
        (allocation, T22, vec![0; 178]),
        (base_vault, T22, vec![0; 178]),
        (config, PROBE, vec![0; 192]),
        (pool.pubkey(), PROBE, vec![0; 512]),
    ];
    // system payer needs enough prefunding for mint creation and model growth.
    let old = std::path::PathBuf::from(std::env::var("FOSSA_PARITY_ORIGINAL").unwrap());
    let fork = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut outcomes = Vec::new();
    let mut logs = Vec::new();
    let mut states = Vec::new();
    for (label, root) in [("original", old), ("anchor", fork)] {
        let mut vm = LiteSVM::new();
        vm.add_program_from_file(
            ROUTER,
            root.join("target/deploy/inference_router_program.so"),
        )
        .unwrap();
        vm.add_program_from_file(
            PROBE,
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../fixtures/privilege-probe/target/deploy/fossa_parity_privilege_probe.so"),
        )
        .unwrap();
        vm.airdrop(&admin.pubkey(), 10_000_000_000).unwrap();
        for (address, owner, bytes) in &mut fixtures {
            vm.set_account(
                *address,
                Account {
                    lamports: vm.minimum_balance_for_rent_exemption(bytes.len()),
                    owner: *owner,
                    data: bytes.clone(),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        let mut signers = vec![&admin];
        if !relaxed {
            signers.push(&pool);
        }
        let tx = Transaction::new(
            &signers,
            Message::new(
                &[Instruction {
                    program_id: ROUTER,
                    accounts: accounts.clone(),
                    data: payload.clone(),
                }],
                Some(&admin.pubkey()),
            ),
            vm.latest_blockhash(),
        );
        let result = vm.send_transaction(tx);
        println!(
            "{label}: {:?}",
            result.as_ref().map(|m| m.compute_units_consumed)
        );
        outcomes.push(result.is_ok());
        logs.push(
            result
                .as_ref()
                .map(|m| {
                    m.logs
                        .iter()
                        .filter(|l| l.starts_with("Program data: "))
                        .cloned()
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default(),
        );
        states.push(
            accounts
                .iter()
                .map(|a| vm.get_account(&a.pubkey))
                .collect::<Vec<_>>(),
        );
    }
    assert!(
        outcomes[0],
        "replacement adapter must succeed against original"
    );
    assert_eq!(
        outcomes[0], outcomes[1],
        "replacement adapter accepted shape parity"
    );
    assert_eq!(
        logs[0], logs[1],
        "replacement adapter received privilege parity"
    );
    assert_eq!(
        states[0], states[1],
        "replacement adapter business account state parity"
    );
}

#[test]
#[ignore = "requires FOSSA_PARITY_ORIGINAL and offline privilege-probe SBF build"]
fn activation_preserves_replacement_adapter_privileges() {
    activation(false);
}

#[test]
#[ignore = "requires FOSSA_PARITY_ORIGINAL and offline privilege-probe SBF build"]
fn activation_preserves_replacement_adapter_account_requirements() {
    activation(true);
}
