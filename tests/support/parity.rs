use litesvm::{types::TransactionResult, LiteSVM};
use solana_address::{address, Address};
use solana_transaction::versioned::VersionedTransaction;
use solana_transaction::Transaction;
use std::{cell::RefCell, io::Write};

thread_local! {
    static BENCH_FIXTURE: RefCell<(String, usize)> = RefCell::new((String::new(), 0));
}

#[allow(dead_code)]
pub fn fixture(label: impl Into<String>) {
    BENCH_FIXTURE.with(|state| *state.borrow_mut() = (label.into(), 0));
}

const LOADER: Address = address!("BPFLoaderUpgradeab1e11111111111111111111111");
const ROUTER: Address = Address::new_from_array([8; 32]);
const DEX: Address = Address::new_from_array([9; 32]);

pub struct Shadow {
    svm: LiteSVM,
    result: TransactionResult,
    keys: Vec<Address>,
    bytecode: Vec<Address>,
    instructions: String,
    limit: u32,
    migration: bool,
}

pub fn run(svm: &LiteSVM, tx: &Transaction, migration: bool) -> Option<Shadow> {
    run_versioned(
        svm,
        &tx.clone().into(),
        tx.message.account_keys.clone(),
        migration,
    )
}

pub fn run_versioned(
    svm: &LiteSVM,
    tx: &VersionedTransaction,
    keys: Vec<Address>,
    migration: bool,
) -> Option<Shadow> {
    let root = std::path::PathBuf::from(std::env::var_os("FOSSA_PARITY_ORIGINAL")?);
    let mut original = svm.clone();
    let mut bytecode = Vec::new();
    for (program, artifact) in [
        (
            ROUTER,
            if migration {
                "target/migration/inference_router_program.so"
            } else {
                "target/deploy/inference_router_program.so"
            },
        ),
        (DEX, "dex/target/deploy/fossa_dex.so"),
    ] {
        if svm.get_account(&program).is_none() {
            continue;
        }
        let pd = Address::find_program_address(&[program.as_ref()], &LOADER).0;
        let header = svm.get_account(&pd).unwrap().data[..45].to_vec();
        original
            .add_program_from_file(program, root.join(artifact))
            .unwrap();
        let mut account = original.get_account(&pd).unwrap();
        account.data[..45].copy_from_slice(&header);
        original.set_account(pd, account).unwrap();
        bytecode.push(pd);
    }
    let result = original.send_transaction(tx.clone());
    let mut limit = 200_000;
    let instructions = tx.message.instructions().iter().map(|ix| {
        let program = tx.message.static_account_keys().get(ix.program_id_index as usize);
        let name = match program {
            Some(p) if *p == ROUTER => "router".into(),
            Some(p) if *p == DEX => "dex".into(),
            Some(p) => p.to_string(),
            None => format!("lookup_program_{}", ix.program_id_index),
        };
        if name == "ComputeBudget111111111111111111111111111111" && ix.data.first() == Some(&2) {
            limit = u32::from_le_bytes(ix.data[1..5].try_into().unwrap());
        }
        format!("{name}:{}", ix.data.first().map_or(255, |v| *v))
    }).collect::<Vec<_>>().join("|");
    Some(Shadow {
        svm: original,
        result,
        keys,
        bytecode,
        instructions,
        limit,
        migration,
    })
}

impl Shadow {
    #[track_caller]
    pub fn check(self, svm: &LiteSVM, result: &TransactionResult, label: &str) {
        assert_eq!(
            self.result.is_ok(),
            result.is_ok(),
            "{label}: accept/reject parity; original={:?}; anchor={result:?}",
            self.result
        );
        for key in &self.keys {
            if self.bytecode.contains(key) {
                continue;
            }
            assert_eq!(
                self.svm.get_account(key),
                svm.get_account(key),
                "{label}: account state parity for {key}"
            );
        }
        if let (Ok(original), Ok(anchor)) = (&self.result, result) {
            let data_logs = |logs: &[String]| {
                logs.iter()
                    .filter(|log| log.starts_with("Program data: "))
                    .cloned()
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                data_logs(&original.logs),
                data_logs(&anchor.logs),
                "{label}: receipt/data log parity"
            );
            assert_eq!(
                original.return_data, anchor.return_data,
                "{label}: return data parity"
            );
        }
        if let Some(path) = std::env::var_os("FOSSA_BENCH_CSV") {
            let meta = |r: &TransactionResult| match r {
                Ok(m) => m.compute_units_consumed,
                Err(e) => e.meta.compute_units_consumed,
            };
            let escape = |s: &str| format!("\"{}\"", s.replace('"', "\"\""));
            let (fixture, ordinal) = BENCH_FIXTURE.with(|state| {
                let mut state = state.borrow_mut();
                state.1 += 1;
                (state.0.clone(), state.1)
            });
            let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path).unwrap();
            if file.metadata().unwrap().len() == 0 {
                writeln!(file, "run,fixture,ordinal,label,source,instructions,migration,success,limit_cu,original_cu,anchor_cu").unwrap();
            }
            writeln!(file, "{},{},{ordinal},{},{},{},{},{},{},{},{}", escape(&std::env::var("FOSSA_BENCH_RUN").unwrap_or_default()), escape(&fixture), escape(label),
                escape(&std::panic::Location::caller().to_string()), escape(&self.instructions), self.migration,
                result.is_ok(), self.limit, meta(&self.result), meta(result))
                .and_then(|_| file.flush()).unwrap();
        }
    }
}
