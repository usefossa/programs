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

#[derive(Clone)]
struct Case {
    program: Address,
    accounts: Vec<AccountMeta>,
    data: Vec<u8>,
    fixtures: Vec<(Address, Address, Vec<u8>)>,
}

fn key() -> Address {
    Keypair::new().pubkey()
}
fn meta(pubkey: Address, is_writable: bool, is_signer: bool) -> AccountMeta {
    AccountMeta {
        pubkey,
        is_writable,
        is_signer,
    }
}
fn put_u64(data: &mut [u8], offset: usize, value: u64) {
    data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}
fn token(mint: Address, owner: Address) -> Vec<u8> {
    let mut data = vec![0; 165];
    data[..32].copy_from_slice(mint.as_ref());
    data[32..64].copy_from_slice(owner.as_ref());
    put_u64(&mut data, 64, 500);
    data[108] = 1;
    data
}
fn transfer(op: u8, owner: Address) -> Case {
    let (market, lane, mint, vault, external) = (key(), key(), key(), key(), key());
    let (authority, bump) = Address::find_program_address(&[b"vault", lane.as_ref()], &ROUTER);
    let mut md = vec![0; 72];
    md[..8].copy_from_slice(b"FOSSAV20");
    md[8..40].copy_from_slice(owner.as_ref());
    md[40..72].copy_from_slice(mint.as_ref());
    let mut ud = vec![0; 120];
    ud[..8].copy_from_slice(b"USER0V20");
    ud[8..40].copy_from_slice(owner.as_ref());
    ud[40..72].copy_from_slice(market.as_ref());
    put_u64(&mut ud, 72, 500);
    ud[80..112].copy_from_slice(vault.as_ref());
    ud[112] = bump;
    let mut mt = vec![0; 82];
    mt[44] = 6;
    mt[45] = 1;
    let mut accounts = vec![
        meta(market, false, false),
        meta(lane, true, false),
        meta(owner, false, true),
    ];
    accounts.extend(if op == 3 {
        [
            meta(external, true, false),
            meta(vault, true, false),
            meta(mint, false, false),
        ]
    } else {
        [
            meta(vault, true, false),
            meta(external, true, false),
            meta(mint, false, false),
        ]
    });
    if op == 10 {
        accounts.push(meta(authority, false, false));
    }
    accounts.push(meta(TOKEN, false, false));
    let mut data = vec![op];
    data.extend(100u64.to_le_bytes());
    Case {
        program: ROUTER,
        accounts,
        data,
        fixtures: vec![
            (market, ROUTER, md),
            (lane, ROUTER, ud),
            (mint, TOKEN, mt),
            (vault, TOKEN, token(mint, authority)),
            (external, TOKEN, token(mint, owner)),
        ],
    }
}

