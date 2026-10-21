use litesvm::LiteSVM;
use solana_account::Account;
use solana_address::{address, Address};
use solana_clock::Clock;
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_message::Message;
use solana_signer::Signer;
use solana_transaction::Transaction;

#[allow(dead_code)]
#[path = "../../../src/abi.rs"]
mod abi;
use abi::*;
#[path = "../../support/mod.rs"]
mod support;
const TOKEN: Address = address!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const SYSTEM: Address = address!("11111111111111111111111111111111");
fn key() -> Address {
    Keypair::new().pubkey()
}
fn a(k: Address, w: bool, s: bool) -> AccountMeta {
    AccountMeta {
        pubkey: k,
        is_signer: s,
        is_writable: w,
    }
}
fn r(k: Address) -> AccountMeta {
    a(k, false, false)
}
fn w(k: Address) -> AccountMeta {
    a(k, true, false)
}
fn s(k: Address) -> AccountMeta {
    a(k, false, true)
}
fn ws(k: Address) -> AccountMeta {
    a(k, true, true)
}
fn u32at(d: &mut [u8], at: usize, value: u32) {
    d[at..at + 4].copy_from_slice(&value.to_le_bytes());
}
fn u64at(d: &mut [u8], at: usize, value: u64) {
    d[at..at + 8].copy_from_slice(&value.to_le_bytes());
}
fn rd32(d: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(d[at..at + 4].try_into().unwrap())
}
fn rd64(d: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(d[at..at + 8].try_into().unwrap())
}
struct Vm {
    svm: LiteSVM,
    program: Address,
    payer: Keypair,
}
impl Vm {
    fn new() -> Self {
        let mut svm = support::svm(std::env::var_os("FOSSA_LEGACY_TOKEN").is_some());
        let program = key();
        support::load_program(&mut svm, program);
        let payer = Keypair::new();
        svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
        Self {
            svm,
            program,
            payer,
        }
    }
    fn put(&mut self, key: Address, owner: Address, data: Vec<u8>) {
        let lamports = self.svm.minimum_balance_for_rent_exemption(data.len());
        self.svm
            .set_account(
                key,
                Account {
                    lamports,
                    owner,
                    data,
                    ..Default::default()
                },
            )
            .unwrap();
    }
    fn blank(&mut self, key: &Keypair, len: usize) {
        self.put(key.pubkey(), self.program, vec![0; len]);
    }
    fn token(&mut self, mint: Address, owner: Address, amount: u64) -> Address {
        let key = key();
        let mut d = vec![0; 165];
        d[..32].copy_from_slice(mint.as_ref());
        d[32..64].copy_from_slice(owner.as_ref());
        u64at(&mut d, 64, amount);
        d[108] = 1;
        self.put(key, TOKEN, d);
        key
    }
    fn data(&self, key: Address) -> Vec<u8> {
        self.svm.get_account(&key).unwrap().data
    }
    fn send(
        &mut self,
        op: u8,
        payload: &[u8],
        accounts: Vec<AccountMeta>,
        signers: &[&Keypair],
        succeeds: bool,
    ) -> u64 {
        self.svm.expire_blockhash();
        let mut data = vec![op];
        data.extend_from_slice(payload);
        let mut budget = vec![2];
        budget.extend_from_slice(&1_400_000u32.to_le_bytes());
        let ixs = [
            Instruction {
                program_id: address!("ComputeBudget111111111111111111111111111111"),
                accounts: vec![],
                data: budget,
            },
            Instruction {
                program_id: self.program,
                accounts,
                data,
            },
        ];
        let mut keys = vec![&self.payer];
        for signer in signers {
            if signer.pubkey() != self.payer.pubkey() {
                keys.push(*signer);
            }
        }
        let msg = Message::new(&ixs, Some(&self.payer.pubkey()));
        let tx = Transaction::new(&keys, msg, self.svm.latest_blockhash());
        match self.svm.send_transaction(tx) {
            Ok(meta) => {
                assert!(succeeds, "opcode {op} unexpectedly succeeded");
                meta.compute_units_consumed
            }
            Err(error) => {
                assert!(!succeeds, "opcode {op} failed: {error:?}");
                error.meta.compute_units_consumed
            }
        }
    }
}
fn book_payload(model: &[u8; 32], provider: Address) -> [u8; 107] {
    let mut p = [0; 107];
    p[..32].copy_from_slice(model);
    for at in [32, 36, 40] {
        u32at(&mut p, at, u32::MAX);
    }
    for at in [46, 50, 63, 71] {
        u32at(&mut p, at, 1_000_000);
    }
    u64at(&mut p, 55, 1_000_000_000_000);
    p[75..107].copy_from_slice(provider.as_ref());
    p
}
fn main() {
    let mut v = Vm::new();
    let admin = v.payer.pubkey();
    let market = Keypair::new();
    v.blank(&market, MARKET_LEN);
    let mint = key();
    let mut mint_data = vec![0; 82];
    mint_data[44] = 6;
    mint_data[45] = 1;
    u64at(&mut mint_data, 36, 1_000_000_000_000);
    v.put(mint, TOKEN, mint_data);
    v.send(
        0,
        &[],
        vec![ws(market.pubkey()), s(admin), r(mint)],
        &[&market],
        true,
    );
    let market = market.pubkey();
    let provider = Keypair::new();
    let (profile, _) = Address::find_program_address(
        &[b"provider", market.as_ref(), provider.pubkey().as_ref()],
        &v.program,
    );
    let mut metadata = vec![1; 32];
    metadata.extend_from_slice(b"https://example.test/metadata.json");
    v.send(
        19,
        &metadata,
        vec![
            r(market),
            w(profile),
            s(provider.pubkey()),
            ws(admin),
            r(SYSTEM),
        ],
        &[&provider],
        true,
    );
    v.send(20, &[1], vec![r(market), w(profile), s(admin)], &[], true);
    let model_id = [7; 32];
    let (model, _) =
        Address::find_program_address(&[b"model", market.as_ref(), &model_id], &v.program);
    // pre-funded canonical address must remain initializable.
    v.svm.airdrop(&model, 1_000_000).unwrap();
    let terms = Keypair::new();
    let capacity = Keypair::new();
    v.blank(&terms, terms_book_len(MAX_ENTRIES));
    v.blank(&capacity, capacity_book_len(MAX_ENTRIES));
    let (model_metadata, _) =
        Address::find_program_address(&[b"model_metadata", model.as_ref()], &v.program);
    let mut init = model_id.to_vec();
    init.extend_from_slice(&(MAX_ENTRIES as u32).to_le_bytes());
    init.extend_from_slice(&metadata);
    let cu = v.send(
        18,
        &init,
        vec![
            r(market),
            w(model),
            ws(terms.pubkey()),
            ws(capacity.pubkey()),
            s(provider.pubkey()),
            ws(admin),
            r(SYSTEM),
            r(profile),
            w(model_metadata),
            s(admin),
        ],
        &[&terms, &capacity, &provider],
        true,
    );
    println!("initialize_model entries={MAX_ENTRIES} cu={cu}");
    let terms = terms.pubkey();
    let capacity = capacity.pubkey();
    let owner = Keypair::new();
    let user = Keypair::new();
    v.blank(&user, USER_LEN);
    let (user_authority, _) =
        Address::find_program_address(&[b"vault", user.pubkey().as_ref()], &v.program);
    let user_vault = v.token(mint, user_authority, 0);
    let source = v.token(mint, owner.pubkey(), 100_000_000);
    v.send(
        1,
        &[],
        vec![
            ws(user.pubkey()),
            s(owner.pubkey()),
            r(market),
            r(user_vault),
        ],
        &[&user, &owner],
        true,
    );
    let user = user.pubkey();
    v.send(
        3,
        &100_000_000u64.to_le_bytes(),
        vec![
            r(market),
            w(user),
            s(owner.pubkey()),
            w(source),
            w(user_vault),
            r(mint),
            r(TOKEN),
        ],
        &[&owner],
        true,
    );
    let payout = v.token(mint, provider.pubkey(), 100_000_000);
    let (bond_authority, _) =
        Address::find_program_address(&[b"bond", model.as_ref(), &0u32.to_le_bytes()], &v.program);
    let bond = v.token(mint, bond_authority, 0);
    let mut registration = [0; 24];
    for (at, value) in [(4, 200), (8, 10), (12, 300), (20, 1_000)] {
        u32at(&mut registration, at, value);
    }
    registration[16] = 100;
    registration[18] = 1;
    registration[19] = 1;
    v.send(
        2,
        &registration,
        vec![
            r(market),
            r(model),
            w(terms),
            w(capacity),
            r(profile),
            s(provider.pubkey()),
            r(payout),
            r(bond),
        ],
        &[&provider],
        true,
    );
    let mut fund = [0; 12];
    u64at(&mut fund, 4, 50_000_000);
    v.send(
        4,
        &fund,
        vec![
            r(market),
            r(model),
            r(terms),
            w(capacity),
            s(provider.pubkey()),
            w(payout),
            w(bond),
            r(mint),
            r(TOKEN),
        ],
        &[&provider],
        true,
    );
    let booking = Keypair::new();
    v.blank(&booking, BOOKING_LEN);
    let payload = book_payload(&model_id, provider.pubkey());
    let mut excessive = payload;
    u32at(&mut excessive, 50, u32::MAX);
    v.send(
        6,
        &excessive,
        vec![r(market), r(model), r(terms), w(capacity), w(user), s(owner.pubkey()), ws(booking.pubkey()), s(admin)],
        &[&owner, &booking],
        false,
    );
    assert_eq!(rd64(&v.data(user), 72), 100_000_000);
    assert_eq!(rd64(&v.data(capacity), HEADER_LEN + 8), 0);
    let cu = v.send(
        6,
        &payload,
        vec![
            r(market),
            r(model),
            r(terms),
            w(capacity),
            w(user),
            s(owner.pubkey()),
            ws(booking.pubkey()),
            s(admin),
        ],
        &[&owner, &booking],
        true,
    );
    let booking = booking.pubkey();
    println!("book entries=1 cu={cu}");
    assert_eq!(rd64(&v.data(booking), 104), 501);
    assert_eq!(rd64(&v.data(capacity), HEADER_LEN + 8), 501);
    let mut clock = v.svm.get_sysvar::<Clock>();
    assert_eq!(rd64(&v.data(booking), 132) as i64 - clock.unix_timestamp, 10_300);
    clock.unix_timestamp += 301;
    v.svm.set_sysvar(&clock);
    // long generation remains funded after the previous five-minute cutoff.
    v.send(8, &[], vec![r(market), r(model), w(capacity), w(user), w(booking)], &[], false);
    // updates cannot alter existing booking price or payment destinations.
    u32at(&mut registration, 4, 9_000);
    u32at(&mut registration, 12, 9_000);
    v.send(
        5,
        &registration,
        vec![r(market), r(model), w(terms), s(provider.pubkey())],
        &[&provider],
        true,
    );
    let mut usage = [0; 12];
    u32at(&mut usage, 0, 500_000);
    u32at(&mut usage, 4, 500_000);
    u32at(&mut usage, 8, 1_000_000);
    let claim_accounts = vec![
        r(market),
        r(model),
        w(capacity),
        w(user),
        w(booking),
        s(provider.pubkey()),
        w(user_vault),
        w(payout),
        r(mint),
        r(user_authority),
        r(TOKEN),
    ];
    let cu = v.send(7, &usage, claim_accounts.clone(), &[&provider], true);
    println!("claim cu={cu}");
    assert_eq!(rd64(&v.data(booking), 160), 405);
    assert_eq!(rd64(&v.data(capacity), HEADER_LEN + 8), 405);
    v.send(7, &usage, claim_accounts, &[&provider], false);
    v.send(
        11,
        &[],
        vec![r(market), r(model), w(booking), s(owner.pubkey())],
        &[&owner],
        true,
    );
    let cu = v.send(
        12,
        &405u64.to_le_bytes(),
        vec![
            r(market),
            r(model),
            w(capacity),
            w(user),
            w(booking),
            s(admin),
            w(bond),
            w(user_vault),
            r(mint),
            r(bond_authority),
            r(TOKEN),
        ],
        &[],
        true,
    );
    println!("resolve cu={cu}");
    assert_eq!(rd64(&v.data(user), 72), 100_000_000);
    assert_eq!(rd64(&v.data(user_vault), 64), 100_000_000);
    assert_eq!(rd64(&v.data(bond), 64), 49_999_595);
    assert_eq!(rd64(&v.data(capacity), HEADER_LEN), 49_999_595);
    assert_eq!(rd64(&v.data(capacity), HEADER_LEN + 8), 0);
    assert_eq!(rd32(&v.data(capacity), HEADER_LEN + 16), 0);
    assert_eq!(rd32(&v.data(capacity), HEADER_LEN + 20), 0);
    v.send(17, &[], vec![w(booking), w(admin)], &[], true);
    assert!(v.svm.get_account(&booking).is_none_or(|a| a.lamports == 0));
    let expired = Keypair::new();
    v.blank(&expired, BOOKING_LEN);
    v.send(
        6,
        &payload,
        vec![r(market), r(model), r(terms), w(capacity), w(user), s(owner.pubkey()), ws(expired.pubkey()), s(admin)],
        &[&owner, &expired],
        true,
    );
    let mut clock = v.svm.get_sysvar::<Clock>();
    clock.unix_timestamp = rd64(&v.data(expired.pubkey()), 132) as i64 + 1;
    v.svm.set_sysvar(&clock);
    v.send(
        7,
        &usage,
        vec![r(market), r(model), w(capacity), w(user), w(expired.pubkey()), s(provider.pubkey()), w(user_vault), w(payout), r(mint), r(user_authority), r(TOKEN)],
        &[&provider],
        false,
    );
    v.send(8, &[], vec![r(market), r(model), w(capacity), w(user), w(expired.pubkey())], &[], true);
    assert_eq!(rd64(&v.data(user), 72), 100_000_000);
    assert_eq!(rd64(&v.data(capacity), HEADER_LEN + 8), 0);
    println!("deadline checks passed: late settlement, early-expiry rejection, expired refund, duration bound");
    let request = Keypair::new();
    v.blank(&request, UNBOND_LEN);
    let mut unbond = [0; 12];
    u64at(&mut unbond, 4, 49_999_595);
    v.send(
        13,
        &unbond,
        vec![
            r(market),
            r(model),
            r(terms),
            w(capacity),
            s(provider.pubkey()),
            ws(request.pubkey()),
            s(admin),
        ],
        &[&provider, &request],
        true,
    );
    assert_eq!(rd64(&v.data(capacity), HEADER_LEN), 0);
    assert_eq!(rd64(&v.data(capacity), HEADER_LEN + 24), 49_999_595);
    let unbond_accounts = vec![
        r(market),
        r(model),
        r(terms),
        w(capacity),
        w(request.pubkey()),
        s(provider.pubkey()),
        w(bond),
        w(payout),
        r(mint),
        r(bond_authority),
        r(TOKEN),
    ];
    v.send(14, &[], unbond_accounts.clone(), &[&provider], false);
    let mut clock = v.svm.get_sysvar::<Clock>();
    clock.unix_timestamp += 4_000;
    v.svm.set_sysvar(&clock);
    v.send(14, &[], unbond_accounts.clone(), &[&provider], true);
    v.send(14, &[], unbond_accounts, &[&provider], false);
    assert_eq!(rd64(&v.data(bond), 64), 0);
    assert_eq!(rd64(&v.data(capacity), HEADER_LEN + 24), 0);
    v.send(17, &[], vec![w(request.pubkey()), w(admin)], &[], true);
    // synthetic scan fixtures measure sbf branches, not large-book registration/custody correctness.
    // litesvm 0.16 default feature profile uses p-token for tokenkeg; settlement cu is profile-specific.
    let original_terms = v.data(terms);
    let original_capacity = v.data(capacity);
    // 0 descending, 1 ascending, 2 ties, 3 inactive, 4 unfunded,
    // 5 maximum integer limits with mixed cache identity, 6 maximum limits with matching cache identity.
    for (count, mode) in [500usize, 1_024, MAX_ENTRIES]
        .into_iter()
        .flat_map(|count| (0..7).map(move |mode| (count, mode)))
    {
        let mut td = original_terms.clone();
        let mut cd = original_capacity.clone();
        u32at(&mut td, 44, count as u32);
        u32at(&mut cd, 44, count as u32);
        for slot in 0..count {
            let ti = HEADER_LEN + slot * TERMS_LEN;
            let ci = HEADER_LEN + slot * CAPACITY_LEN;
            td[ti..ti + TERMS_LEN]
                .copy_from_slice(&original_terms[HEADER_LEN..HEADER_LEN + TERMS_LEN]);
            // strictly descending scores force every entry to replace winner.
            for at in [96, 100, 104] {
                u32at(
                    &mut td,
                    ti + at,
                    match mode {
                        0 => (count - slot) as u32,
                        1 => (slot + 1) as u32,
                        5 | 6 => u32::MAX - slot as u32,
                        _ => 1,
                    },
                );
            }
            if mode == 3 {
                td[ti + 111] = 0;
            }
            if mode == 5 && slot % 2 == 1 {
                td[ti] ^= 1;
            }
            if mode >= 5 {
                td[ti + 108..ti + 110].copy_from_slice(&u16::MAX.to_le_bytes());
            }
            u64at(&mut cd, ci, if mode == 4 { 0 } else { 50_000_000_000_000 });
            u64at(&mut cd, ci + 8, 0);
            u32at(&mut cd, ci + 16, 0);
            u32at(&mut cd, ci + 20, 0);
        }
        v.put(terms, v.program, td);
        v.put(capacity, v.program, cd);
        let booking = Keypair::new();
        v.blank(&booking, BOOKING_LEN);
        let mut request = payload;
        if mode >= 5 {
            for at in [46, 50, 63, 71] {
                u32at(&mut request, at, u32::MAX);
            }
            u32at(&mut request, 67, u32::MAX / 2);
            u64at(&mut request, 55, u64::MAX);
            let mut ud = v.data(user);
            u64at(&mut ud, 72, 9_000_000_000_000_000);
            v.put(user, v.program, ud);
        }
        let cu = v.send(
            6,
            &request,
            vec![
                r(market),
                r(model),
                r(terms),
                w(capacity),
                w(user),
                s(owner.pubkey()),
                ws(booking.pubkey()),
                s(admin),
            ],
            &[&owner, &booking],
            mode != 3 && mode != 4,
        );
        if mode != 3 && mode != 4 {
            assert_eq!(
                rd32(&v.data(booking.pubkey()), 352),
                if mode == 0 || mode >= 5 {
                    count as u32 - 1
                } else {
                    0
                }
            );
        }
        println!("book mode={mode} entries={count} cu={cu}");
    }
    println!("smoke passed: real sbf init/register/fund/book/update/claim/dispute/refund/unbond/close; custody reconciled");
}
