use litesvm::LiteSVM;
use solana_account::Account;
use solana_address::{address, Address};
use solana_instruction::{AccountMeta, Instruction};
use solana_keypair::Keypair;
use solana_message::Message;
use solana_signer::Signer;
use solana_transaction::Transaction;
#[path = "../../../../tests/support/parity.rs"]
mod parity;

#[allow(dead_code)]
#[path = "../../../src/abi.rs"]
mod abi;
use abi::*;
const TOKEN: Address = address!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA");
const T22: Address = address!("TokenzQdBNbLqP5VEhdkAS6EPFLC1PHnBqCXEpPxuEb");
fn k() -> Address {
    Keypair::new().pubkey()
}
fn r(pubkey: Address) -> AccountMeta {
    AccountMeta::new_readonly(pubkey, false)
}
fn w(pubkey: Address) -> AccountMeta {
    AccountMeta::new(pubkey, false)
}
fn s(pubkey: Address) -> AccountMeta {
    AccountMeta::new(pubkey, true)
}
fn wr(d: &mut [u8], at: usize, n: u64) {
    d[at..at + 8].copy_from_slice(&n.to_le_bytes());
}
fn rd(d: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(d[at..at + 8].try_into().unwrap())
}
fn numbers(n: &[u64]) -> Vec<u8> {
    n.iter().flat_map(|n| n.to_le_bytes()).collect()
}

struct Vm {
    svm: LiteSVM,
    program: Address,
    payer: Keypair,
    global: Address,
}
impl Vm {
    fn new() -> Self {
        let mut svm = LiteSVM::new();
        let program = Address::new_from_array([9; 32]);
        let artifact = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/deploy/fossa_dex.so");
        svm.add_program_from_file(program, artifact).unwrap();
        let payer = Keypair::new();
        svm.airdrop(&payer.pubkey(), 100_000_000_000).unwrap();
        let global = Address::find_program_address(&[b"protocol"], &program).0;
        let mut vm = Self {
            svm,
            program,
            payer,
            global,
        };
        let data = vm.data(program);
        let pd = Address::new_from_array(data[4..36].try_into().unwrap());
        let mut d = vm.data(pd);
        d[12] = 1;
        d[13..45].copy_from_slice(vm.payer.pubkey().as_ref());
        vm.set_data(pd, d);
        let user = vm.payer.pubkey();
        let mut payload = user.to_bytes().to_vec();
        payload.extend(user.to_bytes());
        let accounts = vec![
            s(user),
            w(global),
            r(program),
            r(pd),
            r(address!("11111111111111111111111111111111")),
        ];
        vm.call(
            "bootstrap global",
            14,
            payload.clone(),
            accounts.clone(),
            &[],
            true,
        );
        vm.call(
            "reject repeated bootstrap",
            14,
            payload,
            accounts,
            &[],
            false,
        );
        vm
    }
    fn put(&mut self, key: Address, owner: Address, data: Vec<u8>) {
        self.svm
            .set_account(
                key,
                Account {
                    lamports: self.svm.minimum_balance_for_rent_exemption(data.len()),
                    owner,
                    data,
                    ..Default::default()
                },
            )
            .unwrap();
    }
    fn mint(
        &mut self,
        authority: Option<Address>,
        supply: u64,
        decimals: u8,
        tax: Option<(u16, Option<Address>, Address)>,
    ) -> Address {
        let key = k();
        let mut d = vec![0; if tax.is_some() { 278 } else { 82 }];
        if let Some(a) = authority {
            d[0] = 1;
            d[4..36].copy_from_slice(a.as_ref());
        }
        wr(&mut d, 36, supply);
        d[44] = decimals;
        d[45] = 1;
        if let Some((bps, config, withdraw)) = tax {
            d[165] = 1;
            d[166] = 1;
            d[168..170].copy_from_slice(&108u16.to_le_bytes());
            if let Some(a) = config {
                d[170..202].copy_from_slice(a.as_ref());
            }
            d[202..234].copy_from_slice(withdraw.as_ref());
            for at in [250, 268] {
                wr(&mut d, at, u64::MAX);
                d[at + 8..at + 10].copy_from_slice(&bps.to_le_bytes());
            }
        }
        self.put(key, if tax.is_some() { T22 } else { TOKEN }, d);
        key
    }
    fn token(&mut self, mint: Address, owner: Address, balance: u64) -> Address {
        let m = self.svm.get_account(&mint).unwrap();
        let key = k();
        let mut d = vec![0; if m.data.len() > 82 { 178 } else { 165 }];
        d[..32].copy_from_slice(mint.as_ref());
        d[32..64].copy_from_slice(owner.as_ref());
        wr(&mut d, 64, balance);
        d[108] = 1;
        if d.len() > 165 {
            d[165] = 2;
            d[166] = 2;
            d[168] = 8;
        }
        self.put(key, m.owner, d);
        key
    }
    fn data(&self, key: Address) -> Vec<u8> {
        self.svm.get_account(&key).unwrap().data
    }
    fn set_data(&mut self, key: Address, d: Vec<u8>) {
        let mut a = self.svm.get_account(&key).unwrap();
        a.data = d;
        self.svm.set_account(key, a).unwrap();
    }
    #[track_caller]
    fn call(
        &mut self,
        label: &str,
        op: u8,
        p: Vec<u8>,
        accounts: Vec<AccountMeta>,
        signers: &[&Keypair],
        ok: bool,
    ) -> u64 {
        self.svm.expire_blockhash();
        let before: Vec<_> = accounts
            .iter()
            .filter(|a| a.is_writable && a.pubkey != self.payer.pubkey())
            .map(|a| (a.pubkey, self.svm.get_account(&a.pubkey)))
            .collect();
        let mut data = vec![op];
        data.extend(p);
        let ix = Instruction {
            program_id: self.program,
            accounts,
            data,
        };
        let mut keys = vec![&self.payer];
        for key in signers {
            if key.pubkey() != self.payer.pubkey() {
                keys.push(key);
            }
        }
        let tx = Transaction::new(
            &keys,
            Message::new(&[ix], Some(&self.payer.pubkey())),
            self.svm.latest_blockhash(),
        );
        let shadow = parity::run(&self.svm, &tx, false);
        let result = self.svm.send_transaction(tx);
        if let Some(shadow) = shadow {
            shadow.check(&self.svm, &result, label);
        }
        match result {
            Ok(m) => {
                assert!(ok, "{label} unexpectedly succeeded");
                if !label.starts_with("rounding cycle") {
                    println!("{label}: {} CU", m.compute_units_consumed);
                }
                m.compute_units_consumed
            }
            Err(e) => {
                assert!(!ok, "{label}: {e:?}");
                for (k, b) in before {
                    assert_eq!(
                        self.svm.get_account(&k),
                        b,
                        "{label}: failed transaction mutated state"
                    );
                }
                0
            }
        }
    }
}

