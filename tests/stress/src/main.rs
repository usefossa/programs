#[path = "../../support/scenarios.rs"]
mod scenarios;
fn main() {
    scenarios::scale();
    scenarios::growth();
    scenarios::run(false);
    scenarios::run(true);
    scenarios::migration(false);
    scenarios::migration(true);
}
#[test]
fn protocol_and_receipts() {
    scenarios::run(false);
}
#[test]
fn protocol_legacy_token() {
    scenarios::run(true);
}
#[test]
fn legacy_rent_recovery() {
    scenarios::migration(false);
}
#[test]
fn legacy_rent_recovery_legacy_token() {
    scenarios::migration(true);
}

#[test]
fn maximum_list_and_descending_book() {
    scenarios::scale();
}

#[test]
fn permissionless_book_growth() {
    scenarios::growth();
}