fn run(case: &Case, signers: &[&Keypair], succeeds: bool, label: &str) {
    let fork = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let original = std::path::PathBuf::from(std::env::var("FOSSA_PARITY_ORIGINAL").unwrap());
    let artifact = if case.program == ROUTER {
        "target/deploy/inference_router_program.so"
    } else {
        "dex/target/deploy/fossa_dex.so"
    };
    for (kind, base) in [("original", original), ("anchor", fork)] {
        let payer = Keypair::new();
        let mut vm = LiteSVM::new();
        vm.add_program_from_file(case.program, base.join(artifact))
            .unwrap();
        vm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
        for (address, owner, data) in &case.fixtures {
            vm.set_account(
                *address,
                Account {
                    owner: *owner,
                    data: data.clone(),
                    lamports: vm.minimum_balance_for_rent_exemption(data.len()),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        let before: Vec<_> = case
            .fixtures
            .iter()
            .map(|(key, _, _)| vm.get_account(key))
            .collect();
        let ix = Instruction {
            program_id: case.program,
            accounts: case.accounts.clone(),
            data: case.data.clone(),
        };
        let message = Message::new(&[ix], Some(&payer.pubkey()));
        let required = &message.account_keys[..usize::from(message.header.num_required_signatures)];
        let mut keys = vec![&payer];
        keys.extend(
            signers
                .iter()
                .copied()
                .filter(|signer| required.contains(&signer.pubkey())),
        );
        let tx = Transaction::new(&keys, message, vm.latest_blockhash());
        let result = vm.send_transaction(tx);
        assert_eq!(result.is_ok(), succeeds, "{label} / {kind}: {result:?}");
        if !succeeds {
            let after: Vec<_> = case
                .fixtures
                .iter()
                .map(|(key, _, _)| vm.get_account(key))
                .collect();
            assert_eq!(
                before, after,
                "{label} / {kind}: failed instruction mutated state"
            );
        }
    }
}

#[test]
#[ignore = "offline security probes require FOSSA_PARITY_ORIGINAL and fresh SBF artifacts"]
fn router_custody_rejects_forgery_aliases_and_malformed_data() {
    let owner = Keypair::new();
    let attacker = Keypair::new();
    for op in [3, 10] {
        let base = transfer(op, owner.pubkey());
        let keys = [&owner, &attacker];
        run(&base, &keys, true, "valid custody control");
        let mut cases: Vec<(&str, Case)> = Vec::new();
        macro_rules! case {
            ($label:expr, $value:ident, $body:block) => {{
                let mut $value = base.clone();
                $body;
                cases.push(($label, $value));
            }};
        }
        case!("missing owner signer", c, {
            c.accounts[2].is_signer = false;
        });
        case!("impostor owner", c, {
            c.accounts[2].pubkey = attacker.pubkey();
        });
        case!("readonly user state", c, {
            c.accounts[1].is_writable = false;
        });
        case!("readonly source", c, {
            c.accounts[3].is_writable = false;
        });
        case!("readonly destination", c, {
            c.accounts[4].is_writable = false;
        });
        case!("wrong token program", c, {
            c.accounts.last_mut().unwrap().pubkey = Address::default();
        });
        case!("vault source alias", c, {
            c.accounts[3].pubkey = c.accounts[4].pubkey;
        });
        case!("state market alias", c, {
            c.accounts[1].pubkey = c.accounts[0].pubkey;
        });
        case!("state token alias", c, {
            c.accounts[3].pubkey = c.accounts[1].pubkey;
        });
        case!("market owner forgery", c, {
            c.fixtures[0].1 = Address::default();
        });
        case!("user owner forgery", c, {
            c.fixtures[1].1 = Address::default();
        });
        case!("user discriminator forgery", c, {
            c.fixtures[1].2[0] ^= 1;
        });
        case!("user wrong market", c, {
            c.fixtures[1].2[40..72].fill(1);
        });
        case!("user wrong vault", c, {
            c.fixtures[1].2[80..112].fill(1);
        });
        case!("truncated user", c, {
            c.fixtures[1].2.truncate(119);
        });
        case!("oversized user", c, {
            c.fixtures[1].2.push(0);
        });
        case!("vault wrong authority", c, {
            c.fixtures[3].2[32..64].copy_from_slice(attacker.pubkey().as_ref());
        });
        case!("vault owner forgery", c, {
            c.fixtures[3].1 = Address::default();
        });
        case!("vault delegate", c, {
            c.fixtures[3].2[72] = 1;
        });
        case!("vault close authority", c, {
            c.fixtures[3].2[129] = 1;
        });
        case!("vault native", c, {
            c.fixtures[3].2[109] = 1;
        });
        case!("vault frozen", c, {
            c.fixtures[3].2[108] = 2;
        });
        case!("zero amount", c, {
            c.data[1..].fill(0);
        });
        case!("excess amount", c, {
            c.data[1..].copy_from_slice(&501u64.to_le_bytes());
        });
        case!("max amount", c, {
            c.data[1..].fill(255);
        });
        case!("truncated payload", c, {
            c.data.pop();
        });
        case!("trailing payload", c, {
            c.data.push(0);
        });
        case!("oversized payload", c, {
            c.data.resize(326, 0);
        });
        case!("extra account", c, {
            c.accounts.push(meta(attacker.pubkey(), false, false));
        });
        if op == 3 {
            case!("free balance overflow", c, {
                put_u64(&mut c.fixtures[1].2, 72, u64::MAX);
            });
            case!("insufficient source tokens", c, {
                put_u64(&mut c.fixtures[4].2, 64, 0);
            });
        } else {
            case!("wrong authority pda", c, {
                c.accounts[6].pubkey = attacker.pubkey();
            });
            case!("insufficient vault tokens", c, {
                put_u64(&mut c.fixtures[3].2, 64, 0);
            });
        }
        for (label, c) in cases {
            run(&c, &keys, false, label);
        }
    }
}

#[test]
#[ignore = "offline security probes require FOSSA_PARITY_ORIGINAL and fresh SBF artifacts"]
fn router_and_dex_admin_transitions_reject_forgery() {
    let admin = Keypair::new();
    let next = Keypair::new();
    let attacker = Keypair::new();
    let keys = [&admin, &next, &attacker];
    for program in [ROUTER, DEX] {
        let (state, mut bytes, mut data) = if program == ROUTER {
            (key(), vec![0; 72], vec![43])
        } else {
            (
                Address::find_program_address(&[b"protocol"], &DEX).0,
                vec![0; 112],
                vec![15],
            )
        };
        bytes[..8].copy_from_slice(if program == ROUTER {
            b"FOSSAV20"
        } else {
            b"FDEXGLO1"
        });
        bytes[8..40].copy_from_slice(admin.pubkey().as_ref());
        if program == DEX {
            data.extend([1; 64]);
        }
        let accounts = if program == ROUTER {
            vec![
                meta(state, true, false),
                meta(admin.pubkey(), false, true),
                meta(next.pubkey(), false, true),
            ]
        } else {
            vec![
                meta(admin.pubkey(), false, true),
                meta(next.pubkey(), false, true),
                meta(state, true, false),
            ]
        };
        let (state_index, admin_index, next_index) = if program == ROUTER {
            (0, 1, 2)
        } else {
            (2, 0, 1)
        };
        let base = Case {
            program,
            accounts,
            data,
            fixtures: vec![(state, program, bytes)],
        };
        run(&base, &keys, true, "valid admin control");
        for mode in 0..8 {
            let mut c = base.clone();
            match mode {
                0 => c.accounts[admin_index].is_signer = false,
                1 => c.accounts[next_index].is_signer = false,
                2 => c.accounts[admin_index].pubkey = attacker.pubkey(),
                3 => c.accounts[state_index].is_writable = false,
                4 => c.fixtures[0].1 = Address::default(),
                5 => c.fixtures[0].2[0] ^= 1,
                6 => {
                    c.fixtures[0].2.pop();
                }
                7 => {
                    c.data.push(0);
                }
                _ => unreachable!(),
            }
            run(&c, &keys, false, &format!("admin rejection {mode}"));
        }
        if program == DEX {
            let mut c = base.clone();
            let forged = key();
            c.fixtures[0].0 = forged;
            c.accounts[state_index].pubkey = forged;
            run(&c, &keys, false, "forged global pda");
        }
    }
}