struct PoolFixture {
    pool: Keypair,
    config: Keypair,
    operator: Address,
    authority: Address,
    ma: Address,
    mb: Address,
    ua: Address,
    ub: Address,
    va: Address,
    vb: Address,
    lp: Address,
    ulp: Address,
    ta: Address,
    tb: Address,
}
impl PoolFixture {
    fn new(vm: &mut Vm, curve: bool, mode: u8, tax_quote: bool) -> Self {
        let pool = Keypair::new();
        let config = Keypair::new();
        let user = vm.payer.pubkey();
        vm.put(pool.pubkey(), vm.program, vec![0; POOL_LEN]);
        vm.put(config.pubkey(), vm.program, vec![0; CONFIG_LEN]);
        let (authority, _) =
            Address::find_program_address(&[b"authority", pool.pubkey().as_ref()], &vm.program);
        let tax = if matches!(mode, REWARD | COMMUNITY) {
            Some((if mode == COMMUNITY { 300 } else { 100 }, None, authority))
        } else {
            None
        };
        let ma = vm.mint(
            if curve { Some(authority) } else { None },
            if curve { 0 } else { 1_000_000_000 },
            6,
            tax,
        );
        let mb = vm.mint(
            None,
            1_000_000_000_000,
            6,
            if tax_quote {
                Some((300, Some(user), user))
            } else {
                None
            },
        );
        let ua = vm.token(ma, user, if curve { 0 } else { 1_000_000_000 });
        let ub = vm.token(mb, user, 1_000_000_000_000);
        let va = vm.token(ma, authority, 0);
        let vb = vm.token(mb, authority, 0);
        let lp = vm.mint(Some(authority), 0, 9, None);
        let ulp = vm.token(lp, user, 0);
        let ta = if tax.is_some() { T22 } else { TOKEN };
        let tb = if tax_quote { T22 } else { TOKEN };
        let mut p = user.to_bytes().to_vec();
        p.extend(125u16.to_le_bytes());
        p.extend(10000u16.to_le_bytes());
        p.extend(0u16.to_le_bytes());
        vm.call(
            "config",
            0,
            p,
            vec![s(user), s(config.pubkey()), r(mb), r(tb), r(vm.global)],
            &[&config],
            true,
        );
        Self {
            pool,
            config,
            operator: user,
            authority,
            ma,
            mb,
            ua,
            ub,
            va,
            vb,
            lp,
            ulp,
            ta,
            tb,
        }
    }
    fn init(&self, user: Address) -> Vec<AccountMeta> {
        vec![
            s(user),
            r(self.config.pubkey()),
            s(self.pool.pubkey()),
            r(self.authority),
            w(self.ma),
            r(self.mb),
            w(self.ua),
            w(self.ub),
            w(self.va),
            w(self.vb),
            w(self.lp),
            w(self.ulp),
            r(self.ta),
            r(self.tb),
            r(TOKEN),
        ]
    }
    fn swap(&self, user: Address) -> Vec<AccountMeta> {
        vec![
            s(user),
            w(self.pool.pubkey()),
            r(self.authority),
            r(self.ma),
            r(self.mb),
            w(self.ua),
            w(self.ub),
            w(self.va),
            w(self.vb),
            r(self.ta),
            r(self.tb),
        ]
    }
    fn liquidity(&self, user: Address) -> Vec<AccountMeta> {
        vec![
            s(user),
            w(self.pool.pubkey()),
            r(self.authority),
            r(self.ma),
            r(self.mb),
            w(self.ua),
            w(self.ub),
            w(self.va),
            w(self.vb),
            w(self.lp),
            w(self.ulp),
            r(self.ta),
            r(self.tb),
            r(TOKEN),
        ]
    }
    fn grad(&self) -> Vec<AccountMeta> {
        vec![
            w(self.pool.pubkey()),
            r(self.authority),
            w(self.ma),
            r(self.mb),
            w(self.va),
            w(self.vb),
            r(self.lp),
            r(self.ta),
            r(self.tb),
            r(TOKEN),
            AccountMeta::new_readonly(self.operator, true),
        ]
    }
}
fn swap_data(direction: u8, amount: u64, limit: u64) -> Vec<u8> {
    let mut p = vec![direction];
    p.extend(numbers(&[amount, limit]));
    p
}

