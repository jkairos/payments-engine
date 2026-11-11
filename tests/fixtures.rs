//! Every `tests/fixtures/<name>.csv` with a `<name>.expected.csv` beside it is run
//! through the library and compared byte for byte. Expected files were calculated by hand.

use std::fs;
use std::path::Path;

fn check(name: &str) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let input = fs::read(dir.join(format!("{name}.csv"))).unwrap();
    let expected = fs::read_to_string(dir.join(format!("{name}.expected.csv"))).unwrap();
    let mut output = Vec::new();
    payments_engine::run(input.as_slice(), &mut output).unwrap();
    assert_eq!(
        String::from_utf8(output).unwrap(),
        expected,
        "fixture {name}"
    );
}

#[test]
fn basic() {
    check("basic");
}

#[test]
fn dispute_resolve() {
    check("dispute_resolve");
}

#[test]
fn chargeback_spent_deposit() {
    check("chargeback_spent_deposit");
}

#[test]
fn cross_client_and_repeats() {
    check("cross_client_and_repeats");
}

#[test]
fn precision_whitespace() {
    check("precision_whitespace");
}

#[test]
fn zero_amounts() {
    check("zero_amounts");
}

#[test]
fn empty() {
    check("empty");
}

#[test]
fn header_only() {
    check("header_only");
}

#[test]
fn every_expected_file_has_a_test() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut names: Vec<String> = fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.unwrap().file_name().into_string().ok())
        .filter_map(|f| f.strip_suffix(".expected.csv").map(str::to_owned))
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "basic",
            "chargeback_spent_deposit",
            "cross_client_and_repeats",
            "dispute_resolve",
            "empty",
            "header_only",
            "precision_whitespace",
            "zero_amounts"
        ]
    );
}
