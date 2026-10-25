#[path = "../../support/scenarios.rs"]
mod scenarios;
fn main() {
    let legacy = std::env::var_os("FOSSA_LEGACY_TOKEN").is_some();
    scenarios::run(legacy);
    scenarios::growth();
    if std::env::var_os("FOSSA_STRESS").is_some() {
        scenarios::scale();
    }
    if std::env::var_os("FOSSA_MIGRATION_SBF").is_some() {
        scenarios::migration(legacy);
    }
}