fn amm() {
    parity::fixture("dex_amm");
    let mut vm = Vm::new();
    let f = PoolFixture::new(&mut vm, false, STANDARD, true);
    let user = vm.payer.pubkey();
    vm.call(
        "initialize amm",
        1,
        numbers(&[500_000_000, 500_000_000, 1]),
        f.init(user),
        &[&f.pool],
        true,
    );
    assert_eq!(rd(&vm.data(f.pool.pubkey()), RESERVE_B), 485_000_000);
    vm.call(
        "amm taxed quote exact in",
        3,
        swap_data(1, 1_000_000, 1),
        f.swap(user),
        &[],
        true,
    );
    vm.call(
        "amm taxed quote exact out",
        4,
        swap_data(0, 1_000_000, 10_000_000),
        f.swap(user),
        &[],
        true,
    );
    vm.call(
        "add liquidity",
        6,
        numbers(&[10_000, 1_000_000, 1_000_000]),
        f.liquidity(user),
        &[],
        true,
    );
    vm.call(
        "remove liquidity",
        7,
        numbers(&[10_000, 1, 1]),
        f.liquidity(user),
        &[],
        true,
    );
    let reserves = (
        rd(&vm.data(f.pool.pubkey()), RESERVE_A),
        rd(&vm.data(f.pool.pubkey()), RESERVE_B),
    );
    let mut d = vm.data(f.va);
    let amount = rd(&d, 64);
    wr(&mut d, 64, amount + 1_000_000_000);
    vm.set_data(f.va, d);
    vm.call(
        "claim protocol fees",
        8,
        swap_data(0, 0, 0),
        f.swap(user),
        &[],
        true,
    );
    assert_eq!(
        reserves,
        (
            rd(&vm.data(f.pool.pubkey()), RESERVE_A),
            rd(&vm.data(f.pool.pubkey()), RESERVE_B)
        )
    );
    vm.call(
        "reject repeated claim",
        8,
        swap_data(0, 0, 0),
        f.swap(user),
        &[],
        false,
    );
    vm.call(
        "reject slippage",
        3,
        swap_data(0, 10_000, 1_000_000_000),
        f.swap(user),
        &[],
        false,
    );
    let mut wrong = f.swap(user);
    wrong[7] = w(f.ua);
    vm.call(
        "reject aliased vault",
        3,
        swap_data(0, 10_000, 1),
        wrong,
        &[],
        false,
    );
    vm.call(
        "reject zero swap",
        3,
        swap_data(0, 0, 0),
        f.swap(user),
        &[],
        false,
    );
    let mut keys = f.liquidity(user);
    keys[10] = w(f.ua);
    vm.call(
        "reject wrong lp",
        7,
        numbers(&[10_000, 1, 1]),
        keys,
        &[],
        false,
    );
    let mut mint = vm.data(f.mb);
    wr(&mut mint, 260, 1);
    mint[276..278].copy_from_slice(&100u16.to_le_bytes());
    vm.set_data(f.mb, mint);
    let mut clock: solana_clock::Clock = vm.svm.get_sysvar();
    clock.epoch = 0;
    vm.svm.set_sysvar(&clock);
    let before = rd(&vm.data(f.pool.pubkey()), RESERVE_B);
    vm.call(
        "quote fee before epoch",
        3,
        swap_data(1, 10000, 1),
        f.swap(user),
        &[],
        true,
    );
    assert_eq!(rd(&vm.data(f.pool.pubkey()), RESERVE_B) - before, 9578);
    clock.epoch = 1;
    vm.svm.set_sysvar(&clock);
    let before = rd(&vm.data(f.pool.pubkey()), RESERVE_B);
    vm.call(
        "quote fee after epoch",
        3,
        swap_data(1, 10000, 1),
        f.swap(user),
        &[],
        true,
    );
    assert_eq!(rd(&vm.data(f.pool.pubkey()), RESERVE_B) - before, 9776);
    let mut mint = vm.data(f.mb);
    mint[166..168].copy_from_slice(&14u16.to_le_bytes());
    vm.set_data(f.mb, mint);
    vm.call(
        "reject unsupported quote extension",
        3,
        swap_data(1, 10000, 1),
        f.swap(user),
        &[],
        false,
    );
}

