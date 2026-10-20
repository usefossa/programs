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
mod metadata;
mod receipt_bench;
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
    last_error: Option<String>,
    executions: usize,
    succeeded: [usize; 23],
    rejected: [usize; 23],
    packet_max: [usize; 23],
    claim_seconds: u64,
}
impl Vm {
    fn new(legacy: bool) -> Self {
        let mut svm = LiteSVM::new();
        if legacy {
            let path = std::env::var("LEGACY_TOKEN_SBF")
                .expect("set LEGACY_TOKEN_SBF to a legacy spl-token SBF artifact");
            svm.add_program(TOKEN, &std::fs::read(path).unwrap())
                .unwrap();
        }
        let program = key();
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/deploy/inference_router_program.so");
        let path = std::env::var("FOSSA_SBF")
            .map(std::path::PathBuf::from)
            .unwrap_or(path);
        svm.add_program(program, &std::fs::read(path).unwrap())
            .unwrap();
        let payer = Keypair::new();
        svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
        Self {
            svm,
            program,
            payer,
            last_error: None,
            executions: 0,
            succeeded: [0; 23],
            rejected: [0; 23],
            packet_max: [0; 23],
            claim_seconds: 0,
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
        self.send_with(op, payload, accounts, signers, succeeds, &[])
    }
    fn send_with(
        &mut self,
        op: u8,
        payload: &[u8],
        accounts: Vec<AccountMeta>,
        signers: &[&Keypair],
        succeeds: bool,
        prefix: &[Instruction],
    ) -> u64 {
        self.executions += 1;
        self.last_error = None;
        self.svm.expire_blockhash();
        let before: Vec<_> = accounts
            .iter()
            .filter(|a| a.pubkey != self.payer.pubkey())
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
        ixs.splice(1..1, prefix.iter().cloned());
        let mut keys = vec![&self.payer];
        for signer in signers {
            if signer.pubkey() != self.payer.pubkey() {
                keys.push(*signer);
            }
        }
        let msg = Message::new(&ixs, Some(&self.payer.pubkey()));
        let tx = Transaction::new(&keys, msg, self.svm.latest_blockhash());
        let packet = 1 + 64 * tx.signatures.len() + tx.message.serialize().len();
        assert!(packet <= 1232, "opcode {op} packet size {packet}");
        self.packet_max[op as usize] = self.packet_max[op as usize].max(packet);
        match self.svm.send_transaction(tx) {
            Ok(meta) => {
                self.succeeded[op as usize] += 1;
                assert!(succeeds, "opcode {op} unexpectedly succeeded");
                meta.compute_units_consumed
            }
            Err(error) => {
                self.rejected[op as usize] += 1;
                self.last_error = Some(format!("{:?}", error.err));
                assert!(!succeeds, "opcode {op} failed: {error:?}");
                for (key, account) in before {
                    assert_eq!(self.svm.get_account(&key), account, "rollback opcode {op}");
                }
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
struct Offer {
    provider: Keypair,
    profile: Address,
    payout: Address,
    bond: Address,
    authority: Address,
}
struct Fixture {
    market: Address,
    creator: Keypair,
    profile: Address,
    metadata: Address,
    mint: Address,
    model: Address,
    id: [u8; 32],
    terms: Address,
    capacity: Address,
    owner: Keypair,
    user: Address,
    vault: Address,
    authority: Address,
    offers: Vec<Offer>,
}
fn metadata_payload(hash: u8, uri: &str) -> Vec<u8> {
    let mut p = vec![hash; 32];
    p.extend_from_slice(uri.as_bytes());
    p
}
fn onboard(v: &mut Vm, market: Address, provider: &Keypair) -> Address {
    let profile = Address::find_program_address(
        &[b"provider", market.as_ref(), provider.pubkey().as_ref()],
        &v.program,
    )
    .0;
    v.send(
        19,
        &metadata_payload(1, "https://example.test/provider.json"),
        vec![
            r(market),
            w(profile),
            s(provider.pubkey()),
            ws(v.payer.pubkey()),
            r(SYSTEM),
        ],
        &[provider],
        true,
    );
    v.send(
        20,
        &[1],
        vec![r(market), w(profile), s(v.payer.pubkey())],
        &[],
        true,
    );
    profile
}
fn setup(v: &mut Vm, count: usize, seed: u8) -> Fixture {
    let admin = v.payer.pubkey();
    let market = Keypair::new();
    v.blank(&market, MARKET_LEN);
    let mint = key();
    let mut md = vec![0; 82];
    md[44] = 6;
    md[45] = 1;
    u64at(&mut md, 36, u64::MAX);
    v.put(mint, TOKEN, md);
    v.send(
        0,
        &[],
        vec![ws(market.pubkey()), s(admin), r(mint)],
        &[&market],
        true,
    );
    let market = market.pubkey();
    let creator = v.payer.insecure_clone();
    let profile = onboard(v, market, &creator);
    let id = [seed; 32];
    let (model, _) = Address::find_program_address(&[b"model", market.as_ref(), &id], &v.program);
    v.svm.airdrop(&model, 1_000_000).unwrap();
    let terms = Keypair::new();
    let capacity = Keypair::new();
    v.blank(&terms, terms_book_len(count));
    v.blank(&capacity, capacity_book_len(count));
    let metadata =
        Address::find_program_address(&[b"model_metadata", model.as_ref()], &v.program).0;
    let mut init = id.to_vec();
    init.extend_from_slice(&(count as u32).to_le_bytes());
    init.extend_from_slice(&metadata_payload(1, "https://example.test/model.json"));
    let ac = vec![
        r(market),
        w(model),
        ws(terms.pubkey()),
        ws(capacity.pubkey()),
        s(creator.pubkey()),
        ws(admin),
        r(SYSTEM),
        r(profile),
        w(metadata),
    ];
    v.send(18, &init, ac.clone(), &[&terms, &capacity, &creator], true);
    v.send(18, &init, ac, &[&terms, &capacity, &creator], false);
    let terms = terms.pubkey();
    let capacity = capacity.pubkey();
    let owner = Keypair::new();
    let user = Keypair::new();
    v.blank(&user, USER_LEN);
    let (authority, _) =
        Address::find_program_address(&[b"vault", user.pubkey().as_ref()], &v.program);
    let vault = v.token(mint, authority, 0);
    let source = v.token(mint, owner.pubkey(), 1_000_000_000_000);
    v.send(
        1,
        &[],
        vec![ws(user.pubkey()), s(owner.pubkey()), r(market), r(vault)],
        &[&user, &owner],
        true,
    );
    let user = user.pubkey();
    v.send(
        3,
        &1_000_000_000_000u64.to_le_bytes(),
        vec![
            r(market),
            w(user),
            s(owner.pubkey()),
            w(source),
            w(vault),
            r(mint),
            r(TOKEN),
        ],
        &[&owner],
        true,
    );
    let mut offers = Vec::new();
    for slot in 0..count {
        let provider = Keypair::new();
        let profile = onboard(v, market, &provider);
        let payout = v.token(mint, provider.pubkey(), 1_000_000_000);
        let (authority, _) = Address::find_program_address(
            &[b"bond", model.as_ref(), &(slot as u32).to_le_bytes()],
            &v.program,
        );
        let bond = v.token(mint, authority, 0);
        let mut p = [0; 24];
        u32at(&mut p, 0, slot as u32);
        for at in [4, 8, 12] {
            u32at(&mut p, at, (count - slot) as u32);
        }
        p[16] = 100;
        p[18] = 1;
        p[19] = 1;
        u32at(&mut p, 20, 4);
        v.send(
            2,
            &p,
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
        let mut p = [0; 12];
        u32at(&mut p, 0, slot as u32);
        u64at(&mut p, 4, 500_000_000);
        v.send(
            4,
            &p,
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
        assert_eq!(rd64(&v.data(bond), 64), 500_000_000);
        assert_eq!(
            rd64(&v.data(capacity), HEADER_LEN + slot * CAPACITY_LEN),
            500_000_000
        );
        offers.push(Offer {
            provider,
            profile,
            payout,
            bond,
            authority,
        });
        if slot % 512 == 511 {
            eprintln!("registered/funded model={seed} slots={}", slot + 1);
        }
    }
    Fixture {
        market,
        creator,
        profile,
        metadata,
        mint,
        model,
        id,
        terms,
        capacity,
        owner,
        user,
        vault,
        authority,
        offers,
    }
}
fn reconcile(v: &Vm, f: &Fixture, bookings: &[Address]) {
    let mut reserve = 0u64;
    let mut exposure = vec![0u64; f.offers.len()];
    let mut active = vec![0u32; f.offers.len()];
    let mut obligations = active.clone();
    for &b in bookings {
        let d = v.data(b);
        let slot = rd32(&d, 352) as usize;
        match d[148] {
            0 => {
                reserve += rd64(&d, 104);
                exposure[slot] += rd64(&d, 104);
                active[slot] += 1;
                obligations[slot] += 1
            }
            1 | 2 => {
                exposure[slot] += rd64(&d, 160);
                obligations[slot] += 1
            }
            _ => {}
        }
    }
    assert!(rd64(&v.data(f.vault), 64) >= rd64(&v.data(f.user), 72) + reserve);
    let cd = v.data(f.capacity);
    for (i, o) in f.offers.iter().enumerate() {
        let at = HEADER_LEN + i * CAPACITY_LEN;
        let funded = rd64(&cd, at);
        let pending = rd64(&cd, at + 24);
        assert!(rd64(&v.data(o.bond), 64) >= funded + pending);
        assert_eq!(rd64(&cd, at + 8), exposure[i]);
        assert_eq!(rd32(&cd, at + 16), active[i]);
        assert_eq!(rd32(&cd, at + 20), obligations[i]);
        assert!(funded >= exposure[i] + exposure[i].div_ceil(100));
    }
}
fn book(v: &mut Vm, f: &Fixture, p: &[u8]) -> (Address, u64) {
    let b = Keypair::new();
    v.blank(&b, BOOKING_LEN);
    let cu = v.send(
        6,
        p,
        vec![
            r(f.market),
            r(f.model),
            r(f.terms),
            w(f.capacity),
            w(f.user),
            s(f.owner.pubkey()),
            ws(b.pubkey()),
            s(v.payer.pubkey()),
        ],
        &[&f.owner, &b],
        true,
    );
    if v.claim_seconds == 0 {
        let booking = v.data(b.pubkey());
        let at = HEADER_LEN + rd32(&booking, 352) as usize * TERMS_LEN;
        let terms = v.data(f.terms);
        let speed = u16::from_le_bytes(terms[at + 108..at + 110].try_into().unwrap());
        let duration = i64::from(rd32(p, 50).div_ceil(u32::from(speed)));
        v.claim_seconds = (i64::from_le_bytes(booking[132..140].try_into().unwrap())
            - v.svm.get_sysvar::<Clock>().unix_timestamp - duration).try_into().unwrap();
        assert!(v.claim_seconds > 0 && v.claim_seconds < 86_400);
    }
    (b.pubkey(), cu)
}
fn settle(v: &mut Vm, f: &Fixture, b: Address, mode: u32) -> u64 {
    let bd = v.data(b);
    let slot = rd32(&bd, 352) as usize;
    let o = &f.offers[slot];
    if mode.is_multiple_of(4) {
        return v.send(
            15,
            &[],
            vec![
                r(f.market),
                r(f.model),
                w(f.capacity),
                w(f.user),
                w(b),
                s(o.provider.pubkey()),
            ],
            &[&o.provider],
            true,
        );
    }
    let mut usage = [0; 12];
    u32at(&mut usage, 0, if mode % 4 == 1 { 0 } else { 500_000 });
    u32at(&mut usage, 8, if mode % 4 == 1 { 0 } else { 500_000 });
    let cu = v.send(
        7,
        &usage,
        vec![
            r(f.market),
            r(f.model),
            w(f.capacity),
            w(f.user),
            w(b),
            s(o.provider.pubkey()),
            w(f.vault),
            w(o.payout),
            r(f.mint),
            r(f.authority),
            r(TOKEN),
        ],
        &[&o.provider],
        true,
    );
    if v.data(b)[148] == 4 {
        return cu;
    }
    if mode % 4 == 3 {
        v.send(
            11,
            &[],
            vec![r(f.market), r(f.model), w(b), s(f.owner.pubkey())],
            &[&f.owner],
            true,
        );
        let charge = rd64(&v.data(b), 160);
        v.send(
            12,
            &(charge / 2).to_le_bytes(),
            vec![
                r(f.market),
                r(f.model),
                w(f.capacity),
                w(f.user),
                w(b),
                s(v.payer.pubkey()),
                w(o.bond),
                w(f.vault),
                r(f.mint),
                r(o.authority),
                r(TOKEN),
            ],
            &[],
            true,
        );
    } else {
        let mut c = v.svm.get_sysvar::<Clock>();
        c.unix_timestamp = i64::from_le_bytes(v.data(b)[140..148].try_into().unwrap()) + 1;
        v.svm.set_sysvar(&c);
        v.send(
            9,
            &[],
            vec![r(f.market), r(f.model), w(f.capacity), w(b)],
            &[],
            true,
        );
    }
    cu
}
fn main() {
    if std::env::args().any(|a| a == "--receipt-bench") {
        receipt_bench::run();
        return;
    }
    if std::env::args().any(|a| a == "--export") {
        export();
        return;
    }
    let start = std::time::Instant::now();
    let mut results = Vec::new();
    for legacy in [false, true] {
        let mut v = Vm::new(legacy);
        let f = setup(&mut v, 4096, 7);
        let metadata_results = metadata::check(&mut v, &f);
        let p = book_payload(&f.id, Address::default());
        let mut bookings = Vec::new();
        let (b, cu) = book(&mut v, &f, &p);
        bookings.push(b);
        assert_eq!(rd32(&v.data(b), 352), 4095);
        reconcile(&v, &f, &bookings);
        let claim_cu = settle(&mut v, &f, b, 3);
        let mut maxp = book_payload(&f.id, f.offers[4095].provider.pubkey());
        for at in [46, 50, 63, 71] {
            u32at(&mut maxp, at, u32::MAX);
        }
        u32at(&mut maxp, 67, u32::MAX / 2);
        // maximum output needs a provider whose advertised speed fits the claim window.
        let terms = v.data(f.terms);
        let at = HEADER_LEN + 4095 * TERMS_LEN;
        let mut fast = [0; 24];
        u32at(&mut fast, 0, 4095);
        fast[4..20].copy_from_slice(&terms[at + 96..at + 112]);
        fast[16..18].copy_from_slice(&u16::MAX.to_le_bytes());
        fast[20..24].copy_from_slice(&terms[at + 116..at + 120]);
        v.send(5, &fast, vec![r(f.market), r(f.model), w(f.terms),
            s(f.offers[4095].provider.pubkey())], &[&f.offers[4095].provider], true);
        let expected = reference(&v, &f, &maxp).unwrap();
        let (maxb, max_cache_cu) = book(&mut v, &f, &maxp);
        bookings.push(maxb);
        assert_eq!(rd32(&v.data(maxb), 352), expected.0);
        reconcile(&v, &f, &bookings);
        settle(&mut v, &f, maxb, 0);
        reconcile(&v, &f, &bookings);
        let mut overflow = [0; 24];
        u32at(&mut overflow, 0, 4096);
        for at in [4, 8, 12] {
            u32at(&mut overflow, at, 1)
        }
        overflow[16] = 100;
        overflow[19] = 1;
        u32at(&mut overflow, 20, 1);
        let o = &f.offers[0];
        v.send(
            2,
            &overflow,
            vec![
                r(f.market),
                r(f.model),
                w(f.terms),
                w(f.capacity),
                r(o.profile),
                s(o.provider.pubkey()),
                r(o.payout),
                r(o.bond),
            ],
            &[&o.provider],
            false,
        );
        error(&v, 6);
        for i in 0..256 {
            let (b, _) = book(&mut v, &f, &p);
            bookings.push(b);
            reconcile(&v, &f, &bookings);
            settle(&mut v, &f, b, i);
            reconcile(&v, &f, &bookings);
        }
        overlap(&mut v, &f);
        boundaries(&mut v, &f);
        let deadline_fixture = setup(&mut v, 32, 18);
        boundaries(&mut v, &deadline_fixture);
        hostile_accounts(&mut v, &deadline_fixture);
        selection_edges(&mut v, &deadline_fixture);
        extremes(&mut v, &deadline_fixture);
        let random_fixture = setup(&mut v, 32, 19);
        randomized(&mut v, &random_fixture, 2000);
        for seed in 20..52 {
            let f = setup(&mut v, 32, seed);
            let p = book_payload(&f.id, Address::default());
            let mut bs = Vec::new();
            for i in 0..32 {
                let (b, _) = book(&mut v, &f, &p);
                bs.push(b);
                reconcile(&v, &f, &bs);
                settle(&mut v, &f, b, i);
                reconcile(&v, &f, &bs);
            }
        }
        results.push(serde_json::json!({"profile":if legacy{"legacy-spl-3.5"}else{"p-token"},"registered_funded_slots":5184,"models":35,"book4096_descending_cu":cu,"claim_cu":claim_cu,"book4096_maxinput_mixedcache_cu":max_cache_cu,"mixed_settlement_bookings":1281,"reference_cases":2000,"elapsed_seconds_cumulative":start.elapsed().as_secs_f64(),"executions":v.executions,"succeeded_by_opcode":v.succeeded,"rejected_by_opcode":v.rejected,"packet_max_by_opcode":v.packet_max,"metadata":metadata_results,"fixtures":"mint/token allocations and preallocated blank accounts seeded; all program mutations and token transfer CPIs real SBF","compute_scope":"includes150CU ComputeBudget; excludes system account creation","reconciliation":"after each tested lifecycle mutation; rollback after all rejected tx"}));
        eprintln!("profile complete legacy={legacy}");
    }
    let output = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/stress");
    std::fs::create_dir_all(&output).unwrap();
    std::fs::write(
        output.join(if std::env::var("FOSSA_SBF").is_ok() {
            "reachable-production.json"
        } else {
            "reachable.json"
        }),
        serde_json::to_vec_pretty(&results).unwrap(),
    )
    .unwrap();
    println!("{}", serde_json::to_string(&results).unwrap());
}
fn random(state: &mut u64) -> u32 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state as u32
}
fn reference(v: &Vm, f: &Fixture, p: &[u8]) -> Option<(u32, u64, u64)> {
    let td = v.data(f.terms);
    let cd = v.data(f.capacity);
    let ceil = |tokens: u32, rate: u32| -> u64 {
        (u128::from(tokens) * u128::from(rate)).div_ceil(1_000_000) as u64
    };
    let mut best = None;
    assert!(v.claim_seconds > 0);
    for slot in 0..f.offers.len() {
        let t = HEADER_LEN + slot * TERMS_LEN;
        let c = HEADER_LEN + slot * CAPACITY_LEN;
        if td[t + 111] != 1
            || td[t + 112] != 1
            || (p[54] == 1 && td[t + 110] != 1)
            || u16::from_le_bytes(td[t + 108..t + 110].try_into().unwrap())
                < u16::from_le_bytes(p[44..46].try_into().unwrap())
            || u64::from(rd32(p, 50)) > (86_400 - v.claim_seconds)
                * u64::from(u16::from_le_bytes(td[t + 108..t + 110].try_into().unwrap()))
        {
            continue;
        }
        let rates = [rd32(&td, t + 96), rd32(&td, t + 100), rd32(&td, t + 104)];
        if (0..3).any(|i| rates[i] > rd32(p, 32 + 4 * i)) {
            continue;
        }
        let cached = if td[t..t + 32] == p[75..107] {
            rd32(p, 67)
        } else {
            0
        };
        let score = ceil(rd32(p, 63) - cached, rates[0])
            + ceil(cached, rates[1])
            + ceil(rd32(p, 71), rates[2]);
        let reserve = ceil(rd32(p, 46), rates[0]) + ceil(rd32(p, 50), rates[2]) + 1;
        let exposure = rd64(&cd, c + 8) + reserve;
        if rd32(&cd, c + 16) >= rd32(&td, t + 116)
            || rd32(&cd, c + 20) == u32::MAX
            || reserve > rd64(p, 55).min(rd64(&v.data(f.user), 72))
            || rd64(&cd, c) < exposure + exposure.div_ceil(100)
        {
            continue;
        }
        if best.is_none_or(|(_, _, old)| score < old) {
            best = Some((slot as u32, reserve, score));
        }
    }
    best
}
fn randomized(v: &mut Vm, f: &Fixture, cases: usize) {
    let mut state = 0x29c783fe312983u64;
    let mut bookings = Vec::new();
    let mut accepted = 0;
    for case in 0..cases {
        let slot = random(&mut state) as usize % f.offers.len();
        let o = &f.offers[slot];
        let mut update = [0; 24];
        u32at(&mut update, 0, slot as u32);
        let input = 1 + random(&mut state) % 10000;
        u32at(&mut update, 4, input);
        u32at(&mut update, 8, random(&mut state) % (input + 1));
        u32at(&mut update, 12, 1 + random(&mut state) % 10000);
        update[16] = (1 + random(&mut state) % 200) as u8;
        update[18] = (random(&mut state) % 2) as u8;
        update[19] = (!random(&mut state).is_multiple_of(5)) as u8;
        u32at(&mut update, 20, 1 + random(&mut state) % 4);
        v.send(
            5,
            &update,
            vec![r(f.market), r(f.model), w(f.terms), s(o.provider.pubkey())],
            &[&o.provider],
            true,
        );
        let mut approval = [0; 5];
        u32at(&mut approval, 0, slot as u32);
        approval[4] = (!random(&mut state).is_multiple_of(5)) as u8;
        v.send(
            16,
            &approval,
            vec![r(f.market), r(f.model), w(f.terms), s(v.payer.pubkey())],
            &[],
            true,
        );
        let mut p = book_payload(
            &f.id,
            if case % 3 == 0 {
                o.provider.pubkey()
            } else {
                Address::default()
            },
        );
        for at in [32, 36, 40] {
            u32at(
                &mut p,
                at,
                if case % 13 == 0 {
                    u32::MAX
                } else {
                    random(&mut state) % 10001
                },
            );
        }
        p[44] = (random(&mut state) % 201) as u8;
        p[54] = (random(&mut state) % 2) as u8;
        let input = if case % 17 == 0 {
            u32::MAX
        } else {
            1 + random(&mut state) % 2_000_000
        };
        let output = if case % 19 == 0 {
            u32::MAX
        } else {
            1 + random(&mut state) % 2_000_000
        };
        u32at(&mut p, 46, input);
        u32at(&mut p, 50, output);
        let estimated = random(&mut state) % (input.min(u32::MAX - 1) + 1);
        u32at(&mut p, 63, estimated);
        u32at(
            &mut p,
            67,
            if case % 3 == 0 {
                random(&mut state) % (estimated.min(u32::MAX - 1) + 1)
            } else {
                0
            },
        );
        u32at(
            &mut p,
            71,
            random(&mut state) % (output.min(u32::MAX - 1) + 1),
        );
        u64at(
            &mut p,
            55,
            if case % 7 == 0 {
                (random(&mut state) % 1000) as u64
            } else {
                u64::MAX
            },
        );
        match reference(v, f, &p) {
            Some((slot, reserve, score)) => {
                let (b, _) = book(v, f, &p);
                bookings.push(b);
                let d = v.data(b);
                assert_eq!(rd32(&d, 352), slot);
                assert_eq!(rd64(&d, 104), reserve);
                assert_eq!(rd64(&d, 360), score);
                reconcile(v, f, &bookings);
                let o = &f.offers[slot as usize];
                v.send(
                    15,
                    &[],
                    vec![
                        r(f.market),
                        r(f.model),
                        w(f.capacity),
                        w(f.user),
                        w(b),
                        s(o.provider.pubkey()),
                    ],
                    &[&o.provider],
                    true,
                );
                accepted += 1
            }
            None => {
                let b = Keypair::new();
                v.blank(&b, BOOKING_LEN);
                v.send(
                    6,
                    &p,
                    vec![
                        r(f.market),
                        r(f.model),
                        r(f.terms),
                        w(f.capacity),
                        w(f.user),
                        s(f.owner.pubkey()),
                        ws(b.pubkey()),
                        s(v.payer.pubkey()),
                    ],
                    &[&f.owner, &b],
                    false,
                );
            }
        }
        reconcile(v, f, &bookings);
    }
    eprintln!("reference sbf cases={cases} accepted={accepted}");
}
fn base64(data: &[u8]) -> String {
    const ALPH: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for c in data.chunks(3) {
        let n = (u32::from(c[0]) << 16)
            | (u32::from(*c.get(1).unwrap_or(&0)) << 8)
            | u32::from(*c.get(2).unwrap_or(&0));
        s.push(ALPH[(n >> 18) as usize] as char);
        s.push(ALPH[((n >> 12) & 63) as usize] as char);
        s.push(if c.len() > 1 {
            ALPH[((n >> 6) & 63) as usize] as char
        } else {
            '='
        });
        s.push(if c.len() > 2 {
            ALPH[(n & 63) as usize] as char
        } else {
            '='
        });
    }
    s
}
fn export() {
    use std::os::unix::fs::PermissionsExt;
    let mut v = Vm::new(false);
    let out = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/stress/genesis");
    if out.exists() {
        std::fs::remove_dir_all(&out).unwrap();
    }
    std::fs::create_dir_all(&out).unwrap();
    let private = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.local/stress");
    std::fs::create_dir_all(&private).unwrap();
    std::fs::set_permissions(&private, std::fs::Permissions::from_mode(0o700)).unwrap();
    let mut fs = vec![setup(&mut v, 4096, 7)];
    for seed in 20..52 {
        fs.push(setup(&mut v, 32, seed));
    }
    let mut models = Vec::new();
    let mut users = Vec::new();
    for f in &fs {
        models.push(serde_json::json!({"market":f.market.to_string(),"creator":f.creator.pubkey().to_string(),"creatorKey":f.creator.to_bytes().to_vec(),"profile":f.profile.to_string(),"metadata":f.metadata.to_string(),"mint":f.mint.to_string(),"model":f.model.to_string(),"modelID":f.id,"terms":f.terms.to_string(),"capacity":f.capacity.to_string(),"offers":f.offers.iter().map(|o|serde_json::json!({"provider":o.provider.pubkey().to_string(),"profile":o.profile.to_string(),"providerKey":o.provider.to_bytes().to_vec(),"payout":o.payout.to_string(),"bond":o.bond.to_string(),"authority":o.authority.to_string()})).collect::<Vec<_>>()}));
        for lane in 0..2 {
            if lane == 0 {
                users.push(serde_json::json!({"model":f.model.to_string(),"owner":f.owner.pubkey().to_string(),"ownerKey":f.owner.to_bytes().to_vec(),"user":f.user.to_string(),"vault":f.vault.to_string(),"authority":f.authority.to_string()}));
                continue;
            }
            let owner = Keypair::new();
            let user = Keypair::new();
            v.blank(&user, USER_LEN);
            let (authority, _) =
                Address::find_program_address(&[b"vault", user.pubkey().as_ref()], &v.program);
            let vault = v.token(f.mint, authority, 0);
            let source = v.token(f.mint, owner.pubkey(), 1_000_000_000_000);
            v.send(
                1,
                &[],
                vec![ws(user.pubkey()), s(owner.pubkey()), r(f.market), r(vault)],
                &[&user, &owner],
                true,
            );
            v.send(
                3,
                &1_000_000_000_000u64.to_le_bytes(),
                vec![
                    r(f.market),
                    w(user.pubkey()),
                    s(owner.pubkey()),
                    w(source),
                    w(vault),
                    r(f.mint),
                    r(TOKEN),
                ],
                &[&owner],
                true,
            );
            users.push(serde_json::json!({"model":f.model.to_string(),"owner":owner.pubkey().to_string(),"ownerKey":owner.to_bytes().to_vec(),"user":user.pubkey().to_string(),"vault":vault.to_string(),"authority":authority.to_string()}));
        }
    }
    let mut payers = Vec::new();
    for _ in 0..66 {
        let payer = Keypair::new();
        v.svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
        let account = v.svm.get_account(&payer.pubkey()).unwrap();
        write_genesis(&out, payer.pubkey(), account);
        payers.push(serde_json::json!({"pubkey":payer.pubkey().to_string(),"key":payer.to_bytes().to_vec()}));
    }
    write_genesis(
        &out,
        v.payer.pubkey(),
        v.svm.get_account(&v.payer.pubkey()).unwrap(),
    );
    for owner in [v.program, TOKEN] {
        for (k, a) in v.svm.get_program_accounts(&owner) {
            write_genesis(&out, k, a);
        }
    }
    let meta = serde_json::json!({"program":v.program.to_string(),"admin":v.payer.pubkey().to_string(),"adminKey":v.payer.to_bytes().to_vec(),"models":models,"users":users,"payers":payers,"fixture":"mint/token/source allocations seeded; model initialization, all registration, funding, user initialization and deposits executed real SBF; zero outstanding bookings"});
    let path = private.join("metadata.json");
    std::fs::write(&path, serde_json::to_vec(&meta).unwrap()).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    eprintln!(
        "export complete public genesis={} private metadata={}",
        out.display(),
        path.display()
    );
}
fn write_genesis(dir: &std::path::Path, k: Address, a: Account) {
    let obj = serde_json::json!({"pubkey":k.to_string(),"account":{"lamports":a.lamports,"owner":a.owner.to_string(),"data":[base64(&a.data),"base64"],"executable":a.executable,"rentEpoch":a.rent_epoch,"space":a.data.len()}});
    std::fs::write(
        dir.join(format!("{k}.json")),
        serde_json::to_vec(&obj).unwrap(),
    )
    .unwrap();
}
fn error(v: &Vm, n: u32) {
    assert!(
        v.last_error
            .as_ref()
            .unwrap()
            .contains(&format!("Custom({n})")),
        "expected custom {n}, got {:?}",
        v.last_error
    );
}
fn clock(v: &mut Vm, time: i64) {
    let mut c = v.svm.get_sysvar::<Clock>();
    c.unix_timestamp = time;
    v.svm.set_sysvar(&c);
}
fn timestamp(v: &Vm, b: Address, at: usize) -> i64 {
    i64::from_le_bytes(v.data(b)[at..at + 8].try_into().unwrap())
}
fn boundaries(v: &mut Vm, f: &Fixture) {
    let p = book_payload(&f.id, Address::default());
    let mut bs = Vec::new();
    // reachable requests with malformed payloads and metadata; all failed transactions restore account state.
    for kind in 0..7 {
        let b = Keypair::new();
        v.blank(&b, BOOKING_LEN);
        let mut ac = vec![
            r(f.market),
            r(f.model),
            r(f.terms),
            w(f.capacity),
            w(f.user),
            s(f.owner.pubkey()),
            ws(b.pubkey()),
            s(v.payer.pubkey()),
        ];
        let mut payload = p.to_vec();
        let mut owner_signs = true;
        match kind {
            0 => {
                payload.pop();
            }
            1 => payload[54] = 2,
            2 => {
                ac[5] = r(f.owner.pubkey());
                owner_signs = false
            }
            3 => ac[3] = r(f.capacity),
            4 => ac[2] = r(f.capacity),
            5 => payload[0] ^= 1,
            6 => ac[4] = w(f.capacity),
            _ => {}
        }
        let signers = if owner_signs {
            vec![&f.owner, &b]
        } else {
            vec![&b]
        };
        v.send(6, &payload, ac, &signers, false);
        if kind <= 1 {
            error(v, 3)
        } else if kind == 2 {
            error(v, 4)
        } else {
            error(v, 2)
        }
    }
    let (b, _) = book(v, f, &p);
    bs.push(b);
    reconcile(v, f, &bs);
    let o = &f.offers[rd32(&v.data(b), 352) as usize];
    let claim = vec![
        r(f.market),
        r(f.model),
        w(f.capacity),
        w(f.user),
        w(b),
        s(o.provider.pubkey()),
        w(f.vault),
        w(o.payout),
        r(f.mint),
        r(f.authority),
        r(TOKEN),
    ];
    let mut usage = [0; 12];
    u32at(&mut usage, 0, 1_000_000);
    u32at(&mut usage, 8, 1_000_000);
    // fixture-only frozen custody account probes CPI rollback, not a reachable freeze transaction.
    let vault = v.svm.get_account(&f.vault).unwrap();
    let mut frozen = vault.clone();
    frozen.data[108] = 2;
    v.svm.set_account(f.vault, frozen).unwrap();
    v.send(7, &usage, claim.clone(), &[&o.provider], false);
    reconcile(v, f, &bs);
    v.svm.set_account(f.vault, vault).unwrap();
    clock(v, timestamp(v, b, 132));
    v.send(7, &usage, claim.clone(), &[&o.provider], true);
    reconcile(v, f, &bs);
    v.send(7, &usage, claim, &[&o.provider], false);
    error(v, 7);
    let end = timestamp(v, b, 140);
    clock(v, end);
    v.send(
        9,
        &[],
        vec![r(f.market), r(f.model), w(f.capacity), w(b)],
        &[],
        false,
    );
    error(v, 7);
    v.send(
        11,
        &[],
        vec![r(f.market), r(f.model), w(b), s(f.owner.pubkey())],
        &[&f.owner],
        true,
    );
    reconcile(v, f, &bs);
    let charge = rd64(&v.data(b), 160);
    v.send(
        12,
        &charge.to_le_bytes(),
        vec![
            r(f.market),
            r(f.model),
            w(f.capacity),
            w(f.user),
            w(b),
            s(v.payer.pubkey()),
            w(o.bond),
            w(f.vault),
            r(f.mint),
            r(o.authority),
            r(TOKEN),
        ],
        &[],
        true,
    );
    reconcile(v, f, &bs);
    let (b, _) = book(v, f, &p);
    bs.push(b);
    let end = timestamp(v, b, 132);
    clock(v, end);
    let ac = vec![r(f.market), r(f.model), w(f.capacity), w(f.user), w(b)];
    v.send(8, &[], ac.clone(), &[], false);
    error(v, 7);
    clock(v, end + 1);
    v.send(8, &[], ac.clone(), &[], true);
    reconcile(v, f, &bs);
    v.send(8, &[], ac, &[], false);
    error(v, 7);
    let (b, _) = book(v, f, &p);
    bs.push(b);
    let o = &f.offers[rd32(&v.data(b), 352) as usize];
    let end = timestamp(v, b, 132);
    clock(v, end + 1);
    v.send(
        7,
        &usage,
        vec![
            r(f.market),
            r(f.model),
            w(f.capacity),
            w(f.user),
            w(b),
            s(o.provider.pubkey()),
            w(f.vault),
            w(o.payout),
            r(f.mint),
            r(f.authority),
            r(TOKEN),
        ],
        &[&o.provider],
        false,
    );
    error(v, 7);
    v.send(
        8,
        &[],
        vec![r(f.market), r(f.model), w(f.capacity), w(f.user), w(b)],
        &[],
        true,
    );
    reconcile(v, f, &bs);
    let (b, _) = book(v, f, &p);
    bs.push(b);
    let slot = rd32(&v.data(b), 352);
    let o = &f.offers[slot as usize];
    let req = Keypair::new();
    v.blank(&req, UNBOND_LEN);
    let mut unbond = [0; 12];
    u32at(&mut unbond, 0, slot);
    u64at(
        &mut unbond,
        4,
        rd64(
            &v.data(f.capacity),
            HEADER_LEN + slot as usize * CAPACITY_LEN,
        ),
    );
    let ac = vec![
        r(f.market),
        r(f.model),
        r(f.terms),
        w(f.capacity),
        s(o.provider.pubkey()),
        ws(req.pubkey()),
        s(v.payer.pubkey()),
    ];
    v.send(13, &unbond, ac.clone(), &[&o.provider, &req], false);
    error(v, 8);
    v.send(
        15,
        &[],
        vec![
            r(f.market),
            r(f.model),
            w(f.capacity),
            w(f.user),
            w(b),
            s(o.provider.pubkey()),
        ],
        &[&o.provider],
        true,
    );
    reconcile(v, f, &bs);
    u64at(&mut unbond, 4, 10);
    v.send(13, &unbond, ac, &[&o.provider, &req], true);
    reconcile(v, f, &bs);
    let end = timestamp(v, req.pubkey(), 80);
    let ac = vec![
        r(f.market),
        r(f.model),
        r(f.terms),
        w(f.capacity),
        w(req.pubkey()),
        s(o.provider.pubkey()),
        w(o.bond),
        w(o.payout),
        r(f.mint),
        r(o.authority),
        r(TOKEN),
    ];
    clock(v, end - 1);
    v.send(14, &[], ac.clone(), &[&o.provider], false);
    error(v, 7);
    clock(v, end);
    v.send(14, &[], ac.clone(), &[&o.provider], true);
    reconcile(v, f, &bs);
    v.send(14, &[], ac, &[&o.provider], false);
    error(v, 7);
    v.send(17, &[], vec![w(req.pubkey()), w(f.user)], &[], false);
    error(v, 7);
    v.send(
        17,
        &[],
        vec![w(req.pubkey()), w(v.payer.pubkey())],
        &[],
        true,
    );
    let closed = bs[0];
    v.send(17, &[], vec![w(closed), w(f.user)], &[], false);
    error(v, 7);
    v.send(17, &[], vec![w(closed), w(v.payer.pubkey())], &[], true);
    assert!(v.svm.get_account(&closed).is_none_or(|a| a.lamports == 0));
    eprintln!("exact deadlines, malformed payload/metadata, frozen-CPI rollback, unbond collateral/pending isolation passed");
}
fn overlap(v: &mut Vm, f: &Fixture) {
    let p = book_payload(&f.id, Address::default());
    let mut bs = Vec::new();
    for i in 0..64 {
        let (b, _) = book(v, f, &p);
        assert_eq!(rd32(&v.data(b), 352), (f.offers.len() - 1 - i / 4) as u32);
        bs.push(b);
        reconcile(v, f, &bs);
    }
    let o = &f.offers[f.offers.len() - 1];
    let mut update = [0; 24];
    u32at(&mut update, 0, (f.offers.len() - 1) as u32);
    for at in [4, 8, 12] {
        u32at(&mut update, at, 1000);
    }
    update[16] = 100;
    update[18] = 1;
    u32at(&mut update, 20, 1);
    v.send(
        5,
        &update,
        vec![r(f.market), r(f.model), w(f.terms), s(o.provider.pubkey())],
        &[&o.provider],
        true,
    );
    let mut approval = [0; 5];
    u32at(&mut approval, 0, (f.offers.len() - 1) as u32);
    v.send(
        16,
        &approval,
        vec![r(f.market), r(f.model), w(f.terms), s(v.payer.pubkey())],
        &[],
        true,
    );
    for (i, &b) in bs.iter().enumerate() {
        settle(v, f, b, if i % 2 == 0 { 3 } else { 0 });
        reconcile(v, f, &bs);
    }
    assert_eq!(rd64(&v.data(bs[0]), 160), 2);
    for at in [4, 8, 12] {
        u32at(&mut update, at, 1)
    }
    update[19] = 1;
    u32at(&mut update, 20, 4);
    v.send(
        5,
        &update,
        vec![r(f.market), r(f.model), w(f.terms), s(o.provider.pubkey())],
        &[&o.provider],
        true,
    );
    approval[4] = 1;
    v.send(
        16,
        &approval,
        vec![r(f.market), r(f.model), w(f.terms), s(v.payer.pubkey())],
        &[],
        true,
    );
    eprintln!("64 outstanding bookings capacity fallthrough + immutable prior terms passed");
}
fn hostile_accounts(v: &mut Vm, f: &Fixture) {
    let p = book_payload(&f.id, Address::default());
    let (b, _) = book(v, f, &p);
    let slot = rd32(&v.data(b), 352) as usize;
    let o = &f.offers[slot];
    let mut usage = [0; 12];
    u32at(&mut usage, 0, 1_000_000);
    let claim = vec![
        r(f.market),
        r(f.model),
        w(f.capacity),
        w(f.user),
        w(b),
        s(o.provider.pubkey()),
        w(f.vault),
        w(o.payout),
        r(f.mint),
        r(f.authority),
        r(TOKEN),
    ];
    for index in [1, 2, 3, 6, 7, 8, 9, 10] {
        let mut ac = claim.clone();
        let wrong = if index == 7 {
            f.offers[0].payout
        } else if index == 6 {
            o.bond
        } else if index == 2 {
            f.terms
        } else if index == 9 {
            f.offers[0].authority
        } else {
            f.capacity
        };
        ac[index].pubkey = wrong;
        v.send(7, &usage, ac, &[&o.provider], false);
        error(v, if index == 10 { 3 } else { 2 });
        reconcile(v, f, &[b]);
    }
    for mutation in 0..2 {
        let original = v.svm.get_account(&f.capacity).unwrap();
        let mut bad = original.clone();
        if mutation == 0 {
            bad.owner = SYSTEM
        } else {
            bad.data[0] ^= 1;
        }
        v.svm.set_account(f.capacity, bad).unwrap();
        v.send(7, &usage, claim.clone(), &[&o.provider], false);
        error(v, 2);
        v.svm.set_account(f.capacity, original).unwrap();
        reconcile(v, f, &[b]);
    }
    let vault = v.svm.get_account(&f.vault).unwrap();
    for offset in [72, 109, 129] {
        let mut bad = vault.clone();
        u32at(&mut bad.data, offset, 1);
        v.svm.set_account(f.vault, bad).unwrap();
        v.send(7, &usage, claim.clone(), &[&o.provider], false);
        error(v, 2);
        v.svm.set_account(f.vault, vault.clone()).unwrap();
        reconcile(v, f, &[b]);
    }
    v.send(
        15,
        &[],
        vec![
            r(f.market),
            r(f.model),
            w(f.capacity),
            w(f.user),
            w(b),
            s(o.provider.pubkey()),
        ],
        &[&o.provider],
        true,
    );
    reconcile(v, f, &[b]);
    let destination = v.token(f.mint, f.owner.pubkey(), 0);
    v.send(
        10,
        &100u64.to_le_bytes(),
        vec![
            r(f.market),
            w(f.user),
            s(f.owner.pubkey()),
            w(f.vault),
            w(destination),
            r(f.mint),
            r(f.authority),
            r(TOKEN),
        ],
        &[&f.owner],
        true,
    );
    assert_eq!(rd64(&v.data(destination), 64), 100);
    reconcile(v, f, &[b]);
    v.send(
        3,
        &100u64.to_le_bytes(),
        vec![
            r(f.market),
            w(f.user),
            s(f.owner.pubkey()),
            w(destination),
            w(f.vault),
            r(f.mint),
            r(TOKEN),
        ],
        &[&f.owner],
        true,
    );
    assert_eq!(rd64(&v.data(destination), 64), 0);
    reconcile(v, f, &[b]);
    eprintln!("wrong pinned identity/mint/token-program, foreign ownership/magic, delegated/native/close-authority vault rejects + withdrawal/deposit passed");
}
fn extremes(v: &mut Vm, f: &Fixture) {
    let source = v.token(f.mint, f.owner.pubkey(), 60_000_000_000_000);
    v.send(
        3,
        &60_000_000_000_000u64.to_le_bytes(),
        vec![
            r(f.market),
            w(f.user),
            s(f.owner.pubkey()),
            w(source),
            w(f.vault),
            r(f.mint),
            r(TOKEN),
        ],
        &[&f.owner],
        true,
    );
    for (slot, o) in f.offers.iter().enumerate() {
        let source = v.token(f.mint, o.provider.pubkey(), 50_000_000_000_000);
        let mut fund = [0; 12];
        u32at(&mut fund, 0, slot as u32);
        u64at(&mut fund, 4, 50_000_000_000_000);
        v.send(
            4,
            &fund,
            vec![
                r(f.market),
                r(f.model),
                r(f.terms),
                w(f.capacity),
                s(o.provider.pubkey()),
                w(source),
                w(o.bond),
                r(f.mint),
                r(TOKEN),
            ],
            &[&o.provider],
            true,
        );
        let mut rates = [0; 24];
        u32at(&mut rates, 0, slot as u32);
        u32at(&mut rates, 4, u32::MAX - slot as u32);
        u32at(&mut rates, 8, (u32::MAX - slot as u32) / 10);
        u32at(&mut rates, 12, u32::MAX - slot as u32);
        rates[16..18].copy_from_slice(&u16::MAX.to_le_bytes());
        rates[18] = 1;
        rates[19] = 1;
        u32at(&mut rates, 20, 4);
        v.send(
            5,
            &rates,
            vec![r(f.market), r(f.model), w(f.terms), s(o.provider.pubkey())],
            &[&o.provider],
            true,
        );
    }
    let mut bs = Vec::new();
    for (case, cached) in [0, u32::MAX / 2, u32::MAX, u32::MAX]
        .into_iter()
        .enumerate()
    {
        if case == 3 {
            let o = &f.offers[7];
            let source = v.token(f.mint, o.provider.pubkey(), 50_000_000_000_000);
            let mut fund = [0; 12];
            u32at(&mut fund, 0, 7);
            u64at(&mut fund, 4, 50_000_000_000_000);
            v.send(
                4,
                &fund,
                vec![
                    r(f.market),
                    r(f.model),
                    r(f.terms),
                    w(f.capacity),
                    s(o.provider.pubkey()),
                    w(source),
                    w(o.bond),
                    r(f.mint),
                    r(TOKEN),
                ],
                &[&o.provider],
                true,
            );
        }
        let mut p = book_payload(&f.id, f.offers[7].provider.pubkey());
        for at in [46, 50, 63, 71] {
            u32at(&mut p, at, u32::MAX);
        }
        u32at(&mut p, 67, cached);
        u64at(&mut p, 55, u64::MAX);
        let expected = reference(v, f, &p).unwrap();
        let (b, _) = book(v, f, &p);
        bs.push(b);
        assert_eq!(rd32(&v.data(b), 352), expected.0);
        assert_eq!(rd64(&v.data(b), 104), expected.1);
        assert_eq!(rd64(&v.data(b), 360), expected.2);
        reconcile(v, f, &bs);
        eprintln!(
            "extreme cached={cached} slot={} reserve={} cacheflag={} limits={}/{}",
            expected.0,
            expected.1,
            v.data(b)[358],
            rd32(&v.data(b), 124),
            rd32(&v.data(b), 128)
        );
        let o = &f.offers[expected.0 as usize];
        let mut usage = [0; 12];
        u32at(&mut usage, 0, u32::MAX - cached);
        u32at(&mut usage, 4, cached);
        u32at(&mut usage, 8, u32::MAX);
        if v.data(b)[358] == 0 && cached > 0 {
            let accounts = vec![
                r(f.market),
                r(f.model),
                w(f.capacity),
                w(f.user),
                w(b),
                s(o.provider.pubkey()),
                w(f.vault),
                w(o.payout),
                r(f.mint),
                r(f.authority),
                r(TOKEN),
            ];
            v.send(7, &usage, accounts, &[&o.provider], false);
            error(v, 8);
            u32at(&mut usage, 0, u32::MAX);
            u32at(&mut usage, 4, 0);
        }
        v.send(
            7,
            &usage,
            vec![
                r(f.market),
                r(f.model),
                w(f.capacity),
                w(f.user),
                w(b),
                s(o.provider.pubkey()),
                w(f.vault),
                w(o.payout),
                r(f.mint),
                r(f.authority),
                r(TOKEN),
            ],
            &[&o.provider],
            true,
        );
        reconcile(v, f, &bs);
        v.send(
            11,
            &[],
            vec![r(f.market), r(f.model), w(b), s(f.owner.pubkey())],
            &[&f.owner],
            true,
        );
        let charge = rd64(&v.data(b), 160);
        v.send(
            12,
            &charge.to_le_bytes(),
            vec![
                r(f.market),
                r(f.model),
                w(f.capacity),
                w(f.user),
                w(b),
                s(v.payer.pubkey()),
                w(o.bond),
                w(f.vault),
                r(f.mint),
                r(o.authority),
                r(TOKEN),
            ],
            &[],
            true,
        );
        reconcile(v, f, &bs);
    }
    eprintln!("u32max rates/tokens mixed/full cache book/claim/refund + u128 reference passed");
}
fn selection_edges(v: &mut Vm, f: &Fixture) {
    for (slot, o) in f.offers.iter().enumerate() {
        let mut p = [0; 24];
        u32at(&mut p, 0, slot as u32);
        for at in [4, 8, 12] {
            u32at(&mut p, at, 1)
        }
        p[16] = 100;
        p[18] = 1;
        p[19] = 1;
        u32at(&mut p, 20, 1);
        v.send(
            5,
            &p,
            vec![r(f.market), r(f.model), w(f.terms), s(o.provider.pubkey())],
            &[&o.provider],
            true,
        );
    }
    let p = book_payload(&f.id, Address::default());
    let mut bs = Vec::new();
    for expected in 0..2 {
        let (b, _) = book(v, f, &p);
        assert_eq!(rd32(&v.data(b), 352), expected);
        bs.push(b);
        reconcile(v, f, &bs);
    }
    for &b in &bs {
        settle(v, f, b, 0);
        reconcile(v, f, &bs);
    }
    for (slot, o) in f.offers.iter().enumerate() {
        let mut p = [0; 24];
        u32at(&mut p, 0, slot as u32);
        u32at(&mut p, 4, if slot == 1 { 2 } else { 1 });
        u32at(&mut p, 8, 1);
        u32at(&mut p, 12, if slot == 0 { 1000 } else { 1 });
        p[16] = 100;
        p[18] = 1;
        p[19] = (slot < 2) as u8;
        u32at(&mut p, 20, 1);
        v.send(
            5,
            &p,
            vec![r(f.market), r(f.model), w(f.terms), s(o.provider.pubkey())],
            &[&o.provider],
            true,
        );
    }
    let mut p = book_payload(&f.id, Address::default());
    u32at(&mut p, 71, 0);
    u64at(&mut p, 55, 4);
    let (b, _) = book(v, f, &p);
    bs.push(b);
    assert_eq!(rd32(&v.data(b), 352), 1);
    settle(v, f, b, 0);
    reconcile(v, f, &bs);
    u64at(&mut p, 55, u64::MAX);
    let (b, _) = book(v, f, &p);
    bs.push(b);
    assert_eq!(rd32(&v.data(b), 352), 0);
    settle(v, f, b, 0);
    reconcile(v, f, &bs);
    let amount = rd64(&v.data(f.user), 72) - 4;
    let destination = v.token(f.mint, f.owner.pubkey(), 0);
    v.send(
        10,
        &amount.to_le_bytes(),
        vec![
            r(f.market),
            w(f.user),
            s(f.owner.pubkey()),
            w(f.vault),
            w(destination),
            r(f.mint),
            r(f.authority),
            r(TOKEN),
        ],
        &[&f.owner],
        true,
    );
    let (b, _) = book(v, f, &p);
    bs.push(b);
    assert_eq!(rd32(&v.data(b), 352), 1);
    settle(v, f, b, 0);
    reconcile(v, f, &bs);
    v.send(
        3,
        &amount.to_le_bytes(),
        vec![
            r(f.market),
            w(f.user),
            s(f.owner.pubkey()),
            w(destination),
            w(f.vault),
            r(f.mint),
            r(TOKEN),
        ],
        &[&f.owner],
        true,
    );
    reconcile(v, f, &bs);
    eprintln!("exact lower-slot tie + capacity1 fallthrough + higher-score lower-reserve maxspend/freebalance selection passed");
}
