use super::*;
use base64::{engine::general_purpose::STANDARD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

const BUCKET_LEN: usize = 736;
const RECEIPT_LEN: usize = 120;
const CLAIM_RECEIPT_OFFSET: usize = 37;

fn prototype_path(root: &Path) -> PathBuf {
    root.join(std::env::var_os("FOSSA_RECEIPT_PROTOTYPE").or_else(|| std::env::var_os("FOSSA_SBF"))
        .map(PathBuf::from).unwrap_or_else(|| "target/deploy/inference_router_program.so".into()))
}

fn baseline_path(root: &Path) -> PathBuf {
    root.join(std::env::var_os("FOSSA_RECEIPT_BASELINE").map(PathBuf::from)
        .unwrap_or_else(|| "target/receipt-bench/baseline/inference_router_program.so".into()))
}

#[derive(serde::Deserialize)]
struct Bench {
    requests_per_day: u64,
    epoch_seconds: u32,
    settlement_seconds: Vec<u64>,
    offer_counts: Vec<usize>,
    samples: usize,
    scale_samples: usize,
    illustrative_sol_usd: f64,
}

fn settings(raw: &str) -> Result<Bench, String> {
    #[derive(serde::Deserialize)]
    struct Config { receipt_benchmark: Bench }
    let bench = toml::from_str::<Config>(raw).map_err(|error| error.to_string())?.receipt_benchmark;
    if !(1..=1_000_000_000).contains(&bench.requests_per_day)
        || !(1..=1800).contains(&bench.epoch_seconds)
        || bench.offer_counts.is_empty() || bench.offer_counts.iter().any(|count| !(1..=4096).contains(count))
        || !(4..=65_534).contains(&bench.samples) || !(4..=65_534).contains(&bench.scale_samples)
        || bench.settlement_seconds.is_empty() || bench.settlement_seconds.iter().any(|delay| !(1..=86_400).contains(delay))
        || !bench.illustrative_sol_usd.is_finite() || bench.illustrative_sol_usd < 0.0
    { return Err("receipt benchmark settings exceed supported bounds".into()); }
    Ok(bench)
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_program_runtime::{declare_process_instruction, solana_sbpf::program::BuiltinFunctionDefinition};
    const START: i64 = 1_800_000_000;

    declare_process_instruction!(ClaimProxy, 1, |context| {
        let current = context.transaction_context.get_current_instruction_context()?;
        let last = current.get_number_of_instruction_accounts() - 1;
        let mut accounts = Vec::new();
        for index in 0..last {
            accounts.push(AccountMeta {
                pubkey: *current.get_key_of_instruction_account(index)?,
                is_signer: current.is_instruction_account_signer(index)?,
                is_writable: current.is_instruction_account_writable(index)?,
            });
        }
        let instruction = Instruction {
            program_id: *current.get_key_of_instruction_account(last)?,
            accounts,
            data: current.get_instruction_data().to_vec(),
        };
        context.native_invoke_signed(instruction, &[])
    });

    fn fixture() -> (Vm, Fixture, Keypair) {
        fixture_for_token(false)
    }
    fn fixture_for_token(legacy: bool) -> (Vm, Fixture, Keypair) {
        let mut v = Vm::new(legacy);
        let f = setup(&mut v, 1, 7);
        v.svm.airdrop(&f.owner.pubkey(), 100_000_000_000).unwrap();
        v.svm.airdrop(&f.offers[0].provider.pubkey(), 1_000_000_000).unwrap();
        clock(&mut v, START);
        let bucket = Keypair::new();
        init_bucket(&mut v, &f, &bucket, 0, 900, true);
        (v, f, bucket)
    }
    fn close(v: &Vm, bucket: Address, recipient: Address, authority: Address) -> Instruction {
        ix(v.program, 27, &[], vec![w(bucket), w(recipient), s(authority)])
    }

    #[test]
    fn benchmark_defaults_are_self_contained_and_invalid_bounds_fail_early() {
        let raw = include_str!("../config.example.toml");
        assert!(settings(raw).is_ok());
        for (field, value) in [
            ("epoch_seconds = 900", "epoch_seconds = 0"),
            ("samples = 64", "samples = -1"),
            ("samples = 64", "samples = 1"),
            ("scale_samples = 8", "scale_samples = 0"),
            ("offer_counts = [1, 32]", "offer_counts = [4097]"),
            ("settlement_seconds = [1, 10, 30, 300]", "settlement_seconds = []"),
            ("illustrative_sol_usd = 100.0", "illustrative_sol_usd = nan"),
        ] {
            assert!(settings(&raw.replace(field, value)).is_err(), "{field}");
        }
    }

    #[test]
    fn receipt_proofs_cover_frontier_boundaries_and_reject_tampering_and_replay() {
        for legacy in [false, true] {
            let (mut v, f, bucket) = fixture_for_token(legacy);
            let booking = Keypair::new();
            let mut receipts = Vec::new();
            let mut leaves = Vec::new();
            for index in 0..31 {
                book_one(&mut v, &f, &booking, true, index == 0);
                let claim = claim_one(&mut v, &f, booking.pubkey(), true, bucket.pubkey());
                let receipt = receipt_from_log(&claim, bucket.pubkey(), START, index);
                leaves.push(leaf(v.program, bucket.pubkey(), START, index, &receipt));
                receipts.push(receipt);
                let count = index as usize + 1;
                if ![1, 2, 3, 4, 7, 8, 15, 16, 31].contains(&count) { continue; }
                let original = v.svm.clone();
                for target in [0, count / 2, count - 1] {
                    v.svm = original.clone();
                    let (valid, case) = dispute_ix(&v, &f, bucket.pubkey(), START, target, &receipts[target], &leaves);
                    let mut forged = valid.clone();
                    forged.data[16 + 96] ^= 1;
                    execute(&mut v, &f.owner, &[forged], &[], false);
                    if valid.data.len() > 16 + RECEIPT_LEN {
                        let mut forged = valid.clone();
                        *forged.data.last_mut().unwrap() ^= 1;
                        execute(&mut v, &f.owner, &[forged], &[], false);
                    }
                    execute(&mut v, &f.owner, &[valid.clone()], &[], true);
                    assert_eq!(rd64(&v.data(bucket.pubkey()), 216), count as u64 - 1);
                    assert_eq!(rd64(&v.data(case), 160), rd64(&receipts[target], 104));
                    execute(&mut v, &f.owner, &[valid], &[], false);
                }
                v.svm = original;
            }
        }
    }

    #[test]
    fn receipt_bucket_accepts_last_leaf_and_rejects_overflow_atomically() {
        let (mut v, f, bucket) = fixture();
        let booking = Keypair::new();
        book_one(&mut v, &f, &booking, true, true);
        let mut data = v.data(bucket.pubkey());
        u64at(&mut data, 200, (1 << 16) - 2);
        data[224..].fill(1);
        v.put(bucket.pubkey(), v.program, data);
        claim_one(&mut v, &f, booking.pubkey(), true, bucket.pubkey());
        assert_eq!(rd64(&v.data(bucket.pubkey()), 200), (1 << 16) - 1);
        book_one(&mut v, &f, &booking, true, false);
        let (claim, _) = claim_ix(&v, &f, booking.pubkey(), true, bucket.pubkey(), rd64(&v.data(booking.pubkey()), 152));
        execute(&mut v, &f.offers[0].provider, &[claim], &[], false);
        assert_eq!(v.data(booking.pubkey())[148], 0);
    }

    #[test]
    fn receipt_claim_validates_published_body_context_and_encoding_atomically() {
        let (mut v, f, bucket) = fixture();
        let booking = Keypair::new();
        book_one(&mut v, &f, &booking, true, true);
        let (claim, _) = claim_ix(&v, &f, booking.pubkey(), true, bucket.pubkey(), rd64(&v.data(booking.pubkey()), 152));
        assert_eq!(claim.data.len(), 157);
        let provider = f.offers[0].provider.insecure_clone();
        for offset in [21, 29, 37, 69, 101, 133, 141, 149] {
            let mut forged = claim.clone();
            forged.data[offset] ^= 1;
            execute(&mut v, &provider, &[forged], &[], false);
        }
        for length in [21, 141, 156, 158] {
            let mut malformed = claim.clone();
            malformed.data.resize(length, 0);
            execute(&mut v, &provider, &[malformed], &[], false);
        }
        execute(&mut v, &provider, &[claim], &[], true);
        book_one(&mut v, &f, &booking, true, false);
        let (mut zero, _) = claim_ix(&v, &f, booking.pubkey(), true, bucket.pubkey(), rd64(&v.data(booking.pubkey()), 152));
        zero.data[1..13].fill(0);
        execute(&mut v, &provider, &[zero.clone()], &[], false);
        zero.data[CLAIM_RECEIPT_OFFSET + 104..CLAIM_RECEIPT_OFFSET + 112].fill(0);
        zero.data[21..37].fill(0xff);
        zero.accounts.last_mut().unwrap().is_writable = false;
        let before = v.data(bucket.pubkey());
        execute(&mut v, &provider, &[zero], &[], true);
        assert_eq!(v.data(bucket.pubkey()), before);
        assert_eq!(v.data(booking.pubkey())[148], 5);
    }

    #[test]
    fn receipt_claims_recover_from_transaction_data_when_logs_are_truncated() {
        let (mut v, f, bucket) = fixture();
        let booking = Keypair::new();
        book_one(&mut v, &f, &booking, true, true);
        v.svm = v.svm.clone().with_log_bytes_limit(Some(0));
        let mut receipts = Vec::new();
        let mut leaves = Vec::new();
        for index in 0..2 {
            if index > 0 {
                book_one(&mut v, &f, &booking, true, false);
            }
            let result = claim_one(&mut v, &f, booking.pubkey(), true, bucket.pubkey());
            assert!(!result.logs.iter().any(|log| log.starts_with("Program data: ")));
            let data = result.instruction_data.iter().find(|data| data.len() == 157 && data[0] == 7).unwrap();
            assert_eq!(rd64(data, 21), START as u64);
            assert_eq!(rd64(data, 29), index as u64);
            let receipt = data[CLAIM_RECEIPT_OFFSET..].to_vec();
            leaves.push(leaf(v.program, bucket.pubkey(), START, index as u64, &receipt));
            receipts.push(receipt);
        }
        assert_ne!(&receipts[0][96..104], &receipts[1][96..104]);
        let (dispute, case) = dispute_ix(&v, &f, bucket.pubkey(), START, 0, &receipts[0], &leaves);
        execute(&mut v, &f.owner, &[dispute], &[], true);
        assert_eq!(rd64(&v.data(case), 160), rd64(&receipts[0], 104));
        assert_eq!(rd64(&v.data(bucket.pubkey()), 216), 1);
    }

    #[test]
    fn receipt_claim_requires_top_level_instruction_and_legacy_claim_allows_cpi() {
        for (reusable, zero) in [(true, false), (true, true), (false, false)] {
            let (mut v, f, bucket) = fixture();
            let proxy = key();
            v.svm.add_builtin(proxy, ClaimProxy::register);
            let booking = Keypair::new();
            book_one(&mut v, &f, &booking, reusable, true);
            let (mut claim, _) = claim_ix(&v, &f, booking.pubkey(), reusable, bucket.pubkey(), rd64(&v.data(booking.pubkey()), 152));
            if zero {
                claim.data[1..13].fill(0);
                claim.data[CLAIM_RECEIPT_OFFSET + 104..CLAIM_RECEIPT_OFFSET + 112].fill(0);
            }
            let mut nested = claim.clone();
            nested.program_id = proxy;
            nested.accounts.push(r(v.program));
            let provider = f.offers[0].provider.insecure_clone();
            let result = execute(&mut v, &provider, &[nested], &[], !reusable);
            assert!(result.logs.iter().any(|log| log == &format!("Program {} invoke [2]", v.program)));
            if reusable {
                assert!(result.logs.iter().any(|log| log == &format!("Program {} failed: custom program error: 0x4", v.program)));
                execute(&mut v, &provider, &[claim], &[], true);
            }
        }
    }

    #[test]
    fn receipt_claim_batches_bind_sequential_indices_and_fit_packet_limit() {
        let (mut v, f, bucket) = fixture();
        let mut claims = Vec::new();
        for index in 0..3 {
            let booking = Keypair::new();
            book_one(&mut v, &f, &booking, true, true);
            let (mut claim, _) = claim_ix(&v, &f, booking.pubkey(), true, bucket.pubkey(), rd64(&v.data(booking.pubkey()), 152));
            u64at(&mut claim.data, 29, index);
            claims.push(claim);
        }
        let provider = f.offers[0].provider.insecure_clone();
        let bytes = |claims: &[Instruction]| {
            let mut data = vec![2];
            data.extend_from_slice(&1_400_000u32.to_le_bytes());
            let mut instructions = vec![Instruction {
                program_id: address!("ComputeBudget111111111111111111111111111111"),
                accounts: vec![], data,
            }];
            instructions.extend_from_slice(claims);
            let tx = Transaction::new(&[&provider], Message::new(&instructions, Some(&provider.pubkey())), v.svm.latest_blockhash());
            1 + tx.signatures.len() * 64 + tx.message.serialize().len()
        };
        let single = bytes(&claims[..1]);
        let shared = bytes(&claims);
        let mut distinct = claims.clone();
        for claim in &mut distinct[1..] {
            claim.accounts[3].pubkey = key();
            claim.accounts[6].pubkey = key();
        }
        let pair = bytes(&distinct[..2]);
        let triple = bytes(&distinct);
        assert!(pair <= 1232 && shared <= 1232 && triple > 1232);
        let result = execute(&mut v, &provider, &claims, &[], true);
        assert_eq!(rd64(&v.data(bucket.pubkey()), 200), 3);
        assert_eq!(result.fee, 5_000);
        eprintln!("receipt claim bytes: single={single}, distinct-users pair={pair}, distinct-users triple={triple}, shared-user triple={shared}; batch fee={} CU={}", result.fee, result.cu);
    }

    #[test]
    fn receipt_claim_compute_budget_covers_full_frontier_carry_for_both_token_programs() {
        for legacy in [false, true] {
            let (mut v, f, bucket) = fixture_for_token(legacy);
            let booking = Keypair::new();
            book_one(&mut v, &f, &booking, true, true);
            let mut frontier = v.data(bucket.pubkey());
            u64at(&mut frontier, 200, (1 << 15) - 1);
            frontier[224..].fill(1);
            v.put(bucket.pubkey(), v.program, frontier);
            let result = claim_one(&mut v, &f, booking.pubkey(), true, bucket.pubkey());
            assert_eq!(rd64(&v.data(bucket.pubkey()), 200), 1 << 15);
            assert!(result.cu <= 16_000, "15-node receipt carry used {} CU", result.cu);
            eprintln!("receipt 15-node carry token={} CU={} bytes={}", if legacy { "legacy" } else { "p-token" }, result.cu, result.bytes);
        }
    }

    #[test]
    #[ignore = "requires FOSSA_PREVIOUS_RECEIPT_SBF with the previous 20-byte claim ABI"]
    fn receipt_claim_cost_comparison_with_previous_sbf() {
        let previous = std::fs::read(std::env::var_os("FOSSA_PREVIOUS_RECEIPT_SBF").expect("previous receipt SBF path required")).unwrap();
        for legacy in [false, true] {
            for count in [0, (1 << 15) - 1] {
                let (mut v, f, bucket) = fixture_for_token(legacy);
                let booking = Keypair::new();
                book_one(&mut v, &f, &booking, true, true);
                let mut frontier = v.data(bucket.pubkey());
                u64at(&mut frontier, 200, count);
                frontier[224..].fill(1);
                v.put(bucket.pubkey(), v.program, frontier);
                let mut old = Vm {
                    svm: v.svm.clone(), program: v.program, payer: v.payer.insecure_clone(),
                    last_error: None, executions: 0, succeeded: [0; 23], rejected: [0; 23],
                    packet_max: [0; 23], claim_seconds: 0,
                };
                old.svm.add_program(old.program, &previous).unwrap();
                let (claim, _) = claim_ix(&v, &f, booking.pubkey(), true, bucket.pubkey(), rd64(&v.data(booking.pubkey()), 152));
                let mut old_claim = claim.clone(); old_claim.data.truncate(21);
                let provider = f.offers[0].provider.insecure_clone();
                let before = execute(&mut old, &provider, &[old_claim], &[], true);
                let after = execute(&mut v, &provider, &[claim], &[], true);
                for account in [booking.pubkey(), bucket.pubkey(), f.user, f.capacity, f.vault, f.offers[0].payout] {
                    assert_eq!(old.data(account), v.data(account));
                }
                eprintln!("receipt before/after token={} count={count} old_CU={} new_CU={} old_bytes={} new_bytes={} fee={}", if legacy { "legacy" } else { "p-token" }, before.cu, after.cu, before.bytes, after.bytes, after.fee);
            }
        }
    }

    #[test]
    fn receipt_close_rejects_invalid_lifecycle_and_authorization_atomically() {
        let (mut v, f, bucket) = fixture();
        clock(&mut v, START + 901);
        let expire = expire_ix(&v, &f, bucket.pubkey());
        execute(&mut v, &f.owner, &[expire], &[], true);
        let original = v.data(bucket.pubkey());
        let instruction = close(&v, bucket.pubkey(), f.owner.pubkey(), f.offers[0].provider.pubkey());
        let provider = f.offers[0].provider.insecure_clone();
        for variant in 0..8 {
            let mut data = original.clone();
            match variant {
                0 => data[173] = 0,
                1 => u64at(&mut data, 208, 1),
                2 => u64at(&mut data, 216, 1),
                3 => u64at(&mut data, 184, (START + 902) as u64),
                4 => u64at(&mut data, 192, (START + 901) as u64),
                5 => data[0] ^= 1,
                6 => data.truncate(735),
                7 => data[72..104].copy_from_slice(key().as_ref()),
                _ => unreachable!(),
            }
            v.put(bucket.pubkey(), v.program, data);
            execute(&mut v, &provider, &[instruction.clone()], &[], false);
        }
        v.put(bucket.pubkey(), v.program, original.clone());
        let admin = v.payer.insecure_clone();
        let unauthorized = close(&v, bucket.pubkey(), f.owner.pubkey(), admin.pubkey());
        execute(&mut v, &admin, &[unauthorized], &[], false);
        let redirected = close(&v, bucket.pubkey(), provider.pubkey(), provider.pubkey());
        execute(&mut v, &provider, &[redirected], &[], false);
        let mut unsigned = instruction.clone(); unsigned.accounts[2].is_signer = false;
        execute(&mut v, &admin, &[unsigned], &[], false);
        for index in [0, 1] {
            let mut readonly = instruction.clone(); readonly.accounts[index].is_writable = false;
            execute(&mut v, &provider, &[readonly], &[], false);
        }
        let mut payload = instruction.clone(); payload.data.push(1);
        execute(&mut v, &provider, &[payload], &[], false);
        let mut missing = instruction.clone(); missing.accounts.pop();
        execute(&mut v, &admin, &[missing], &[], false);
        let alias = close(&v, bucket.pubkey(), bucket.pubkey(), provider.pubkey());
        execute(&mut v, &provider, &[alias], &[], false);
        v.put(bucket.pubkey(), SYSTEM, original);
        execute(&mut v, &provider, &[instruction], &[], false);
    }

    #[test]
    fn receipt_close_refunds_original_payer_and_allows_provider_or_payer_authority() {
        for owner_authority in [false, true] {
            let (mut v, f, bucket) = fixture();
            clock(&mut v, START + 901);
            let expire = expire_ix(&v, &f, bucket.pubkey());
            execute(&mut v, &f.owner, &[expire], &[], true);
            let provider = f.offers[0].provider.insecure_clone();
            let payer = if owner_authority { &f.owner } else { &provider };
            let recipient_before = v.svm.get_account(&f.owner.pubkey()).unwrap().lamports;
            let rent = v.svm.get_account(&bucket.pubkey()).unwrap().lamports;
            let instruction = close(&v, bucket.pubkey(), f.owner.pubkey(), payer.pubkey());
            let result = execute(&mut v, payer, &[instruction.clone()], &[], true);
            assert_eq!(v.svm.get_account(&f.owner.pubkey()).unwrap().lamports, recipient_before + rent - if owner_authority { result.fee } else { 0 });
            assert!(v.svm.get_account(&bucket.pubkey()).is_none_or(|account| account.lamports == 0));
            execute(&mut v, payer, &[instruction], &[], false);
        }
    }

    #[test]
    fn receipt_dispute_resolves_after_bucket_closure_or_reuse() {
        for retire in [false, true] {
            let (mut v, f, bucket) = fixture();
            let booking = Keypair::new();
            book_one(&mut v, &f, &booking, true, true);
            let claim = claim_one(&mut v, &f, booking.pubkey(), true, bucket.pubkey());
            let receipt = receipt_from_log(&claim, bucket.pubkey(), START, 0);
            let leaves = [leaf(v.program, bucket.pubkey(), START, 0, &receipt)];
            let (dispute, case) = dispute_ix(&v, &f, bucket.pubkey(), START, 0, &receipt, &leaves);
            execute(&mut v, &f.owner, &[dispute.clone()], &[], true);
            let provider = f.offers[0].provider.insecure_clone();
            let closing = close(&v, bucket.pubkey(), f.owner.pubkey(), provider.pubkey());
            execute(&mut v, &provider, &[closing.clone()], &[], false);
            let deadline = rd64(&v.data(bucket.pubkey()), 192) as i64;
            clock(&mut v, deadline + 1);
            let expire = expire_ix(&v, &f, bucket.pubkey());
            execute(&mut v, &f.owner, &[expire], &[], true);
            let capacity = v.data(f.capacity);
            let charge = rd64(&v.data(case), 160);
            assert_eq!(rd64(&capacity, HEADER_LEN + 8), charge);
            assert_eq!(rd32(&capacity, HEADER_LEN + 20), 1);
            if retire {
                execute(&mut v, &provider, &[closing], &[], true);
            } else {
                init_bucket(&mut v, &f, &bucket, 0, 900, false);
                execute(&mut v, &provider, &[closing], &[], false);
                assert_eq!(v.data(bucket.pubkey())[173], 0);
            }
            assert_eq!(v.data(f.capacity), capacity);
            execute(&mut v, &f.owner, &[dispute], &[], false);
            let before = rd64(&v.data(f.user), 72);
            let resolve = resolve_ix(&v, &f, case, charge);
            let admin = v.payer.insecure_clone();
            execute(&mut v, &admin, &[resolve.clone()], &[], true);
            assert_eq!(rd64(&v.data(f.user), 72), before + charge);
            assert_eq!(rd64(&v.data(f.capacity), HEADER_LEN + 8), 0);
            assert_eq!(rd32(&v.data(f.capacity), HEADER_LEN + 20), 0);
            execute(&mut v, &admin, &[resolve], &[], false);
            let case_close = ix(v.program, 17, &[], vec![w(case), w(f.owner.pubkey())]);
            execute(&mut v, &f.owner, &[case_close], &[], true);
            if !retire {
                book_one(&mut v, &f, &booking, true, false);
                claim_one(&mut v, &f, booking.pubkey(), true, bucket.pubkey());
                assert_eq!(rd64(&v.data(bucket.pubkey()), 200), 1);
            }
        }
    }
}

#[derive(Clone)]
struct Measurement {
    cu: u64,
    fee: u64,
    bytes: usize,
    signatures: usize,
    logs: Vec<String>,
    #[cfg(test)]
    instruction_data: Vec<Vec<u8>>,
}
impl Measurement {
    fn json(&self) -> Value {
        json!({"cu":self.cu,"fee_lamports":self.fee,"bytes":self.bytes,"signatures":self.signatures})
    }
}

fn ix(program: Address, op: u8, p: &[u8], accounts: Vec<AccountMeta>) -> Instruction {
    let mut data = vec![op];
    data.extend_from_slice(p);
    Instruction {
        program_id: program,
        accounts,
        data,
    }
}

fn execute(
    v: &mut Vm,
    payer: &Keypair,
    instructions: &[Instruction],
    signers: &[&Keypair],
    success: bool,
) -> Measurement {
    v.svm.expire_blockhash();
    let mut budget = vec![2];
    budget.extend_from_slice(&1_400_000u32.to_le_bytes());
    let mut all = vec![Instruction {
        program_id: address!("ComputeBudget111111111111111111111111111111"),
        accounts: vec![],
        data: budget,
    }];
    all.extend_from_slice(instructions);
    let before: Vec<_> = instructions
        .iter()
        .flat_map(|i| &i.accounts)
        .filter(|a| a.is_writable && a.pubkey != payer.pubkey())
        .map(|a| (a.pubkey, v.svm.get_account(&a.pubkey)))
        .collect();
    let mut keys = vec![payer];
    for signer in signers {
        if !keys.iter().any(|key| key.pubkey() == signer.pubkey()) {
            keys.push(*signer);
        }
    }
    let tx = Transaction::new(
        &keys,
        Message::new(&all, Some(&payer.pubkey())),
        v.svm.latest_blockhash(),
    );
    let signatures = tx.signatures.len();
    #[cfg(test)]
    let instruction_data = tx.message.instructions.iter().map(|instruction| instruction.data.clone()).collect();
    let bytes = 1 + signatures * 64 + tx.message.serialize().len();
    assert!(
        bytes <= 1232,
        "packet {bytes} exceeds legacy transaction limit"
    );
    let meta = match v.svm.send_transaction(tx) {
        Ok(meta) => {
            assert!(success, "unexpected acceptance");
            meta
        }
        Err(error) => {
            assert!(!success, "transaction failed: {error:?}");
            for (key, state) in before {
                assert_eq!(
                    v.svm.get_account(&key),
                    state,
                    "failed transaction changed {key}"
                );
            }
            error.meta
        }
    };
    Measurement {
        cu: meta.compute_units_consumed,
        fee: meta.fee,
        bytes,
        signatures,
        logs: meta.logs,
        #[cfg(test)]
        instruction_data,
    }
}

fn create(v: &Vm, payer: Address, account: Address, size: usize) -> Instruction {
    let mut data = 0u32.to_le_bytes().to_vec();
    data.extend_from_slice(&v.svm.minimum_balance_for_rent_exemption(size).to_le_bytes());
    data.extend_from_slice(&(size as u64).to_le_bytes());
    data.extend_from_slice(v.program.as_ref());
    Instruction {
        program_id: SYSTEM,
        accounts: vec![ws(payer), ws(account)],
        data,
    }
}

fn clock(v: &mut Vm, timestamp: i64) {
    let mut c = v.svm.get_sysvar::<Clock>();
    c.unix_timestamp = timestamp;
    v.svm.set_sysvar(&c);
}

fn generation(v: &Vm, f: &Fixture) -> u64 {
    let mut bytes = [0; 8];
    bytes[..7].copy_from_slice(&v.data(f.user)[113..120]);
    u64::from_le_bytes(bytes) + 1
}

fn book_ix(
    v: &Vm,
    f: &Fixture,
    booking: Address,
    prototype: bool,
    fresh: bool,
    nonce: u64,
) -> Instruction {
    let mut p = book_payload(&f.id, Address::default()).to_vec();
    if prototype {
        p.extend_from_slice(&nonce.to_le_bytes());
    }
    ix(
        v.program,
        6,
        &p,
        vec![
            r(f.market),
            r(f.model),
            r(f.terms),
            w(f.capacity),
            w(f.user),
            s(f.owner.pubkey()),
            a(booking, true, fresh),
            s(f.owner.pubkey()),
        ],
    )
}

fn book_one(
    v: &mut Vm,
    f: &Fixture,
    booking: &Keypair,
    prototype: bool,
    fresh: bool,
) -> Measurement {
    let instruction = book_ix(v, f, booking.pubkey(), prototype, fresh, generation(v, f));
    let mut instructions = Vec::new();
    if fresh {
        instructions.push(create(v, f.owner.pubkey(), booking.pubkey(), BOOKING_LEN));
    }
    instructions.push(instruction);
    let signers = [booking];
    execute(
        v,
        &f.owner,
        &instructions,
        if fresh { &signers } else { &[] },
        true,
    )
}

fn claim_ix(
    v: &Vm,
    f: &Fixture,
    booking: Address,
    prototype: bool,
    bucket: Address,
    nonce: u64,
) -> (Instruction, usize) {
    let bd = v.data(booking);
    let slot = rd32(&bd, 352) as usize;
    let offer = &f.offers[slot];
    let mut p = vec![0; 12];
    u32at(&mut p, 0, 500_000);
    u32at(&mut p, 8, 500_000);
    if prototype {
        p.extend_from_slice(&nonce.to_le_bytes());
        let hd = v.data(bucket);
        p.extend_from_slice(&hd[176..184]);
        p.extend_from_slice(&hd[200..208]);
        let charge: u64 = (0..3)
            .map(|i| (u64::from(rd32(&p, i * 4)) * u64::from(rd32(&bd, 112 + i * 4))).div_ceil(UNIT))
            .sum();
        p.extend_from_slice(&bd[8..40]);
        p.extend_from_slice(&bd[192..224]);
        p.extend_from_slice(booking.as_ref());
        p.extend_from_slice(&nonce.to_le_bytes());
        p.extend_from_slice(&charge.to_le_bytes());
        p.extend_from_slice(&bd[140..148]);
    }
    let mut accounts = vec![
        r(f.market),
        r(f.model),
        w(f.capacity),
        w(f.user),
        w(booking),
        s(offer.provider.pubkey()),
        w(f.vault),
        w(offer.payout),
        r(f.mint),
        r(f.authority),
        r(TOKEN),
    ];
    if prototype {
        accounts.push(w(bucket));
    }
    (ix(v.program, 7, &p, accounts), slot)
}

fn claim_one(
    v: &mut Vm,
    f: &Fixture,
    booking: Address,
    prototype: bool,
    bucket: Address,
) -> Measurement {
    let (instruction, slot) = claim_ix(
        v,
        f,
        booking,
        prototype,
        bucket,
        rd64(&v.data(booking), 152),
    );
    execute(v, &f.offers[slot].provider, &[instruction], &[], true)
}

fn init_bucket(
    v: &mut Vm,
    f: &Fixture,
    bucket: &Keypair,
    slot: usize,
    duration: u32,
    fresh: bool,
) -> Measurement {
    let mut p = (slot as u32).to_le_bytes().to_vec();
    p.extend_from_slice(&duration.to_le_bytes());
    let mut instructions = Vec::new();
    if fresh {
        instructions.push(create(v, f.owner.pubkey(), bucket.pubkey(), BUCKET_LEN));
    }
    instructions.push(ix(
        v.program,
        23,
        &p,
        vec![
            r(f.market),
            r(f.model),
            r(f.terms),
            a(bucket.pubkey(), true, fresh),
            s(f.owner.pubkey()),
        ],
    ));
    let signers = [bucket];
    execute(
        v,
        &f.owner,
        &instructions,
        if fresh { &signers } else { &[] },
        true,
    )
}

fn expire_ix(v: &Vm, f: &Fixture, bucket: Address) -> Instruction {
    ix(
        v.program,
        24,
        &[],
        vec![r(f.market), r(f.model), w(f.capacity), w(bucket)],
    )
}

fn cleanup(v: &mut Vm, f: &Fixture, bookings: &[Address]) -> Measurement {
    let instructions: Vec<_> = bookings
        .iter()
        .flat_map(|b| {
            [
                ix(
                    v.program,
                    9,
                    &[],
                    vec![r(f.market), r(f.model), w(f.capacity), w(*b)],
                ),
                ix(v.program, 17, &[], vec![w(*b), w(f.owner.pubkey())]),
            ]
        })
        .collect();
    execute(v, &f.owner, &instructions, &[], true)
}

fn hash(parts: &[&[u8]]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    for part in parts {
        hasher.update(part);
    }
    hasher.finalize().into()
}
fn leaf(program: Address, bucket: Address, epoch: i64, index: u64, receipt: &[u8]) -> [u8; 32] {
    hash(&[
        b"fossa/receipt/v1",
        program.as_ref(),
        bucket.as_ref(),
        &epoch.to_le_bytes(),
        &index.to_le_bytes(),
        receipt,
    ])
}
fn node(left: &[u8], right: &[u8]) -> [u8; 32] {
    hash(&[b"fossa/node/v1", left, right])
}

fn receipt_from_log(m: &Measurement, bucket: Address, epoch: i64, index: u64) -> Vec<u8> {
    let parts: Vec<Vec<u8>> = m
        .logs
        .iter()
        .filter_map(|log| log.strip_prefix("Program data: "))
        .map(|log| {
            log.split_whitespace()
                .map(|part| STANDARD.decode(part).unwrap())
                .collect::<Vec<_>>()
        })
        .find(|parts| {
            parts
                .first()
                .is_some_and(|part| part == b"fossa/receipt/v1")
        })
        .expect("canonical receipt missing from ledger logs");
    assert_eq!(parts[1], bucket.as_ref());
    assert_eq!(parts[2], epoch.to_le_bytes());
    assert_eq!(parts[3], index.to_le_bytes());
    assert_eq!(parts[4].len(), RECEIPT_LEN);
    parts[4].clone()
}

fn proof(leaves: &[[u8; 32]], index: usize) -> Vec<u8> {
    let mut start = 0;
    let mut width = 0;
    for level in (0..16).rev() {
        if leaves.len() & (1 << level) == 0 {
            continue;
        }
        width = 1 << level;
        if index < start + width {
            break;
        }
        start += width;
    }
    let mut nodes = leaves[start..start + width].to_vec();
    let mut at = index - start;
    let mut proof = Vec::new();
    while nodes.len() > 1 {
        proof.extend_from_slice(&nodes[at ^ 1]);
        nodes = nodes
            .chunks_exact(2)
            .map(|pair| node(&pair[0], &pair[1]))
            .collect();
        at /= 2;
    }
    proof
}

fn dispute_ix(
    v: &Vm,
    f: &Fixture,
    bucket: Address,
    epoch: i64,
    index: usize,
    receipt: &[u8],
    leaves: &[[u8; 32]],
) -> (Instruction, Address) {
    let mut p = epoch.to_le_bytes().to_vec();
    p.extend_from_slice(&(index as u64).to_le_bytes());
    p.extend_from_slice(receipt);
    p.extend(proof(leaves, index));
    let case = Address::find_program_address(
        &[
            b"receipt-case",
            bucket.as_ref(),
            &epoch.to_le_bytes(),
            &(index as u64).to_le_bytes(),
        ],
        &v.program,
    )
    .0;
    (
        ix(
            v.program,
            25,
            &p,
            vec![
                r(f.market),
                r(f.model),
                w(bucket),
                w(case),
                s(f.owner.pubkey()),
                ws(f.owner.pubkey()),
                r(SYSTEM),
            ],
        ),
        case,
    )
}

fn resolve_ix(v: &Vm, f: &Fixture, case: Address, refund: u64) -> Instruction {
    let slot = rd32(&v.data(case), 352) as usize;
    let offer = &f.offers[slot];
    ix(
        v.program,
        12,
        &refund.to_le_bytes(),
        vec![
            r(f.market),
            r(f.model),
            w(f.capacity),
            w(f.user),
            w(case),
            s(v.payer.pubkey()),
            w(offer.bond),
            w(f.vault),
            r(f.mint),
            r(offer.authority),
            r(TOKEN),
        ],
    )
}

fn stats(values: &[Measurement]) -> Value {
    let mut units: Vec<_> = values.iter().map(|v| v.cu).collect();
    units.sort_unstable();
    json!({"samples":values.len(),"cu_min":units[0],"cu_p50":units[units.len()/2],"cu_p95":units[(units.len()-1)*95/100],"cu_max":units[units.len()-1],"cu_mean":units.iter().sum::<u64>() as f64 / units.len() as f64,"fee_lamports":values[0].fee,"max_packet_bytes":values.iter().map(|v|v.bytes).max().unwrap(),"signatures":values[0].signatures})
}

fn profile(root: &Path, legacy: bool, count: usize, samples: usize, duration: u32) -> Value {
    let mut baseline = Vm::new(legacy);
    baseline
        .svm
        .add_program(
            baseline.program,
            &std::fs::read(baseline_path(root))
                .expect("build baseline with cargo build-sbf --no-default-features --sbf-out-dir target/receipt-bench/baseline -- --locked, or set FOSSA_RECEIPT_BASELINE"),
        )
        .unwrap();
    let f = setup(&mut baseline, count, 7);
    baseline
        .svm
        .airdrop(&f.owner.pubkey(), 100_000_000_000)
        .unwrap();
    for offer in &f.offers {
        baseline
            .svm
            .airdrop(&offer.provider.pubkey(), 1_000_000_000)
            .unwrap();
    }
    let timestamp = 1_800_000_000i64 / i64::from(duration) * i64::from(duration);
    clock(&mut baseline, timestamp);
    let mut prototype = Vm {
        svm: baseline.svm.clone(),
        program: baseline.program,
        payer: baseline.payer.insecure_clone(),
        last_error: None,
        executions: 0,
        succeeded: [0; 23],
        rejected: [0; 23],
        packet_max: [0; 23],
        claim_seconds: 0,
    };
    prototype
        .svm
        .add_program(
            prototype.program,
            &std::fs::read(prototype_path(root)).unwrap(),
        )
        .unwrap();
    let bucket = Keypair::new();
    let active = Keypair::new();
    let init = init_bucket(&mut prototype, &f, &bucket, count - 1, duration, true);
    let mut old_book = Vec::new();
    let mut old_claim = Vec::new();
    let mut old_accounts = Vec::new();
    let mut new_book = Vec::new();
    let mut new_claim = Vec::new();
    let mut leaves = Vec::new();
    let mut receipts = Vec::new();
    let baseline_lamports = baseline
        .svm
        .get_account(&f.owner.pubkey())
        .unwrap()
        .lamports;
    let mut cold_book = None;
    for i in 0..samples {
        let booking = Keypair::new();
        old_book.push(book_one(&mut baseline, &f, &booking, false, true));
        old_claim.push(claim_one(
            &mut baseline,
            &f,
            booking.pubkey(),
            false,
            Address::default(),
        ));
        old_accounts.push(booking.pubkey());
        let measured = book_one(&mut prototype, &f, &active, true, i == 0);
        if i == 0 {
            cold_book = Some(measured);
        } else {
            new_book.push(measured);
        }
        let claimed = claim_one(&mut prototype, &f, active.pubkey(), true, bucket.pubkey());
        let receipt = receipt_from_log(&claimed, bucket.pubkey(), timestamp, i as u64);
        leaves.push(leaf(
            prototype.program,
            bucket.pubkey(),
            timestamp,
            i as u64,
            &receipt,
        ));
        receipts.push(receipt);
        new_claim.push(claimed);
        assert_eq!(
            baseline.data(f.user)[72..80],
            prototype.data(f.user)[72..80]
        );
        assert_eq!(baseline.data(f.capacity), prototype.data(f.capacity));
        assert_eq!(
            baseline.data(f.offers[count - 1].payout),
            prototype.data(f.offers[count - 1].payout)
        );
        assert_eq!(prototype.data(active.pubkey())[148], 5);
    }
    // baseline rent returns completely; only transaction fees remain spent.
    let retention_seconds = rd64(&prototype.data(active.pubkey()), 140) - timestamp as u64;
    clock(&mut baseline, timestamp + retention_seconds as i64 + 1);
    let mut old_cleanup = Vec::new();
    for chunk in old_accounts.chunks(8) {
        old_cleanup.push(cleanup(&mut baseline, &f, chunk));
    }
    let owner_fees = old_book
        .iter()
        .chain(&old_cleanup)
        .map(|m| m.fee)
        .sum::<u64>();
    assert_eq!(
        baseline
            .svm
            .get_account(&f.owner.pubkey())
            .unwrap()
            .lamports,
        baseline_lamports - owner_fees
    );
    assert!(old_accounts.iter().all(|account| baseline
        .svm
        .get_account(account)
        .is_none_or(|account| account.lamports == 0)));

    let mut checks = vec![
        "identical matching, USDC charges and collateral",
        "ledger receipt reconstruction",
        "baseline rent fully recovered",
        "repeated slot reuse",
    ];
    let snapshot = prototype.data(active.pubkey());
    let stale_generation = rd64(&snapshot, 152);
    let next = book_ix(
        &prototype,
        &f,
        active.pubkey(),
        true,
        false,
        generation(&prototype, &f),
    );
    execute(&mut prototype, &f.owner, &[next.clone()], &[], true);
    execute(&mut prototype, &f.owner, &[next], &[], false);
    let (stale, slot) = claim_ix(
        &prototype,
        &f,
        active.pubkey(),
        true,
        bucket.pubkey(),
        stale_generation,
    );
    execute(
        &mut prototype,
        &f.offers[slot].provider,
        &[stale],
        &[],
        false,
    );
    let claimed = claim_one(&mut prototype, &f, active.pubkey(), true, bucket.pubkey());
    let receipt = receipt_from_log(&claimed, bucket.pubkey(), timestamp, samples as u64);
    leaves.push(leaf(
        prototype.program,
        bucket.pubkey(),
        timestamp,
        samples as u64,
        &receipt,
    ));
    receipts.push(receipt);
    checks.push("stale booking and provider claims rejected atomically");
    let (dispute, case) = dispute_ix(
        &prototype,
        &f,
        bucket.pubkey(),
        timestamp,
        0,
        &receipts[0],
        &leaves,
    );
    let mut forged = dispute.clone();
    forged.data[1 + 16 + 104] ^= 1;
    execute(&mut prototype, &f.owner, &[forged], &[], false);
    let mut forged = dispute.clone();
    let last = forged.data.len() - 1;
    forged.data[last] ^= 1;
    execute(&mut prototype, &f.owner, &[forged], &[], false);
    let mut forged = dispute.clone();
    forged.data[1] ^= 1;
    execute(&mut prototype, &f.owner, &[forged], &[], false);
    let admin = prototype.payer.insecure_clone();
    let mut forged = dispute.clone();
    forged.accounts[4] = s(admin.pubkey());
    forged.accounts[5] = ws(admin.pubkey());
    execute(&mut prototype, &admin, &[forged], &[], false);
    let expire = expire_ix(&prototype, &f, bucket.pubkey());
    execute(&mut prototype, &f.owner, &[expire.clone()], &[], false);
    let dispute_cu = execute(&mut prototype, &f.owner, &[dispute.clone()], &[], true);
    execute(&mut prototype, &f.owner, &[dispute.clone()], &[], false);
    let before = rd64(&prototype.data(f.user), 72);
    let refund = rd64(&prototype.data(case), 160);
    let resolve = resolve_ix(&prototype, &f, case, refund);
    let resolve_cu = execute(&mut prototype, &admin, &[resolve.clone()], &[], true);
    assert_eq!(rd64(&prototype.data(f.user), 72), before + refund);
    execute(&mut prototype, &admin, &[resolve], &[], false);
    let close = ix(
        prototype.program,
        17,
        &[],
        vec![w(case), w(f.owner.pubkey())],
    );
    execute(&mut prototype, &f.owner, &[close.clone()], &[], false);
    execute(&mut prototype, &f.owner, &[dispute.clone()], &[], false);
    checks.push("forged amount, proof, epoch and unauthorized owner rejected");
    checks.push("dispute survives active-slot reuse; refund exactly once");
    checks.push("early expiry and early dispute-marker closure rejected");
    let (second, case2) = dispute_ix(
        &prototype,
        &f,
        bucket.pubkey(),
        timestamp,
        1,
        &receipts[1],
        &leaves,
    );
    execute(&mut prototype, &f.owner, &[second], &[], true);
    for index in [samples / 2, samples - 1, samples] {
        let (challenge, case) = dispute_ix(
            &prototype,
            &f,
            bucket.pubkey(),
            timestamp,
            index,
            &receipts[index],
            &leaves,
        );
        execute(&mut prototype, &f.owner, &[challenge], &[], true);
        let resolve = resolve_ix(&prototype, &f, case, 0);
        execute(&mut prototype, &admin, &[resolve], &[], true);
    }
    checks.push("independently reconstructed proofs across MMR mountains accepted");
    clock(&mut prototype, timestamp + retention_seconds as i64 + 1);
    let expire_cu = execute(&mut prototype, &f.owner, &[expire.clone()], &[], true);
    execute(&mut prototype, &f.owner, &[expire], &[], false);
    let capacity = prototype.data(f.capacity);
    let at = HEADER_LEN + (count - 1) * CAPACITY_LEN;
    assert_eq!(rd64(&capacity, at + 8), rd64(&prototype.data(case2), 160));
    assert_eq!(rd32(&capacity, at + 20), 1);
    let recycle_cu = init_bucket(&mut prototype, &f, &bucket, count - 1, duration, false);
    execute(&mut prototype, &f.owner, &[dispute.clone()], &[], false);
    let resolve = resolve_ix(&prototype, &f, case2, rd64(&prototype.data(case2), 160));
    execute(&mut prototype, &admin, &[resolve], &[], true);
    assert_eq!(rd64(&prototype.data(f.capacity), at + 8), 0);
    assert_eq!(rd32(&prototype.data(f.capacity), at + 20), 0);
    execute(&mut prototype, &f.owner, &[close], &[], true);
    execute(&mut prototype, &f.owner, &[dispute], &[], false);
    checks.push("pending dispute collateral survives bucket expiry and reuse");
    checks.push("late proof and repeated refund rejected after marker closure");
    // another bucket can be added for the same user/provider; no fixed per-user history quota.
    let extra = Keypair::new();
    init_bucket(&mut prototype, &f, &extra, count - 1, duration, true);
    book_one(&mut prototype, &f, &active, true, false);
    claim_one(&mut prototype, &f, active.pubkey(), true, extra.pubkey());
    checks.push("additional independently funded history bucket accepted");
    let (duplicate, slot) = claim_ix(
        &prototype,
        &f,
        active.pubkey(),
        true,
        extra.pubkey(),
        rd64(&prototype.data(active.pubkey()), 152),
    );
    execute(
        &mut prototype,
        &f.offers[slot].provider,
        &[duplicate],
        &[],
        false,
    );
    // a zero-charge claim releases the slot without adding receipt liabilities.
    let free_before = rd64(&prototype.data(f.user), 72);
    let capacity_before = prototype.data(f.capacity);
    let bucket_before = prototype.data(extra.pubkey());
    book_one(&mut prototype, &f, &active, true, false);
    let nonce = rd64(&prototype.data(active.pubkey()), 152);
    let (mut zero, slot) = claim_ix(&prototype, &f, active.pubkey(), true, extra.pubkey(), nonce);
    zero.data[1..13].fill(0);
    zero.data[CLAIM_RECEIPT_OFFSET + 104..CLAIM_RECEIPT_OFFSET + 112].fill(0);
    execute(&mut prototype, &f.offers[slot].provider, &[zero], &[], true);
    assert_eq!(rd64(&prototype.data(f.user), 72), free_before);
    assert_eq!(prototype.data(f.capacity), capacity_before);
    assert_eq!(prototype.data(extra.pubkey()), bucket_before);
    // release/expiry messages cannot affect a later booking in the same slot.
    book_one(&mut prototype, &f, &active, true, false);
    let nonce = rd64(&prototype.data(active.pubkey()), 152);
    let release = ix(
        prototype.program,
        15,
        &nonce.to_le_bytes(),
        vec![
            r(f.market),
            r(f.model),
            w(f.capacity),
            w(f.user),
            w(active.pubkey()),
            s(f.offers[slot].provider.pubkey()),
        ],
    );
    execute(
        &mut prototype,
        &f.offers[slot].provider,
        &[release.clone()],
        &[],
        true,
    );
    book_one(&mut prototype, &f, &active, true, false);
    execute(
        &mut prototype,
        &f.offers[slot].provider,
        &[release],
        &[],
        false,
    );
    let mut expiry = ix(
        prototype.program,
        8,
        &nonce.to_le_bytes(),
        vec![
            r(f.market),
            r(f.model),
            w(f.capacity),
            w(f.user),
            w(active.pubkey()),
        ],
    );
    let claim_end = rd64(&prototype.data(active.pubkey()), 132) as i64;
    clock(&mut prototype, claim_end + 1);
    execute(&mut prototype, &f.owner, &[expiry.clone()], &[], false);
    expiry.data[1..9].copy_from_slice(&rd64(&prototype.data(active.pubkey()), 152).to_le_bytes());
    execute(&mut prototype, &f.owner, &[expiry], &[], true);
    assert_eq!(rd64(&prototype.data(f.user), 72), free_before);
    assert_eq!(prototype.data(f.capacity), capacity_before);
    checks.push(
        "zero-charge, provider release and timeout return reserves; stale release/expiry rejected",
    );
    // closing a reusable account needs its rent payer; recreation preserves the lane nonce.
    let close = ix(
        prototype.program,
        17,
        &[],
        vec![w(active.pubkey()), w(f.owner.pubkey())],
    );
    execute(&mut prototype, &admin, &[close.clone()], &[], false);
    let old_nonce = rd64(&prototype.data(active.pubkey()), 152);
    let before_close = prototype
        .svm
        .get_account(&f.owner.pubkey())
        .unwrap()
        .lamports;
    let locked = prototype
        .svm
        .get_account(&active.pubkey())
        .unwrap()
        .lamports;
    let closed = execute(&mut prototype, &f.owner, &[close], &[], true);
    assert_eq!(
        prototype
            .svm
            .get_account(&f.owner.pubkey())
            .unwrap()
            .lamports,
        before_close + locked - closed.fee
    );
    let extra = Keypair::new();
    init_bucket(&mut prototype, &f, &extra, count - 1, duration, true);
    book_one(&mut prototype, &f, &active, true, true);
    let (stale, slot) = claim_ix(
        &prototype,
        &f,
        active.pubkey(),
        true,
        extra.pubkey(),
        old_nonce,
    );
    execute(
        &mut prototype,
        &f.offers[slot].provider,
        &[stale],
        &[],
        false,
    );
    claim_one(&mut prototype, &f, active.pubkey(), true, extra.pubkey());
    checks.push(
        "idle rent recovered; unauthorized closure and replay after account recreation rejected",
    );
    // an epoch boundary needs another bucket; the booking survives a failed stale-bucket claim.
    book_one(&mut prototype, &f, &active, true, false);
    let (stale_bucket, slot) = claim_ix(
        &prototype,
        &f,
        active.pubkey(),
        true,
        extra.pubkey(),
        rd64(&prototype.data(active.pubkey()), 152),
    );
    let epoch_end = rd64(&prototype.data(extra.pubkey()), 184) as i64;
    clock(&mut prototype, epoch_end);
    execute(
        &mut prototype,
        &f.offers[slot].provider,
        &[stale_bucket],
        &[],
        false,
    );
    let next_bucket = Keypair::new();
    init_bucket(&mut prototype, &f, &next_bucket, count - 1, duration, true);
    claim_one(
        &mut prototype,
        &f,
        active.pubkey(),
        true,
        next_bucket.pubkey(),
    );
    checks.push(
        "claim across epoch rollover succeeds with new bucket; stale-bucket claim rolls back",
    );
    // legacy instructions remain valid after upgrading, without resetting lane generations.
    let nonce_before=generation(&prototype,&f);
    let old=Keypair::new();
    book_one(&mut prototype,&f,&old,false,true);
    claim_one(&mut prototype,&f,old.pubkey(),false,next_bucket.pubkey());
    assert_eq!(generation(&prototype,&f),nonce_before);
    checks.push("legacy booking and claim remain compatible; generation counter preserved");
    // sponsor owns rent; owner alone signs each subsequent booking.
    let sponsor=Keypair::new(); prototype.svm.airdrop(&sponsor.pubkey(),1_000_000_000).unwrap();
    let sponsored=Keypair::new();
    let init_pool=ix(prototype.program,26,&[],vec![r(f.market),r(f.user),s(f.owner.pubkey()),ws(sponsored.pubkey()),s(sponsor.pubkey())]);
    let allocation=create(&prototype,sponsor.pubkey(),sponsored.pubkey(),BOOKING_LEN);
    execute(&mut prototype,&sponsor,&[allocation,init_pool],&[&sponsored,&f.owner],true);
    assert_eq!(prototype.data(sponsored.pubkey())[148],5);
    let mut instruction=book_ix(&prototype,&f,sponsored.pubkey(),true,false,generation(&prototype,&f));
    instruction.accounts[7]=r(sponsor.pubkey());
    let signed=execute(&mut prototype,&f.owner,&[instruction],&[],true);
    assert_eq!(signed.signatures,1);
    claim_one(&mut prototype,&f,sponsored.pubkey(),true,next_bucket.pubkey());
    assert_eq!(&prototype.data(sponsored.pubkey())[256..288],sponsor.pubkey().as_ref());
    checks.push("sponsored rent allocation supports one-signature user bookings and reuse");
    let future=Keypair::new();
    let now=prototype.svm.get_sysvar::<Clock>().unix_timestamp;
    let target=(now/i64::from(duration)+1)*i64::from(duration);
    let payload=[((count-1) as u32).to_le_bytes().as_slice(),&duration.to_le_bytes(),&target.to_le_bytes()].concat();
    let allocation=create(&prototype,f.owner.pubkey(),future.pubkey(),BUCKET_LEN);
    let initialize=ix(prototype.program,23,&payload,vec![r(f.market),r(f.model),r(f.terms),ws(future.pubkey()),s(f.owner.pubkey())]);
    execute(&mut prototype,&f.owner,&[allocation,initialize],&[&future],true);
    assert_eq!(rd64(&prototype.data(future.pubkey()),176),target as u64);
    checks.push("next epoch history is funded before rollover");
    json!({"token_program":if legacy{"legacy-spl-3.5"}else{"p-token"},"provider_count":count,"samples":samples,"retention_seconds":retention_seconds,
        "baseline":{"book_with_account_creation":stats(&old_book),"claim":stats(&old_claim),"finalize_close_batch8":stats(&old_cleanup)},
        "prototype":{"book_reused":stats(&new_book),"claim_with_receipt_hashing_and_log":stats(&new_claim),"cold_book":cold_book.unwrap().json(),"cold_bucket":init.json(),"expire_bucket":expire_cu.json(),"reuse_bucket":recycle_cu.json(),"open_dispute":dispute_cu.json(),"resolve_dispute":resolve_cu.json()},"checks":checks})
}

pub fn run() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let raw = std::env::var_os("FOSSA_BENCH_CONFIG")
        .map(|path| std::fs::read_to_string(path).expect("benchmark config unavailable"))
        .unwrap_or_else(|| include_str!("../config.example.toml").to_owned());
    let bench = settings(&raw).expect("invalid receipt benchmark settings");
    let duration = bench.epoch_seconds;
    let counts = &bench.offer_counts;
    let samples = bench.samples;
    let scale_samples = bench.scale_samples;
    let mut profiles = Vec::new();
    for legacy in [false, true] {
        for &count in counts {
            eprintln!(
                "receipt benchmark: token={} offers={count}",
                if legacy { "legacy" } else { "p-token" }
            );
            profiles.push(profile(
                &root,
                legacy,
                count,
                if count == 1 { samples } else { scale_samples },
                duration,
            ));
        }
    }
    let svm = LiteSVM::new();
    let booking_rent = svm.minimum_balance_for_rent_exemption(368);
    let bucket_rent = svm.minimum_balance_for_rent_exemption(736);
    let rents = json!({"source":"local LiteSVM rent sysvar; no live network quote","lamports":{"368":booking_rent,"736":bucket_rent}});
    let requests = bench.requests_per_day;
    let sol_usd = bench.illustrative_sol_usd;
    let mut economics = Vec::new();
    for profile in &profiles {
        let old = &profile["baseline"];
        let new = &profile["prototype"];
        let maintenance = 86400u64.div_ceil(u64::from(duration));
        let old_fees = requests
            * (old["book_with_account_creation"]["fee_lamports"]
                .as_u64()
                .unwrap()
                + old["claim"]["fee_lamports"].as_u64().unwrap())
            + requests.div_ceil(8)
                * old["finalize_close_batch8"]["fee_lamports"]
                    .as_u64()
                    .unwrap();
        let new_fees = requests
            * (new["book_reused"]["fee_lamports"].as_u64().unwrap()
                + new["claim_with_receipt_hashing_and_log"]["fee_lamports"]
                    .as_u64()
                    .unwrap())
            + maintenance
                * (new["expire_bucket"]["fee_lamports"].as_u64().unwrap()
                    + new["reuse_bucket"]["fee_lamports"].as_u64().unwrap());
        let old_cu = old["book_with_account_creation"]["cu_mean"]
            .as_f64()
            .unwrap()
            + old["claim"]["cu_mean"].as_f64().unwrap()
            + old["finalize_close_batch8"]["cu_mean"].as_f64().unwrap() / 8.0;
        let new_cu = new["book_reused"]["cu_mean"].as_f64().unwrap()
            + new["claim_with_receipt_hashing_and_log"]["cu_mean"]
                .as_f64()
                .unwrap()
            + maintenance as f64
                * (new["expire_bucket"]["cu"].as_u64().unwrap()
                    + new["reuse_bucket"]["cu"].as_u64().unwrap()) as f64
                / requests as f64;
        economics.push(json!({"token_program":profile["token_program"],"provider_count":profile["provider_count"],"scope":format!("{requests} requests uniformly over 24 hours; one selected provider; owner fee payer; baseline cleanup batched 8; prototype maintenance unbatched; cold setup and disputes excluded"),
            "baseline_fee_sol_per_day":old_fees as f64/1e9,"prototype_fee_sol_per_day":new_fees as f64/1e9,"fee_reduction_percent":100.0*(1.0-new_fees as f64/old_fees as f64),
            "baseline_cu_per_request_including_maintenance":old_cu,"prototype_cu_per_request_including_maintenance":new_cu,"cu_change_percent":100.0*(new_cu/old_cu-1.0)}));
    }
    let mut scenarios = Vec::new();
    let retention_seconds = profiles.iter().map(|profile| profile["retention_seconds"].as_u64().unwrap()).max().unwrap();
    for hours in [24u64, 1] {
        let rate = requests as f64 / (hours * 3600) as f64;
        for &delay in &bench.settlement_seconds {
            let old_records = ((rate * retention_seconds as f64).ceil() as u64).min(requests);
            let active_records = ((rate * delay as f64).ceil().max(1.0) as u64).min(requests);
            let buckets = retention_seconds.div_ceil(u64::from(duration)) + 1;
            let old = old_records * booking_rent;
            let new = active_records * booking_rent + buckets * bucket_rent;
            scenarios.push(json!({"requests":requests,"arrival_hours":hours,"settlement_seconds":delay,"baseline_outstanding_records":old_records,"prototype_active_records":active_records,"history_buckets_one_provider":buckets,"baseline_locked_sol":old as f64/1e9,"prototype_locked_sol":new as f64/1e9,"locked_sol_reduction_percent":100.0*(1.0-new as f64/old as f64),"baseline_locked_usd_at_example_price":old as f64/1e9*sol_usd,"prototype_locked_usd_at_example_price":new as f64/1e9*sol_usd}));
        }
    }
    let output = json!({"scope":"local LiteSVM real SBF execution; no mainnet transactions or TPS measurement","program_hashes":{"baseline":format!("{:x}",Sha256::digest(std::fs::read(baseline_path(&root)).unwrap())),"prototype":format!("{:x}",Sha256::digest(std::fs::read(prototype_path(&root)).unwrap()))},"rent_quote":rents,"rent_is_spent":false,"storage":{"active_record_bytes":368,"history_bucket_bytes":BUCKET_LEN,"receipt_bytes_published":RECEIPT_LEN,"max_receipts_per_bucket":65535,"bucket_seconds":duration},"illustrative_sol_usd":sol_usd,"profiles":profiles,"scenarios":scenarios,"economics":economics,"limitations":["matching and balance write locks unchanged; not a network-scale throughput implementation","history buckets are per provider/model; multiple providers increase storage","active records need funding and can grow; benchmark excludes backend sponsor allocation","independent receipt reconstruction tested from runtime logs; this benchmark does not exercise backend archival","scenarios assume uniform arrivals, successful settlement, prompt cleanup and no persistent disputes; setup balances shared by both designs excluded","no priority fee configured; full transaction base fees measured separately from refundable rent","MMR proofs may need refreshing while a bucket is appending; epochs close before the receipt dispute deadline"]});
    let dest = std::env::var_os("FOSSA_RECEIPT_OUTPUT").map(PathBuf::from)
        .unwrap_or_else(|| root.join("target/receipt-bench/results.json"));
    if let Some(parent) = dest.parent() { std::fs::create_dir_all(parent).unwrap(); }
    std::fs::write(&dest, serde_json::to_vec_pretty(&output).unwrap()).unwrap();
    println!("{}", serde_json::to_string_pretty(&output).unwrap());
}