fn benchmark_plain_amm() {
    parity::fixture("dex_plain_amm");
    let mut vm = Vm::new();
    let f = PoolFixture::new(&mut vm, false, STANDARD, false);
    let user = vm.payer.pubkey();
    vm.call("initialize amm", 1, numbers(&[500_000_000, 500_000_000, 1]), f.init(user), &[&f.pool], true);
    vm.call("amm exact in", 3, swap_data(1, 1_000_000, 1), f.swap(user), &[], true);
    vm.call("amm exact out", 4, swap_data(0, 1_000_000, 10_000_000), f.swap(user), &[], true);
    vm.call("add liquidity", 6, numbers(&[10_000, 1_000_000, 1_000_000]), f.liquidity(user), &[], true);
    vm.call("remove liquidity", 7, numbers(&[10_000, 1, 1]), f.liquidity(user), &[], true);
    vm.call("claim protocol fees", 8, swap_data(0, 0, 0), f.swap(user), &[], true);
}

fn curve(mode: u8, kind: u8) {
    parity::fixture(format!("dex_curve_mode={mode}_kind={kind}"));
    println!("curve kind={kind} mode={mode}");
    let mut vm = Vm::new();
    let f = PoolFixture::new(&mut vm, true, mode, true);
    let user = vm.payer.pubkey();
    let (va, vb, target, minimum) = match kind {
        0 => (125_000_000, 100_000_000, 200_000_000, 250_000_000),
        1 => (1_000_000_000, 100_000_000, 25_000_000, 100_000_000),
        2 => (1_000_000_000, 1, 125_000_000, 200_000_000),
        _ => unreachable!(),
    };
    let mut p = numbers(&[1_000_000_000, va, vb, target, minimum]);
    p.push(mode);
    p.extend(
        (if mode == STANDARD {
            0u16
        } else if mode == COMMUNITY {
            300
        } else {
            100
        })
        .to_le_bytes(),
    );
    p.push(kind);
    vm.call("initialize curve", 2, p, f.init(user), &[&f.pool], true);
    assert_eq!(&vm.data(f.ma)[..4], &[0, 0, 0, 0]);
    vm.call(
        "reject premature graduation",
        5,
        vec![],
        f.grad(),
        &[],
        false,
    );
    vm.call(
        "curve buy",
        3,
        swap_data(1, 10_000_000, 1),
        f.swap(user),
        &[],
        true,
    );
    vm.call(
        "curve buy exact out",
        4,
        swap_data(1, 100_000, 1_000_000),
        f.swap(user),
        &[],
        true,
    );
    vm.call(
        "curve sell",
        3,
        swap_data(0, 100_000, 1),
        f.swap(user),
        &[],
        true,
    );
    vm.call(
        "curve sell exact out",
        4,
        swap_data(0, 100_000, 10_000_000),
        f.swap(user),
        &[],
        true,
    );
    let remaining = target - rd(&vm.data(f.pool.pubkey()), RESERVE_B);
    let mut net = (remaining * 10_000).div_ceil(if mode == STANDARD { 9825 } else { 9875 });
    while net
        - (net * 125).div_ceil(10000)
        - if mode == STANDARD {
            (net * 50).div_ceil(10000)
        } else {
            0
        }
        < remaining
    {
        net += 1;
    }
    let gross = (net * 10000).div_ceil(9700);
    vm.call(
        "reject threshold overshoot",
        3,
        swap_data(1, gross + 1000, 1),
        f.swap(user),
        &[],
        false,
    );
    vm.call(
        "curve final buy",
        3,
        swap_data(1, gross, 1),
        f.swap(user),
        &[],
        true,
    );
    vm.call(
        "reject curve trade after threshold",
        3,
        swap_data(0, 100000, 1),
        f.swap(user),
        &[],
        false,
    );
    let before = vm.data(f.pool.pubkey());
    let old_a = rd(&before, RESERVE_A);
    let quote = rd(&before, RESERVE_B);
    let (numerator, denominator) = match kind {
        0 => (quote + vb, old_a + va),
        1 => (vb, va),
        2 => (vb * (1_000_000_000 - old_a), va),
        _ => unreachable!(),
    };
    let supply = rd(&vm.data(f.ma), 36);
    let caller = Keypair::new();
    vm.svm.airdrop(&caller.pubkey(), 1_000_000).unwrap();
    let operator = std::mem::replace(&mut vm.payer, caller);
    let mut missing = f.grad();
    missing.pop();
    vm.call(
        "reject missing graduation operator",
        5,
        vec![],
        missing,
        &[],
        false,
    );
    let mut unsigned = f.grad();
    *unsigned.last_mut().unwrap() = r(f.operator);
    vm.call(
        "reject unsigned graduation operator",
        5,
        vec![],
        unsigned,
        &[],
        false,
    );
    let mut wrong = f.grad();
    *wrong.last_mut().unwrap() = AccountMeta::new_readonly(vm.payer.pubkey(), true);
    vm.call(
        "reject wrong graduation operator",
        5,
        vec![],
        wrong,
        &[],
        false,
    );
    vm.payer = operator;
    vm.call("graduate", 5, vec![], f.grad(), &[], true);
    let after = vm.data(f.pool.pubkey());
    let matched = rd(&after, RESERVE_A);
    assert_eq!(after[STATUS], AMM);
    assert_eq!(rd(&after, RESERVE_B), quote);
    assert!(
        u128::from(quote) * u128::from(denominator) <= u128::from(matched) * u128::from(numerator)
    );
    assert!(
        u128::from(matched) * u128::from(numerator)
            < u128::from(quote) * u128::from(denominator) + u128::from(numerator)
    );
    assert_eq!(supply - rd(&vm.data(f.ma), 36), old_a - matched);
    assert_eq!(rd(&before, PROTOCOL_A), rd(&after, PROTOCOL_A));
    assert_eq!(rd(&before, CREATOR_A), rd(&after, CREATOR_A));
    vm.call("reject double graduate", 5, vec![], f.grad(), &[], false);
    vm.call(
        "graduated amm swap",
        3,
        swap_data(1, 1_000_000, 1),
        f.swap(user),
        &[],
        true,
    );
    vm.call(
        "curve protocol claim",
        8,
        swap_data(0, 0, 0),
        f.swap(user),
        &[],
        true,
    );
    vm.call(
        "curve creator claim",
        8,
        swap_data(1, 0, 0),
        f.swap(user),
        &[],
        mode == STANDARD,
    );
    if mode != STANDARD {
        let before = rd(&vm.data(f.ua), 64);
        vm.call(
            "collect native transfer tax",
            10,
            vec![],
            vec![
                w(f.pool.pubkey()),
                r(f.authority),
                w(f.ma),
                w(f.ua),
                r(T22),
                w(f.va),
            ],
            &[],
            true,
        );
        assert!(rd(&vm.data(f.ua), 64) > before);
        assert!(rd(&vm.data(f.pool.pubkey()), TAX_COLLECTED) > 0);
        vm.call(
            "reject repeated tax claim",
            10,
            vec![],
            vec![
                w(f.pool.pubkey()),
                r(f.authority),
                w(f.ma),
                w(f.ua),
                r(T22),
                w(f.va),
            ],
            &[],
            false,
        );
    }
}

