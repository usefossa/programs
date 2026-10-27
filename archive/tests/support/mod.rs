use litesvm::LiteSVM;
use solana_address::Address;
use std::{path::PathBuf, sync::OnceLock};

pub fn artifact(variable: &str, default: &str) -> PathBuf {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    root.join(
        std::env::var_os(variable)
            .map(PathBuf::from)
            .unwrap_or_else(|| default.into()),
    )
}

pub fn svm(legacy: bool) -> LiteSVM {
    let mut features = LiteSVM::mainnet_feature_set();
    if legacy {
        features.deactivate(&agave_feature_set::replace_spl_token_with_p_token::id());
    }
    let mut svm = LiteSVM::default()
        .with_feature_set(features)
        .with_builtins()
        .with_lamports(1_000_000_000_000_000)
        .with_sysvars()
        .with_feature_accounts()
        .with_default_programs()
        .with_sigverify(true)
        .with_blockhash_check(true);
    if legacy && std::env::var_os("LEGACY_TOKEN_SBF").is_some() {
        svm.add_program_from_file(
            solana_address::address!("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA"),
            artifact("LEGACY_TOKEN_SBF", ""),
        )
        .expect("could not load LEGACY_TOKEN_SBF override");
    }
    svm
}

pub fn load_program(svm: &mut LiteSVM, program: Address) {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    let bytes = BYTES.get_or_init(|| {
        let path = artifact("FOSSA_SBF", "target/deploy/inference_router_program.so");
        std::fs::read(&path).unwrap_or_else(|error| {
            panic!(
                "could not read {}: {error}; run cargo build-sbf -- --locked or set FOSSA_SBF",
                path.display()
            )
        })
    });
    svm.add_program(program, bytes)
        .expect("could not load program SBF artifact");
}
