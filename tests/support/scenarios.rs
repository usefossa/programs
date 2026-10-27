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
#[path = "../../src/abi.rs"]
mod abi;
use abi::*;
#[path = "mod.rs"]
mod support;
#[path = "parity.rs"]
mod parity;
const TOKEN: Address = address!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const SYSTEM: Address = address!("11111111111111111111111111111111");
fn key() -> Address {
    Keypair::new().pubkey()
}
fn a(pubkey: Address, is_writable: bool, is_signer: bool) -> AccountMeta {
    AccountMeta {
        pubkey,
        is_signer,
        is_writable,
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
fn wr32(d: &mut [u8], at: usize, n: u32) {
    d[at..at + 4].copy_from_slice(&n.to_le_bytes());
}
fn wr64(d: &mut [u8], at: usize, n: u64) {
    d[at..at + 8].copy_from_slice(&n.to_le_bytes());
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
    executions: usize,
    migration: bool,
}
impl Vm {
    fn new(legacy_token: bool, migration: bool) -> Self {
        let mut svm = support::svm(legacy_token);
        let program = Address::new_from_array([8; 32]);
        if migration {
            svm.add_program_from_file(
                program,
                support::artifact(
                    "FOSSA_MIGRATION_SBF",
                    "target/migration/inference_router_program.so",
                ),
            )
            .expect("build migration SBF first");
        } else {
            support::load_program(&mut svm, program);
        }
        let payer = Keypair::new();
        svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
        Self {
            svm,
            program,
            payer,
            executions: 0,
            migration,
        }
    }
    fn put(&mut self, address: Address, owner: Address, data: Vec<u8>) {
        let lamports = self.svm.minimum_balance_for_rent_exemption(data.len());
        self.svm
            .set_account(
                address,
                Account {
                    lamports,
                    owner,
                    data,
                    ..Default::default()
                },
            )
            .unwrap();
    }
    fn replace(&mut self, address: Address, data: Vec<u8>) {
        let mut account = self.svm.get_account(&address).unwrap();
        account.data = data;
        self.svm.set_account(address, account).unwrap();
    }
    fn blank(&mut self, key: &Keypair, len: usize) {
        self.put(key.pubkey(), self.program, vec![0; len]);
    }
    fn token(&mut self, mint: Address, owner: Address, amount: u64) -> Address {
        let address = key();
        let mut d = vec![0; 165];
        d[..32].copy_from_slice(mint.as_ref());
        d[32..64].copy_from_slice(owner.as_ref());
        wr64(&mut d, 64, amount);
        d[108] = 1;
        self.put(address, TOKEN, d);
        address
    }
    fn data(&self, address: Address) -> Vec<u8> {
        self.svm.get_account(&address).unwrap().data
    }
    #[track_caller]
    fn send(
        &mut self,
        op: u8,
        payload: &[u8],
        accounts: Vec<AccountMeta>,
        signers: &[&Keypair],
        succeeds: bool,
    ) -> u64 {
        self.send_with_prefix(op, payload, accounts, signers, succeeds, vec![])
    }
    #[track_caller]
    fn send_with_prefix(
        &mut self,
        op: u8,
        payload: &[u8],
        accounts: Vec<AccountMeta>,
        signers: &[&Keypair],
        succeeds: bool,
        mut prefix: Vec<Instruction>,
    ) -> u64 {
        self.executions += 1;
        self.svm.expire_blockhash();
        let before: Vec<_> = accounts
            .iter()
            .filter(|a| a.is_writable && a.pubkey != self.payer.pubkey())
            .map(|a| (a.pubkey, self.svm.get_account(&a.pubkey)))
            .collect();
        let mut data = vec![op];
        data.extend_from_slice(payload);
        let mut budget = vec![2];
        budget.extend_from_slice(&1_400_000u32.to_le_bytes());
        let mut ixs = vec![
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
        ixs.splice(1..1, prefix.drain(..));
        let mut keys = vec![&self.payer];
        for key in signers {
            if key.pubkey() != self.payer.pubkey()
                && !keys.iter().any(|k| k.pubkey() == key.pubkey())
            {
                keys.push(*key);
            }
        }
        let tx = Transaction::new(
            &keys,
            Message::new(&ixs, Some(&self.payer.pubkey())),
            self.svm.latest_blockhash(),
        );
        let packet_len = 1 + 64 * tx.signatures.len() + tx.message_data().len();
        if op == 37 {
            assert!(
                packet_len <= 1232,
                "activation packet {packet_len} exceeds 1232 bytes"
            );
        }
        let shadow = parity::run(&self.svm, &tx, self.migration);
        let result = self.svm.send_transaction(tx);
        if let Some(shadow) = shadow {
            shadow.check(&self.svm, &result, &format!("router opcode {op}"));
        }
        match result {
            Ok(meta) => {
                assert!(succeeds, "opcode {op} unexpectedly succeeded");
                if op == 37 {
                    println!(
                        "atomic model activation serialized legacy transaction: {packet_len} bytes"
                    );
                }
                meta.compute_units_consumed
            }
            Err(error) => {
                assert!(!succeeds, "opcode {op} failed: {error:?}");
                for (key, account) in before {
                    assert_eq!(
                        self.svm.get_account(&key),
                        account,
                        "opcode {op} failed without rollback"
                    );
                }
                error.meta.compute_units_consumed
            }
        }
    }
}

struct Fixture {
    market: Address,
    mint: Address,
    model: Address,
    id: [u8; 32],
    terms: Address,
    capacity: Address,
    providers: Vec<Keypair>,
    payouts: Vec<Address>,
    owner: Keypair,
    user: Address,
    vault: Address,
    authority: Address,
    source: Address,
    list: Keypair,
    manager: Keypair,
}
fn list_metadata_address(v: &Vm, list: Address) -> Address {
    Address::find_program_address(&[b"list_metadata", list.as_ref()], &v.program).0
}
fn metadata() -> Vec<u8> {
    let mut p = vec![1; 32];
    p.extend_from_slice(b"https://example.test/model.json");
    p
}
fn offer(slot: u32, rates: [u32; 3]) -> [u8; 24] {
    let mut p = [0; 24];
    wr32(&mut p, 0, slot);
    for (index, rate) in rates.iter().enumerate() {
        wr32(&mut p, 4 + index * 4, *rate);
    }
    p[16..18].copy_from_slice(&100u16.to_le_bytes());
    p[18] = 1;
    p[19] = 1;
    wr32(&mut p, 20, 4);
    p
}
fn setup(v: &mut Vm) -> Fixture {
    let creator = v.payer.pubkey();
    let market = Keypair::new();
    v.blank(&market, MARKET_LEN);
    let mint = key();
    let mut mint_data = vec![0; 82];
    mint_data[44] = 6;
    mint_data[45] = 1;
    wr64(&mut mint_data, 36, 1_000_000_000);
    v.put(mint, TOKEN, mint_data);
    v.send(
        0,
        &[],
        vec![ws(market.pubkey()), s(creator), r(mint)],
        &[&market],
        true,
    );
    let market = market.pubkey();
    let providers: Vec<_> = (0..3).map(|_| Keypair::new()).collect();
    let mut profiles = vec![];
    let mut payouts = vec![];
    for provider in &providers {
        v.svm.airdrop(&provider.pubkey(), 1_000_000).unwrap();
        let profile = Address::find_program_address(
            &[b"provider", market.as_ref(), provider.pubkey().as_ref()],
            &v.program,
        )
        .0;
        let accounts = vec![
            r(market),
            w(profile),
            s(provider.pubkey()),
            ws(creator),
            r(SYSTEM),
        ];
        v.send(19, &metadata(), accounts.clone(), &[provider], true);
        assert_eq!(v.data(profile)[106], 0);
        v.send(19, &metadata(), accounts, &[provider], false);
        profiles.push(profile);
        payouts.push(v.token(mint, provider.pubkey(), 0));
    }
    let id = [7; 32];
    let model = Address::find_program_address(&[b"model", market.as_ref(), &id], &v.program).0;
    v.svm.airdrop(&model, 1_000_000).unwrap();
    let terms = Keypair::new();
    let capacity = Keypair::new();
    v.blank(&terms, terms_book_len(MAX_ENTRIES));
    v.blank(&capacity, capacity_book_len(MAX_ENTRIES));
    let model_metadata =
        Address::find_program_address(&[b"model_metadata", model.as_ref()], &v.program).0;
    let mut p = id.to_vec();
    p.extend_from_slice(&(MAX_ENTRIES as u32).to_le_bytes());
    p.extend_from_slice(&metadata());
    let accounts = vec![
        r(market),
        w(model),
        ws(terms.pubkey()),
        ws(capacity.pubkey()),
        s(providers[0].pubkey()),
        ws(creator),
        r(SYSTEM),
        r(profiles[0]),
        w(model_metadata),
    ];
    let mut invalid = accounts.clone();
    invalid[7] = r(profiles[1]);
    v.send(18, &p, invalid, &[&terms, &capacity, &providers[0]], false);
    v.send(
        18,
        &p,
        accounts.clone(),
        &[&terms, &capacity, &providers[0]],
        true,
    );
    v.send(18, &p, accounts, &[&terms, &capacity, &providers[0]], false);
    assert_eq!(
        &v.data(model_metadata)[72..104],
        providers[0].pubkey().as_ref()
    );
    v.send(
        22,
        &metadata(),
        vec![
            r(market),
            r(model),
            w(model_metadata),
            s(providers[1].pubkey()),
        ],
        &[&providers[1]],
        false,
    );
    for slot in 0..3 {
        let prices = if slot == 1 {
            [200, 20, 300]
        } else {
            [100, 10, 200]
        };
        let accounts = vec![
            r(market),
            r(model),
            w(terms.pubkey()),
            w(capacity.pubkey()),
            r(profiles[slot]),
            s(providers[slot].pubkey()),
            r(payouts[slot]),
        ];
        v.send(
            2,
            &offer(slot as u32, prices),
            accounts.clone(),
            &[&providers[slot]],
            true,
        );
        v.send(
            2,
            &offer(slot as u32 + 1, prices),
            accounts,
            &[&providers[slot]],
            false,
        );
    }
    let owner = Keypair::new();
    v.svm.airdrop(&owner.pubkey(), 1_000_000).unwrap();
    let user = Keypair::new();
    v.blank(&user, USER_LEN);
    let authority =
        Address::find_program_address(&[b"vault", user.pubkey().as_ref()], &v.program).0;
    let vault = v.token(mint, authority, 0);
    let source = v.token(mint, owner.pubkey(), 1_000_000);
    v.send(
        1,
        &[],
        vec![ws(user.pubkey()), s(owner.pubkey()), r(market), r(vault)],
        &[&owner, &user],
        true,
    );
    v.send(
        3,
        &1_000_000u64.to_le_bytes(),
        vec![
            r(market),
            w(user.pubkey()),
            s(owner.pubkey()),
            w(source),
            w(vault),
            r(mint),
            r(TOKEN),
        ],
        &[&owner],
        true,
    );
    let manager = Keypair::new();
    v.svm.airdrop(&manager.pubkey(), 1_000_000).unwrap();
    let list = Keypair::new();
    v.blank(&list, provider_list_len(4));
    v.send(
        28,
        &4u32.to_le_bytes(),
        vec![r(market), ws(list.pubkey()), s(manager.pubkey())],
        &[&list, &manager],
        true,
    );
    Fixture {
        market,
        mint,
        model,
        id,
        terms: terms.pubkey(),
        capacity: capacity.pubkey(),
        providers,
        payouts,
        owner,
        user: user.pubkey(),
        vault,
        authority,
        source,
        list,
        manager,
    }
}
fn member(v: &mut Vm, f: &Fixture, provider: Address, included: bool, succeeds: bool) {
    let mut p = provider.to_bytes().to_vec();
    p.push(u8::from(included));
    v.send(
        29,
        &p,
        vec![r(f.market), w(f.list.pubkey()), s(f.manager.pubkey())],
        &[&f.manager],
        succeeds,
    );
}
fn request(f: &Fixture, generation: Option<u64>) -> Vec<u8> {
    let mut p = vec![0; 107];
    p[..32].copy_from_slice(&f.id);
    for at in [32, 36, 40] {
        wr32(&mut p, at, u32::MAX);
    }
    for at in [46, 50, 63, 71] {
        wr32(&mut p, at, 1_000_000);
    }
    wr64(&mut p, 55, 1_000_000);
    p[75..107].copy_from_slice(f.providers[0].pubkey().as_ref());
    if let Some(generation) = generation {
        p.extend_from_slice(&generation.to_le_bytes());
    }
    p
}
fn booking_accounts(v: &Vm, f: &Fixture, b: Address, fresh: bool) -> Vec<AccountMeta> {
    vec![
        r(f.market),
        r(f.model),
        r(f.terms),
        w(f.capacity),
        w(f.user),
        s(f.owner.pubkey()),
        a(b, true, fresh),
        s(v.payer.pubkey()),
        r(f.list.pubkey()),
    ]
}
fn claim_accounts(f: &Fixture, b: Address, slot: usize) -> Vec<AccountMeta> {
    vec![
        r(f.market),
        r(f.model),
        w(f.capacity),
        w(f.user),
        w(b),
        s(f.providers[slot].pubkey()),
        w(f.vault),
        w(f.payouts[slot]),
        r(f.mint),
        r(f.authority),
        r(TOKEN),
    ]
}
fn claim(v: &Vm, f: &Fixture, b: Address, usage: [u32; 3]) -> Vec<u8> {
    let bd = v.data(b);
    let mut p = vec![0; 140];
    for (i, n) in usage.iter().enumerate() {
        wr32(&mut p, i * 4, *n);
    }
    let mut cost: u64 = usage
        .iter()
        .enumerate()
        .map(|(i, n)| (u64::from(*n) * u64::from(rd32(&bd, 112 + i * 4))).div_ceil(1_000_000))
        .sum();
    if bd.len() == FEE_BOOKING_LEN {
        let bps = u16::from_le_bytes(bd[432..434].try_into().unwrap());
        cost += (cost * u64::from(bps)).div_ceil(10_000);
    }
    p[12..20].copy_from_slice(&bd[152..160]);
    p[20..52].copy_from_slice(f.owner.pubkey().as_ref());
    p[52..84].copy_from_slice(f.user.as_ref());
    p[84..116].copy_from_slice(b.as_ref());
    p[116..124].copy_from_slice(&bd[152..160]);
    wr64(&mut p, 124, cost);
    p[132..140].copy_from_slice(&bd[132..140]);
    p
}

#[path = "economics.rs"]
mod economics;
pub fn economics() {
    economics::run();
}
fn reconcile(v: &Vm, f: &Fixture, open: &[Address], paid: u64) {
    let reserved: u64 = open.iter().map(|key| rd64(&v.data(*key), 104)).sum();
    assert_eq!(
        rd64(&v.data(f.user), 72) + reserved,
        rd64(&v.data(f.vault), 64)
    );
    assert_eq!(rd64(&v.data(f.vault), 64) + paid, 1_000_000);
    assert_eq!(
        (0..3)
            .map(|slot| rd32(&v.data(f.capacity), HEADER_LEN + slot * CAPACITY_LEN + 16) as usize)
            .sum::<usize>(),
        open.len()
    );
    for slot in 0..3 {
        let c = v.data(f.capacity);
        let at = HEADER_LEN + slot * CAPACITY_LEN;
        assert!(c[at..at + 16]
            .iter()
            .chain(c[at + 20..at + 32].iter())
            .all(|b| *b == 0));
    }
}

pub fn run(legacy_token: bool) {
    parity::fixture(format!("router_protocol_legacy={legacy_token}"));
    let mut v = Vm::new(legacy_token, false);
    let f = setup(&mut v);
    let payer = v.payer.pubkey();
    for op in [4, 9, 11, 12, 13, 14, 16, 20, 23, 24, 25, 27, 31, 255] {
        v.send(op, &[], vec![], &[], false);
    }
    let b = Keypair::new();
    v.blank(&b, BOOKING_LEN);
    let p = request(&f, Some(1));
    v.send(
        6,
        &p,
        booking_accounts(&v, &f, b.pubkey(), true),
        &[&b, &f.owner],
        false,
    );
    let mut invalid = booking_accounts(&v, &f, b.pubkey(), true);
    invalid.pop();
    v.send(6, &p, invalid, &[&b, &f.owner], false);
    let foreign = Keypair::new();
    v.blank(&foreign, provider_list_len(1));
    let mut fake = v.data(f.list.pubkey());
    fake.resize(provider_list_len(1), 0);
    wr32(&mut fake, 72, 1);
    fake[8..40].fill(9);
    v.replace(foreign.pubkey(), fake);
    let mut invalid = booking_accounts(&v, &f, b.pubkey(), true);
    invalid[8] = r(foreign.pubkey());
    v.send(6, &p, invalid, &[&b, &f.owner], false);
    member(&mut v, &f, f.providers[1].pubkey(), true, true);
    member(&mut v, &f, f.providers[1].pubkey(), true, false);
    member(&mut v, &f, f.providers[0].pubkey(), false, false);
    let mut membership = f.providers[0].pubkey().to_bytes().to_vec();
    membership.push(1);
    v.send(
        29,
        &membership,
        vec![r(f.market), w(f.list.pubkey()), s(f.owner.pubkey())],
        &[&f.owner],
        false,
    );
    v.send(
        30,
        &[],
        vec![
            w(f.list.pubkey()),
            ws(f.manager.pubkey()),
            w(list_metadata_address(&v, f.list.pubkey())),
        ],
        &[&f.manager],
        false,
    );
    let reserve_cu = v.send(
        6,
        &p,
        booking_accounts(&v, &f, b.pubkey(), true),
        &[&b, &f.owner],
        true,
    );
    let bd = v.data(b.pubkey());
    assert_eq!(rd32(&bd, 352), 1);
    assert_eq!(&bd[320..352], f.list.pubkey().as_ref());
    assert_eq!(&bd[140..148], &[0; 8]);
    assert_eq!(bd[356], 0);
    reconcile(&v, &f, &[b.pubkey()], 0);
    v.send(
        6,
        &p,
        booking_accounts(&v, &f, b.pubkey(), false),
        &[&f.owner],
        false,
    );
    member(&mut v, &f, f.providers[1].pubkey(), false, true);
    member(&mut v, &f, f.providers[0].pubkey(), true, true);
    let good = claim(&v, &f, b.pubkey(), [100, 0, 100]);
    for at in [12, 20, 52, 84, 116, 124, 132] {
        let mut bad = good.clone();
        bad[at] ^= 1;
        v.send(
            7,
            &bad,
            claim_accounts(&f, b.pubkey(), 1),
            &[&f.providers[1]],
            false,
        );
    }
    for length in [0, 11, 12, 20, 139, 141, 156] {
        let mut bad = good.clone();
        bad.resize(length, 0);
        v.send(
            7,
            &bad,
            claim_accounts(&f, b.pubkey(), 1),
            &[&f.providers[1]],
            false,
        );
    }
    let bad = claim(&v, &f, b.pubkey(), [1_000_001, 0, 0]);
    v.send(
        7,
        &bad,
        claim_accounts(&f, b.pubkey(), 1),
        &[&f.providers[1]],
        false,
    );
    let bad = claim(&v, &f, b.pubkey(), [0, 1, 0]);
    v.send(
        7,
        &bad,
        claim_accounts(&f, b.pubkey(), 1),
        &[&f.providers[1]],
        false,
    );
    v.send(
        7,
        &good,
        claim_accounts(&f, b.pubkey(), 0),
        &[&f.providers[0]],
        false,
    );
    let mut frozen = v.data(f.payouts[1]);
    frozen[108] = 2;
    v.replace(f.payouts[1], frozen.clone());
    v.send(
        7,
        &good,
        claim_accounts(&f, b.pubkey(), 1),
        &[&f.providers[1]],
        false,
    );
    frozen[108] = 1;
    v.replace(f.payouts[1], frozen);
    let claim_cu = v.send(
        7,
        &good,
        claim_accounts(&f, b.pubkey(), 1),
        &[&f.providers[1]],
        true,
    );
    assert_eq!(v.data(b.pubkey())[148], 5);
    let paid = rd64(&v.data(f.payouts[1]), 64);
    assert_eq!(paid, 2);
    reconcile(&v, &f, &[], paid);
    v.send(
        7,
        &good,
        claim_accounts(&f, b.pubkey(), 1),
        &[&f.providers[1]],
        false,
    );
    v.send(
        6,
        &p,
        booking_accounts(&v, &f, b.pubkey(), false),
        &[&f.owner],
        false,
    );
    let reused = request(&f, Some(2));
    v.send(
        6,
        &reused,
        booking_accounts(&v, &f, b.pubkey(), false),
        &[&f.owner],
        true,
    );
    assert_eq!(rd32(&v.data(b.pubkey()), 352), 0);
    reconcile(&v, &f, &[b.pubkey()], paid);
    v.send(
        7,
        &good,
        claim_accounts(&f, b.pubkey(), 1),
        &[&f.providers[1]],
        false,
    );
    let release = vec![
        r(f.market),
        r(f.model),
        w(f.capacity),
        w(f.user),
        w(b.pubkey()),
        s(f.providers[0].pubkey()),
    ];
    v.send(
        15,
        &1u64.to_le_bytes(),
        release.clone(),
        &[&f.providers[0]],
        false,
    );
    v.send(15, &2u64.to_le_bytes(), release, &[&f.providers[0]], true);
    reconcile(&v, &f, &[], paid);
    // legacy idle bookings keep their address, owner and counters; reuse clears obsolete bytes.
    let mut old = v.data(b.pubkey());
    wr64(&mut old, 140, 123);
    old[320..352].fill(9);
    old[356] = 250;
    v.replace(b.pubkey(), old);
    let p = request(&f, Some(3));
    v.send(
        6,
        &p,
        booking_accounts(&v, &f, b.pubkey(), false),
        &[&f.owner],
        true,
    );
    let current = v.data(b.pubkey());
    assert_eq!(&current[140..148], &[0; 8]);
    assert_eq!(current[356], 0);
    let zero = claim(&v, &f, b.pubkey(), [0, 0, 0]);
    v.send(
        7,
        &zero,
        claim_accounts(&f, b.pubkey(), 0),
        &[&f.providers[0]],
        true,
    );
    reconcile(&v, &f, &[], paid);
    let p = request(&f, Some(4));
    v.send(
        6,
        &p,
        booking_accounts(&v, &f, b.pubkey(), false),
        &[&f.owner],
        true,
    );
    let expire = vec![
        r(f.market),
        r(f.model),
        w(f.capacity),
        w(f.user),
        w(b.pubkey()),
    ];
    v.send(8, &4u64.to_le_bytes(), expire.clone(), &[], false);
    let deadline = rd64(&v.data(b.pubkey()), 132) as i64;
    let mut clock = v.svm.get_sysvar::<Clock>();
    clock.unix_timestamp = deadline;
    v.svm.set_sysvar(&clock);
    v.send(8, &4u64.to_le_bytes(), expire.clone(), &[], false);
    clock.unix_timestamp += 1;
    v.svm.set_sysvar(&clock);
    v.send(
        7,
        &claim(&v, &f, b.pubkey(), [1, 0, 1]),
        claim_accounts(&f, b.pubkey(), 0),
        &[&f.providers[0]],
        false,
    );
    v.send(8, &4u64.to_le_bytes(), expire, &[], true);
    reconcile(&v, &f, &[], paid);
    let rent = v.svm.get_account(&b.pubkey()).unwrap().lamports;
    v.send(
        17,
        &[],
        vec![w(b.pubkey()), w(f.owner.pubkey())],
        &[],
        false,
    );
    v.send(17, &[], vec![w(b.pubkey()), w(payer)], &[], true);
    assert!(v
        .svm
        .get_account(&b.pubkey())
        .is_none_or(|a| a.lamports == 0));
    assert!(rent > 0);
    // every list remains independently owned; removals preserve sorting and zero unused slots.
    for provider in [f.providers[2].pubkey(), f.providers[1].pubkey()] {
        member(&mut v, &f, provider, true, true);
    }
    let list = v.data(f.list.pubkey());
    let keys: Vec<_> = list[80..176].chunks_exact(32).collect();
    assert!(keys.windows(2).all(|pair| pair[0] < pair[1]));
    for provider in &f.providers {
        member(&mut v, &f, provider.pubkey(), false, true);
    }
    let list = v.data(f.list.pubkey());
    assert_eq!(rd32(&list, 76), 0);
    assert!(list[80..].iter().all(|b| *b == 0));
    let before = v.svm.get_account(&f.manager.pubkey()).unwrap().lamports;
    let rent = v.svm.get_account(&f.list.pubkey()).unwrap().lamports;
    v.send(
        30,
        &[],
        vec![
            w(f.list.pubkey()),
            ws(f.owner.pubkey()),
            w(list_metadata_address(&v, f.list.pubkey())),
        ],
        &[&f.owner],
        false,
    );
    v.send(
        30,
        &[],
        vec![
            w(f.list.pubkey()),
            ws(f.manager.pubkey()),
            w(list_metadata_address(&v, f.list.pubkey())),
        ],
        &[&f.manager],
        true,
    );
    assert_eq!(
        v.svm.get_account(&f.manager.pubkey()).unwrap().lamports,
        before + rent
    );
    let free = rd64(&v.data(f.user), 72);
    v.send(
        10,
        &free.to_le_bytes(),
        vec![
            r(f.market),
            w(f.user),
            s(f.owner.pubkey()),
            w(f.vault),
            w(f.source),
            r(f.mint),
            r(f.authority),
            r(TOKEN),
        ],
        &[&f.owner],
        true,
    );
    assert_eq!(rd64(&v.data(f.user), 72), 0);
    assert_eq!(rd64(&v.data(f.vault), 64), 0);
    assert_eq!(rd64(&v.data(f.source), 64) + paid, 1_000_000);
    println!("protocol passed: token_legacy={legacy_token} executions={} reserve_cu={reserve_cu} claim_cu={claim_cu}; list auth/filtering, permissionless registration, replay, receipts, deadlines, rollback and custody",v.executions);
}

pub fn migration(legacy_token: bool) {
    parity::fixture(format!("router_migration_legacy={legacy_token}"));
    let mut v = Vm::new(legacy_token, true);
    let f = setup(&mut v);
    let slot = 0u32;
    let (authority, bump) = Address::find_program_address(
        &[b"bond", f.model.as_ref(), &slot.to_le_bytes()],
        &v.program,
    );
    let vault = v.token(f.mint, authority, 47);
    let mut terms = v.data(f.terms);
    terms[HEADER_LEN + 64..HEADER_LEN + 96].copy_from_slice(vault.as_ref());
    terms[HEADER_LEN + 112] = 1;
    terms[HEADER_LEN + 113] = bump;
    v.replace(f.terms, terms);
    let accounts = vec![
        r(f.market),
        r(f.model),
        w(f.terms),
        w(f.capacity),
        ws(f.providers[0].pubkey()),
        w(vault),
        w(f.payouts[0]),
        r(f.mint),
        r(authority),
        r(TOKEN),
    ];
    let original = v.data(f.capacity);
    for offset in [0, 8, 16, 20, 24] {
        let mut c = original.clone();
        c[HEADER_LEN + offset] = 1;
        v.replace(f.capacity, c);
        v.send(
            31,
            &slot.to_le_bytes(),
            accounts.clone(),
            &[&f.providers[0]],
            false,
        );
    }
    v.replace(f.capacity, original);
    for (index, key) in [
        (4, f.providers[1].pubkey()),
        (5, f.vault),
        (6, f.payouts[1]),
        (8, f.authority),
    ] {
        let mut wrong = accounts.clone();
        wrong[index].pubkey = key;
        let signer = if index == 4 {
            &f.providers[1]
        } else {
            &f.providers[0]
        };
        v.send(31, &slot.to_le_bytes(), wrong, &[signer], false);
    }
    let before = v
        .svm
        .get_account(&f.providers[0].pubkey())
        .unwrap()
        .lamports;
    let rent = v.svm.get_account(&vault).unwrap().lamports;
    v.send(
        31,
        &slot.to_le_bytes(),
        accounts.clone(),
        &[&f.providers[0]],
        true,
    );
    assert_eq!(rd64(&v.data(f.payouts[0]), 64), 47);
    assert_eq!(
        v.svm
            .get_account(&f.providers[0].pubkey())
            .unwrap()
            .lamports,
        before + rent
    );
    assert!(v.svm.get_account(&vault).is_none_or(|a| a.lamports == 0));
    assert_eq!(&v.data(f.terms)[HEADER_LEN + 64..HEADER_LEN + 96], &[0; 32]);
    v.send(31, &slot.to_le_bytes(), accounts, &[&f.providers[0]], false);
    println!("legacy sweep passed: token_legacy={legacy_token}, wrong authorities/vault/payout and every outstanding liability rejected; donated tokens and rent recovered");
}

fn tree_fixture(d: &mut [u8]) {
    let limit = rd32(d, 72) as usize;
    let count = rd32(d, 76) as usize;
    let keys = d[80..80 + count * 32].to_vec();
    d[80 + limit * 32..].fill(0);
    fn visit(d: &mut [u8], keys: &[u8], node: usize, at: &mut usize, limit: usize) {
        if node >= keys.len() / 32 {
            return;
        }
        visit(d, keys, node * 2 + 1, at, limit);
        for word in 0..4 {
            let value = u64::from_be_bytes(
                keys[*at * 32 + word * 8..*at * 32 + word * 8 + 8]
                    .try_into()
                    .unwrap(),
            );
            wr64(d, 80 + limit * 32 + node * 32 + word * 8, value);
        }
        *at += 1;
        visit(d, keys, node * 2 + 2, at, limit);
    }
    visit(d, &keys, 0, &mut 0, limit);
}

fn scale_case(count: usize, list_count: usize, mode: u8, cached: bool) {
    scale_with_token(count, list_count, mode, cached, false, false)
}
fn scale_with_token(
    count: usize,
    list_count: usize,
    mode: u8,
    cached: bool,
    legacy: bool,
    fees: bool,
) {
    parity::fixture(format!("router_scale_offers={count}_members={list_count}_mode={mode}_cached={cached}_legacy={legacy}_fees={fees}"));
    let mut v = Vm::new(legacy, false);
    let mut f = setup(&mut v);
    let mut keys = vec![[0u8; 32]; list_count];
    for (slot, key) in keys.iter_mut().enumerate() {
        let offset = if mode == 0 { 0 } else { 28 };
        key[offset..offset + 4].copy_from_slice(&(slot as u32 * 2 + 2).to_be_bytes());
        if mode == 8 {
            key[28..].copy_from_slice(
                &(if slot == 0 {
                    2
                } else {
                    (slot as u32 + 1) * 1_000_000
                })
                .to_be_bytes(),
            );
        }
        if matches!(mode, 2 | 5 | 6 | 7 | 8) && slot >= list_count - 7 {
            key.fill(0);
            let offset = (slot - (list_count - 7)) * 4;
            key[offset..offset + 4].copy_from_slice(&1u32.to_be_bytes());
        }
    }
    keys.sort_unstable();
    let mut terms = v.data(f.terms);
    let mut capacity = v.data(f.capacity);
    wr32(&mut terms, 44, count as u32);
    wr32(&mut capacity, 44, count as u32);
    for slot in 0..count {
        let at = HEADER_LEN + slot * TERMS_LEN;
        terms[at..at + TERMS_LEN].fill(0);
        let key = if mode >= 2 && mode != 7 && slot > 0 {
            let mut key = [0; 32];
            let value = match mode {
                2 => list_count as u32 * 4 + slot as u32,
                3 | 5 => 1,
                6 => 3,
                8 => slot as u32 + 2,
                _ => slot as u32 * 2 + 1,
            };
            key[28..].copy_from_slice(&value.to_be_bytes());
            key
        } else {
            keys[(slot * 4051) % list_count]
        };
        terms[at..at + 32].copy_from_slice(&key);
        terms[at + 32..at + 64].copy_from_slice(f.payouts[0].as_ref());
        let prices = offer(
            slot as u32,
            [(count - slot) as u32, 0, (count - slot) as u32],
        );
        terms[at + 96..at + 112].copy_from_slice(&prices[4..20]);
        wr32(&mut terms, at + 116, 4);
    }
    v.replace(f.terms, terms.clone());
    v.replace(f.capacity, capacity);
    let list = Keypair::new();
    v.blank(&list, provider_list_len(list_count));
    let initialize = v.send(
        28,
        &(list_count as u32).to_le_bytes(),
        vec![r(f.market), ws(list.pubkey()), s(f.manager.pubkey())],
        &[&list, &f.manager],
        true,
    );
    let mut members = v.data(list.pubkey());
    wr32(&mut members, 76, list_count as u32);
    for (slot, key) in keys.iter().enumerate() {
        members[80 + slot * 32..112 + slot * 32].copy_from_slice(key);
    }
    tree_fixture(&mut members);
    v.replace(list.pubkey(), members.clone());
    f.list = list;
    let mut mutation = keys[0].to_vec();
    mutation.push(0);
    let remove = v.send(
        29,
        &mutation,
        vec![r(f.market), w(f.list.pubkey()), s(f.manager.pubkey())],
        &[&f.manager],
        true,
    );
    mutation[32] = 1;
    let insert = v.send(
        29,
        &mutation,
        vec![r(f.market), w(f.list.pubkey()), s(f.manager.pubkey())],
        &[&f.manager],
        true,
    );
    assert_eq!(v.data(f.list.pubkey()), members);
    if fees {
        let admin = v.payer.pubkey();
        let mut cfg = vec![0; 137];
        cfg[..32].copy_from_slice(admin.as_ref());
        cfg[32..64].copy_from_slice(key().as_ref());
        v.send(
            35,
            &cfg,
            vec![w(f.market), s(admin), ws(admin), r(SYSTEM)],
            &[],
            true,
        );
        let p = [300u16.to_le_bytes(), 5000u16.to_le_bytes()].concat();
        v.send(
            36,
            &p,
            vec![
                r(f.market),
                w(f.list.pubkey()),
                s(f.manager.pubkey()),
                ws(admin),
                r(SYSTEM),
            ],
            &[&f.manager],
            true,
        );
    }
    let b = Keypair::new();
    v.blank(&b, if fees { FEE_BOOKING_LEN } else { BOOKING_LEN });
    let mut p = request(&f, Some(1));
    if cached {
        wr32(&mut p, 67, 500_000);
        p[75..107].copy_from_slice(
            &terms[HEADER_LEN + (count - 1) * TERMS_LEN..HEADER_LEN + (count - 1) * TERMS_LEN + 32],
        );
    }
    let prefix = if legacy {
        let mut source = v.data(f.source);
        wr64(&mut source, 64, 1);
        v.replace(f.source, source);
        let mut data = vec![3];
        data.extend_from_slice(&1u64.to_le_bytes());
        vec![Instruction {
            program_id: v.program,
            accounts: vec![
                r(f.market),
                w(f.user),
                s(f.owner.pubkey()),
                w(f.source),
                w(f.vault),
                r(f.mint),
                r(TOKEN),
            ],
            data,
        }]
    } else {
        vec![]
    };
    let cu = v.send_with_prefix(
        6,
        &p,
        booking_accounts(&v, &f, b.pubkey(), true),
        &[&b, &f.owner],
        true,
        prefix,
    );
    assert_eq!(
        rd32(&v.data(b.pubkey()), 352),
        if mode >= 2 && mode != 7 {
            0
        } else {
            count as u32 - 1
        }
    );
    let budget = (20_000 + 500 * count as u64).min(1_400_000);
    assert!(cu <= budget, "reserve_cu={cu}, budget={budget}");
    println!("scale offers={count} members={list_count} mode={mode} cached={cached} legacy_deposit={legacy} fees={fees}: reserve={cu} init={initialize} remove={remove} insert={insert}");
}

pub fn scale() {
    for mode in 0..=5 {
        scale_case(4096, 4096, mode, true);
    }
    scale_case(4096, 4096, 5, false);
    scale_case(4096, 4096, 6, false);
    scale_case(4096, 4096, 7, true);
    scale_case(4096, 4096, 7, false);
    scale_case(4096, 4096, 8, true);
    scale_case(4095, 4095, 8, false);
    scale_with_token(4096, 4096, 7, true, true, false);
    scale_with_token(4096, 4096, 7, true, false, true);
    scale_with_token(4096, 4096, 7, true, true, true);
    for count in [1, 16, 256] {
        scale_case(count, 4096, 1, true);
    }
}

pub fn growth() {
    parity::fixture("router_growth");
    let mut v = Vm::new(false, false);
    let f = setup(&mut v);
    let payer = Keypair::new();
    v.svm.airdrop(&payer.pubkey(), 10_000_000_000).unwrap();
    let mut model = v.data(f.model);
    wr32(&mut model, 136, 4);
    v.replace(f.model, model.clone());
    for (address, size) in [
        (f.terms, terms_book_len(4)),
        (f.capacity, capacity_book_len(4)),
    ] {
        let mut d = v.data(address);
        d.truncate(size);
        wr32(&mut d, 40, 4);
        v.put(address, v.program, d);
    }
    let terms = v.data(f.terms);
    let capacity = v.data(f.capacity);
    let user = v.data(f.user);
    let accounts = vec![
        r(f.market),
        w(f.model),
        w(f.terms),
        w(f.capacity),
        ws(payer.pubkey()),
        r(SYSTEM),
    ];
    for limit in [0u32, 3, 4, 90, 4097, u32::MAX] {
        v.send(32, &limit.to_le_bytes(), accounts.clone(), &[&payer], false);
    }
    for index in 1..=4 {
        let mut invalid = accounts.clone();
        invalid[index].is_writable = false;
        v.send(32, &5u32.to_le_bytes(), invalid, &[&payer], false);
    }
    let mut invalid = accounts.clone();
    invalid[4].is_signer = false;
    v.send(32, &5u32.to_le_bytes(), invalid, &[], false);
    let mut invalid = accounts.clone();
    invalid[5] = r(TOKEN);
    v.send(32, &5u32.to_le_bytes(), invalid, &[&payer], false);
    let foreign = key();
    v.put(foreign, v.program, terms.clone());
    let mut invalid = accounts.clone();
    invalid[2] = w(foreign);
    v.send(32, &5u32.to_le_bytes(), invalid, &[&payer], false);
    let foreign_market = key();
    v.put(foreign_market, v.program, v.data(f.market));
    let mut invalid = accounts.clone();
    invalid[0] = r(foreign_market);
    v.send(32, &5u32.to_le_bytes(), invalid, &[&payer], false);
    let poor = Keypair::new();
    v.svm.airdrop(&poor.pubkey(), 1_000_000).unwrap();
    let mut invalid = accounts.clone();
    invalid[4] = ws(poor.pubkey());
    v.send(32, &89u32.to_le_bytes(), invalid, &[&poor], false);
    let before = v.svm.get_account(&payer.pubkey()).unwrap().lamports;
    let rent = v.svm.minimum_balance_for_rent_exemption(terms_book_len(89))
        + v.svm
            .minimum_balance_for_rent_exemption(capacity_book_len(89))
        - v.svm.minimum_balance_for_rent_exemption(terms_book_len(4))
        - v.svm
            .minimum_balance_for_rent_exemption(capacity_book_len(4));
    let cu = v.send(32, &89u32.to_le_bytes(), accounts.clone(), &[&payer], true);
    assert_eq!(
        v.svm.get_account(&payer.pubkey()).unwrap().lamports,
        before - rent
    );
    wr32(&mut model, 136, 89);
    assert_eq!(v.data(f.model), model);
    for (address, mut expected, size) in [
        (f.terms, terms, terms_book_len(89)),
        (f.capacity, capacity, capacity_book_len(89)),
    ] {
        wr32(&mut expected, 40, 89);
        expected.resize(size, 0);
        assert_eq!(v.data(address), expected);
        assert_eq!(
            v.svm.get_account(&address).unwrap().lamports,
            v.svm.minimum_balance_for_rent_exemption(size)
        );
    }
    assert_eq!(v.data(f.user), user);
    v.send(32, &89u32.to_le_bytes(), accounts.clone(), &[&payer], false);
    let next = v.send(32, &90u32.to_le_bytes(), accounts, &[&payer], true);
    println!("permissionless growth preserved entries/metadata/user and zeroed extension: 85slots={cu} one_slot={next}; bounds, foreign accounts, readonly, signer and payer failures rollback");
}

pub fn list_metadata() {
    parity::fixture("router_list_metadata");
    let mut v = Vm::new(false, false);
    let f = setup(&mut v);
    let list = f.list.pubkey();
    let authority = f.manager.pubkey();
    let profile = list_metadata_address(&v, list);
    let sponsor = Keypair::new();
    v.svm.airdrop(&sponsor.pubkey(), 50_000_000).unwrap();
    let init = vec![
        r(f.market),
        r(list),
        w(profile),
        s(authority),
        ws(sponsor.pubkey()),
        r(SYSTEM),
    ];
    let update = vec![r(f.market), r(list), w(profile), s(authority)];
    let close = vec![w(list), ws(authority), w(profile)];
    let signers = [&f.manager, &sponsor];
    let payload = metadata();
    let list_before = v.data(list);
    v.send(34, &payload, update.clone(), &[&f.manager], false);
    for index in [2, 4] {
        let mut invalid = init.clone();
        invalid[index].is_writable = false;
        v.send(33, &payload, invalid, &signers, false);
    }
    for index in [3, 4] {
        let mut invalid = init.clone();
        invalid[index].is_signer = false;
        let keys = if index == 3 {
            vec![&sponsor]
        } else {
            vec![&f.manager]
        };
        v.send(33, &payload, invalid, &keys, false);
    }
    for index in [0, 1, 2, 5] {
        let mut invalid = init.clone();
        invalid[index].pubkey = key();
        v.send(33, &payload, invalid, &signers, false);
    }
    let mut invalid = init.clone();
    invalid[3] = s(f.owner.pubkey());
    v.send(33, &payload, invalid, &[&f.owner, &sponsor], false);
    let mut wrong_market = list_before.clone();
    wrong_market[8..40].fill(9);
    v.replace(list, wrong_market);
    v.send(33, &payload, init.clone(), &signers, false);
    v.replace(list, list_before.clone());
    for invalid in [
        vec![],
        vec![0; 32],
        vec![0; 289],
        [vec![1; 32], vec![0xff]].concat(),
        [vec![1; 32], b"x y".to_vec()].concat(),
    ] {
        v.send(33, &invalid, init.clone(), &signers, false);
    }
    for count in [5, 7] {
        let mut invalid = init.clone();
        invalid.resize(count, r(f.mint));
        v.send(33, &payload, invalid, &signers, false);
    }
    for (owner, data) in [
        (v.program, vec![]),
        (key(), vec![]),
        (SYSTEM, vec![0]),
        (v.program, vec![0; LIST_METADATA_LEN]),
    ] {
        v.put(profile, owner, data);
        v.send(33, &payload, init.clone(), &signers, false);
        v.send(30, &[], close.clone(), &[&f.manager], false);
    }
    // donated lamports do not block initialization; only the rent deficit is charged.
    v.put(profile, SYSTEM, vec![]);
    let prefunded = v.svm.get_account(&profile).unwrap().lamports;
    let sponsor_before = v.svm.get_account(&sponsor.pubkey()).unwrap().lamports;
    let rent = v.svm.minimum_balance_for_rent_exemption(LIST_METADATA_LEN);
    let init_cu = v.send(33, &payload, init.clone(), &signers, true);
    assert_eq!(
        v.svm.get_account(&sponsor.pubkey()).unwrap().lamports,
        sponsor_before - (rent - prefunded)
    );
    let original = v.svm.get_account(&profile).unwrap();
    let d = &original.data;
    assert_eq!(d.len(), LIST_METADATA_LEN);
    assert_eq!(&d[..8], LIST_METADATA_MAGIC);
    assert_eq!(&d[8..40], f.market.as_ref());
    assert_eq!(&d[40..72], list.as_ref());
    assert_eq!(&d[72..104], authority.as_ref());
    assert_eq!(&d[104..136], &payload[..32]);
    assert_eq!(
        d[138],
        Address::find_program_address(&[b"list_metadata", list.as_ref()], &v.program).1
    );
    assert_eq!(d[139], 0);
    assert_eq!(v.data(list), list_before);
    v.send(33, &payload, init, &signers, false);
    for index in [0, 1, 2, 3] {
        let mut invalid = update.clone();
        invalid[index] = if index == 3 {
            s(f.owner.pubkey())
        } else {
            r(key())
        };
        let signer = if index == 3 { &f.owner } else { &f.manager };
        v.send(34, &payload, invalid, &[signer], false);
    }
    let mut invalid = update.clone();
    invalid[3].is_signer = false;
    v.send(34, &payload, invalid, &[], false);
    let mut invalid = update.clone();
    invalid[2].is_writable = false;
    v.send(34, &payload, invalid, &[&f.manager], false);
    for count in [3, 5] {
        let mut invalid = update.clone();
        invalid.resize(count, r(f.mint));
        let keys = if count == 3 { vec![] } else { vec![&f.manager] };
        v.send(34, &payload, invalid, &keys, false);
    }
    for invalid in [
        vec![],
        vec![0; 32],
        vec![0; 289],
        [vec![1; 32], b"x\ny".to_vec()].concat(),
    ] {
        v.send(34, &invalid, update.clone(), &[&f.manager], false);
    }
    // account identity and malformed stored metadata fail closed for updates and closure.
    for at in [0, 8, 40, 72, 138, 139, 140, LIST_METADATA_LEN - 1] {
        let mut corrupt = original.clone();
        corrupt.data[at] ^= 0xff;
        v.svm.set_account(profile, corrupt).unwrap();
        v.send(34, &payload, update.clone(), &[&f.manager], false);
        v.send(30, &[], close.clone(), &[&f.manager], false);
    }
    for length in [0u16, 257, u16::MAX] {
        let mut corrupt = original.clone();
        corrupt.data[136..138].copy_from_slice(&length.to_le_bytes());
        v.svm.set_account(profile, corrupt).unwrap();
        v.send(34, &payload, update.clone(), &[&f.manager], false);
        v.send(30, &[], close.clone(), &[&f.manager], false);
    }
    for (owner, length) in [
        (key(), LIST_METADATA_LEN),
        (SYSTEM, LIST_METADATA_LEN),
        (v.program, LIST_METADATA_LEN - 1),
        (v.program, LIST_METADATA_LEN + 1),
    ] {
        let mut corrupt = original.clone();
        corrupt.owner = owner;
        corrupt.data.resize(length, 0);
        v.svm.set_account(profile, corrupt).unwrap();
        v.send(34, &payload, update.clone(), &[&f.manager], false);
        v.send(30, &[], close.clone(), &[&f.manager], false);
    }
    v.svm.set_account(profile, original.clone()).unwrap();
    let mut longest = vec![2; 32];
    longest.extend_from_slice("é".repeat(128).as_bytes());
    let update_cu = v.send(34, &longest, update.clone(), &[&f.manager], true);
    let shortest = [vec![0; 32], vec![b'x']].concat();
    v.send(34, &shortest, update, &[&f.manager], true);
    let d = v.data(profile);
    assert_eq!(&d[..104], &original.data[..104]);
    assert_eq!(&d[104..136], &[0; 32]);
    assert_eq!(&d[136..138], &[1, 0]);
    assert_eq!(&d[138..140], &original.data[138..140]);
    assert_eq!(d[140], b'x');
    assert!(d[141..].iter().all(|b| *b == 0));
    assert_eq!(v.data(list), list_before);
    member(&mut v, &f, f.providers[0].pubkey(), true, true);
    v.send(30, &[], close.clone(), &[&f.manager], false);
    member(&mut v, &f, f.providers[0].pubkey(), false, true);
    for index in [0, 1, 2] {
        let mut invalid = close.clone();
        invalid[index].is_writable = false;
        v.send(30, &[], invalid, &[&f.manager], false);
    }
    let mut invalid = close.clone();
    invalid[1].is_signer = false;
    v.send(30, &[], invalid, &[], false);
    let mut invalid = close.clone();
    invalid[2].pubkey = key();
    v.send(30, &[], invalid, &[&f.manager], false);
    v.send(30, &[], close[..2].to_vec(), &[&f.manager], false);
    v.send(30, &[0], close.clone(), &[&f.manager], false);
    let authority_before = v.svm.get_account(&authority).unwrap().lamports;
    let sponsor_before = v.svm.get_account(&sponsor.pubkey()).unwrap().lamports;
    let list_rent = v.svm.get_account(&list).unwrap().lamports;
    let close_cu = v.send(30, &[], close, &[&f.manager], true);
    assert_eq!(
        v.svm.get_account(&authority).unwrap().lamports,
        authority_before + list_rent + rent
    );
    assert_eq!(
        v.svm.get_account(&sponsor.pubkey()).unwrap().lamports,
        sponsor_before
    );
    assert!(v.svm.get_account(&profile).is_none_or(|a| a.lamports == 0));
    assert!(v.svm.get_account(&list).is_none_or(|a| a.lamports == 0));
    // legacy lists without metadata also close when the canonical PDA has system-owned dust.
    v.blank(&f.list, provider_list_len(4));
    v.send(
        28,
        &4u32.to_le_bytes(),
        vec![r(f.market), ws(list), s(authority)],
        &[&f.list, &f.manager],
        true,
    );
    v.put(profile, SYSTEM, vec![]);
    let dust = v.svm.get_account(&profile).unwrap();
    v.send(
        30,
        &[],
        vec![w(list), ws(authority), w(profile)],
        &[&f.manager],
        true,
    );
    let after = v.svm.get_account(&profile).unwrap();
    assert_eq!(after.lamports, dust.lamports);
    assert_eq!(after.owner, dust.owner);
    assert!(after.data.is_empty());
    let maximum = Keypair::new();
    let maximum_profile = list_metadata_address(&v, maximum.pubkey());
    let len = provider_list_len(MAX_ENTRIES);
    let mut create = 0u32.to_le_bytes().to_vec();
    create.extend_from_slice(&v.svm.minimum_balance_for_rent_exemption(len).to_le_bytes());
    create.extend_from_slice(&(len as u64).to_le_bytes());
    create.extend_from_slice(v.program.as_ref());
    let mut initialize = vec![28];
    initialize.extend_from_slice(&(MAX_ENTRIES as u32).to_le_bytes());
    let combined_cu = v.send_with_prefix(
        33,
        &longest,
        vec![
            r(f.market),
            r(maximum.pubkey()),
            w(maximum_profile),
            s(authority),
            ws(sponsor.pubkey()),
            r(SYSTEM),
        ],
        &[&maximum, &f.manager, &sponsor],
        true,
        vec![
            Instruction {
                program_id: SYSTEM,
                accounts: vec![ws(v.payer.pubkey()), ws(maximum.pubkey())],
                data: create,
            },
            Instruction {
                program_id: v.program,
                accounts: vec![r(f.market), ws(maximum.pubkey()), s(authority)],
                data: initialize,
            },
        ],
    );
    assert!(
        combined_cu < 250_000,
        "maximum list and metadata creation exceeds CLI budget: {combined_cu}"
    );
    println!("maximum list creation plus 256-byte metadata URI: {combined_cu} CU");
    println!("list metadata passed: executions={} init_cu={init_cu} update_cu={update_cu} close_cu={close_cu}; authority, sponsor, prefunding, malformed accounts, atomic rent cleanup, legacy closure", v.executions);
}