fn gross_effective(amount: u64, standard: bool, tax_bps: u64) -> u64 {
    let mut net = (amount * 10000).div_ceil(if standard { 9825 } else { 9875 });
    while net
        - (net * 125).div_ceil(10000)
        - if standard {
            (net * 50).div_ceil(10000)
        } else {
            0
        }
        < amount
    {
        net += 1;
    }
    (net * 10000).div_ceil(10000 - tax_bps)
}

fn terminal_dust_and_vesting() {
    parity::fixture("dex_terminal_dust_and_vesting");
    let mut vm = Vm::new();
    let f = PoolFixture::new(&mut vm, true, STANDARD, true);
    let user = vm.payer.pubkey();
    let mut p = numbers(&[1_000_000, 1, 1_000_000, 1_000_000_000, 500_000]);
    p.extend([STANDARD, 0, 0, 1]);
    vm.call("fixed dust curve", 2, p, f.init(user), &[&f.pool], true);
    let vest = Keypair::new();
    vm.put(vest.pubkey(), vm.program, vec![0; VESTING_LEN]);
    vm.call(
        "create vesting",
        11,
        numbers(&[100_000, 100, 100]),
        vec![s(user), w(f.pool.pubkey()), s(vest.pubkey()), r(user)],
        &[&vest],
        true,
    );
    let claim = vec![
        s(user),
        w(f.pool.pubkey()),
        w(vest.pubkey()),
        r(f.authority),
        r(f.ma),
        w(f.va),
        w(f.ua),
        r(f.ta),
    ];
    vm.call(
        "reject vesting before graduation",
        12,
        numbers(&[0]),
        claim.clone(),
        &[],
        false,
    );
    vm.call(
        "dust first buy",
        3,
        swap_data(1, gross_effective(999_999_999, true, 300), 1),
        f.swap(user),
        &[],
        true,
    );
    let gift = Keypair::new();
    vm.put(gift.pubkey(), vm.program, vec![0; VESTING_LEN]);
    vm.call(
        "reject vesting after trading",
        11,
        numbers(&[1, 0, 0]),
        vec![s(user), w(f.pool.pubkey()), s(gift.pubkey()), r(user)],
        &[&gift],
        false,
    );
    assert_eq!(rd(&vm.data(f.pool.pubkey()), RESERVE_B), 999_999_999);
    let last = gross_effective(1, true, 300);
    vm.call(
        "reject dust with nonzero minout",
        3,
        swap_data(1, last, 1),
        f.swap(user),
        &[],
        false,
    );
    vm.call(
        "explicit terminal dust topup",
        3,
        swap_data(1, last, 0),
        f.swap(user),
        &[],
        true,
    );
    let before = rd(&vm.data(f.ma), 36);
    vm.call(
        "graduate burns surplus preserves vest",
        5,
        vec![],
        f.grad(),
        &[],
        true,
    );
    assert!(rd(&vm.data(f.ma), 36) < before);
    assert_eq!(rd(&vm.data(f.pool.pubkey()), VESTED_BASE), 100_000);
    let start = rd(&vm.data(f.pool.pubkey()), GRADUATED_AT);
    let mut clock: solana_clock::Clock = vm.svm.get_sysvar();
    clock.unix_timestamp = (start + 99) as i64;
    vm.svm.set_sysvar(&clock);
    vm.call(
        "reject vest before cliff",
        12,
        numbers(&[0]),
        claim.clone(),
        &[],
        false,
    );
    clock.unix_timestamp = (start + 150) as i64;
    vm.svm.set_sysvar(&clock);
    vm.call(
        "claim half vested",
        12,
        numbers(&[50_000]),
        claim.clone(),
        &[],
        true,
    );
    assert_eq!(rd(&vm.data(f.pool.pubkey()), VESTED_BASE), 50_000);
    clock.unix_timestamp = (start + 200) as i64;
    vm.svm.set_sysvar(&clock);
    vm.call(
        "claim remaining vested",
        12,
        numbers(&[50_000]),
        claim.clone(),
        &[],
        true,
    );
    vm.call(
        "reject repeated vest claim",
        12,
        numbers(&[0]),
        claim,
        &[],
        false,
    );
    assert_eq!(rd(&vm.data(f.pool.pubkey()), VESTED_BASE), 0);
}

fn model_graduation_roundtrip() {
    parity::fixture("dex_model_graduation_roundtrip");
    let mut vm = Vm::new();
    let f = PoolFixture::new(&mut vm, false, STANDARD, false);
    let user = vm.payer.pubkey();
    let mut accounts = f.init(user);
    accounts.push(s(user));
    accounts.push(r(vm.global));
    vm.call(
        "reject migration arbitrage config",
        9,
        numbers(&[
            1_000_000_000,
            10_000_000_000,
            1_000_000_000,
            80_000_000,
            100_000_000,
        ]),
        accounts.clone(),
        &[&f.pool],
        false,
    );
    vm.call(
        "model curve",
        9,
        numbers(&[1_000_000_000, 0, 1_000_000_000, 80_000_000, 100_000_000]),
        accounts,
        &[&f.pool],
        true,
    );
    vm.call(
        "model first buyer",
        3,
        swap_data(1, gross_effective(40_000_000, false, 0), 1),
        f.swap(user),
        &[],
        true,
    );
    let attacker = Keypair::new();
    vm.svm.airdrop(&attacker.pubkey(), 1_000_000).unwrap();
    let aa = vm.token(f.ma, attacker.pubkey(), 0);
    let ab = vm.token(f.mb, attacker.pubkey(), 1_000_000_000);
    let mut swap = f.swap(attacker.pubkey());
    swap[5] = w(aa);
    swap[6] = w(ab);
    vm.call(
        "model terminal buyer",
        3,
        swap_data(1, gross_effective(40_000_000, false, 0), 1),
        swap.clone(),
        &[&attacker],
        true,
    );
    vm.call("model graduate burn", 5, vec![], f.grad(), &[], true);
    let acquired = rd(&vm.data(aa), 64);
    vm.call(
        "sell across graduation",
        3,
        swap_data(0, acquired, 1),
        swap,
        &[&attacker],
        true,
    );
    assert!(
        rd(&vm.data(ab), 64) < 1_000_000_000,
        "graduation roundtrip must lose fees, not gain quote"
    );
    assert_eq!(rd(&vm.data(f.pool.pubkey()), CREATOR_A), 0);
    assert_eq!(rd(&vm.data(f.pool.pubkey()), CREATOR_B), 0);
}

fn admin_boundaries() {
    parity::fixture("dex_admin_boundaries");
    let mut vm = Vm::new();
    let f = PoolFixture::new(&mut vm, false, STANDARD, false);
    let user = vm.payer.pubkey();
    let pending = PoolFixture::new(&mut vm, false, STANDARD, false);
    let impostor = Keypair::new();
    vm.svm.airdrop(&impostor.pubkey(), 1_000_000).unwrap();
    let forged = Keypair::new();
    vm.put(forged.pubkey(), vm.program, vec![0; CONFIG_LEN]);
    let mut payload = impostor.pubkey().to_bytes().to_vec();
    payload.extend([125, 0, 16, 39, 0, 0]);
    vm.call(
        "reject fee recipient bypass",
        0,
        payload,
        vec![
            s(impostor.pubkey()),
            s(forged.pubkey()),
            r(f.mb),
            r(f.tb),
            r(vm.global),
        ],
        &[&impostor, &forged],
        false,
    );
    let config_accounts = vec![
        s(user),
        w(f.config.pubkey()),
        r(f.mb),
        r(f.tb),
        r(vm.global),
    ];
    vm.call(
        "disable quote config",
        13,
        vec![0],
        config_accounts.clone(),
        &[],
        true,
    );
    vm.call(
        "reject disabled quote",
        1,
        numbers(&[500_000_000, 500_000_000, 1]),
        f.init(user),
        &[&f.pool],
        false,
    );
    vm.call(
        "enable quote config",
        13,
        vec![1],
        config_accounts,
        &[],
        true,
    );
    let mut model = f.init(user);
    model.push(s(impostor.pubkey()));
    model.push(r(vm.global));
    vm.call(
        "reject fake model profile",
        9,
        numbers(&[1_000_000_000, 0, 1_000_000_000, 80_000_000, 100_000_000]),
        model,
        &[&f.pool, &impostor],
        false,
    );
    vm.call(
        "admin test pool",
        1,
        numbers(&[500_000_000, 500_000_000, 1]),
        f.init(user),
        &[&f.pool],
        true,
    );
    let mut payload = impostor.pubkey().to_bytes().to_vec();
    payload.extend(impostor.pubkey().to_bytes());
    vm.call(
        "reject unsigned admin handoff",
        15,
        payload.clone(),
        vec![s(user), r(impostor.pubkey()), w(vm.global)],
        &[],
        false,
    );
    vm.call(
        "admin handoff",
        15,
        payload,
        vec![s(user), s(impostor.pubkey()), w(vm.global)],
        &[&impostor],
        true,
    );
    let mut old_model = pending.init(user);
    old_model.push(s(user));
    old_model.push(r(vm.global));
    vm.call(
        "reject former model admin",
        9,
        numbers(&[1_000_000_000, 0, 1_000_000_000, 80_000_000, 100_000_000]),
        old_model,
        &[&pending.pool],
        false,
    );
    let mut current_model = pending.init(user);
    current_model.push(s(impostor.pubkey()));
    current_model.push(r(vm.global));
    vm.call(
        "current admin stale quote config",
        9,
        numbers(&[1_000_000_000, 0, 1_000_000_000, 80_000_000, 100_000_000]),
        current_model,
        &[&pending.pool, &impostor],
        true,
    );
    vm.call(
        "reject former admin",
        16,
        vec![],
        vec![s(user), w(f.pool.pubkey()), r(vm.global)],
        &[],
        false,
    );
    vm.call(
        "update pool recipients",
        16,
        vec![],
        vec![s(impostor.pubkey()), w(f.pool.pubkey()), r(vm.global)],
        &[&impostor],
        true,
    );
    assert_eq!(
        &vm.data(f.pool.pubkey())[TREASURY..TREASURY + 32],
        impostor.pubkey().as_ref()
    );
}

fn rounding_cycles_preserve_graduation() {
    parity::fixture("dex_rounding_cycles_preserve_graduation");
    let mut vm = Vm::new();
    let f = PoolFixture::new(&mut vm, true, STANDARD, false);
    let user = vm.payer.pubkey();
    let mut p = numbers(&[1_000_000, 1_000_000_000, 1, 125, 200_000]);
    p.extend([STANDARD, 0, 0, 2]);
    vm.call(
        "rounding regression curve",
        2,
        p,
        f.init(user),
        &[&f.pool],
        true,
    );
    for cycle in 0..=116 {
        let before = rd(&vm.data(f.ua), 64);
        vm.call(
            "rounding cycle buy",
            3,
            swap_data(1, gross_effective(2, true, 0), 1),
            f.swap(user),
            &[],
            true,
        );
        let bought = rd(&vm.data(f.ua), 64) - before;
        vm.call(
            "rounding cycle sell",
            3,
            swap_data(0, bought, 1),
            f.swap(user),
            &[],
            cycle < 116,
        );
    }
    let remaining = 125 - rd(&vm.data(f.pool.pubkey()), RESERVE_B);
    assert_eq!(remaining, 7);
    vm.call(
        "rounding-safe terminal buy",
        3,
        swap_data(1, gross_effective(remaining, true, 0), 1),
        f.swap(user),
        &[],
        true,
    );
    vm.call("rounding-safe graduation", 5, vec![], f.grad(), &[], true);
}

fn main() {
    amm();
    for kind in 0..3 {
        curve(STANDARD, kind);
    }
    curve(REWARD, 0);
    curve(COMMUNITY, 0);
    terminal_dust_and_vesting();
    model_graduation_roundtrip();
    admin_boundaries();
    rounding_cycles_preserve_graduation();
    if std::env::var_os("FOSSA_BENCH_CSV").is_some() {
        benchmark_plain_amm();
        for mode in [REWARD, COMMUNITY] {
            for kind in 1..3 {
                curve(mode, kind);
            }
        }
    }
    println!("all dex SBF scenarios passed");
}
