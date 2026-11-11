//! Runs the compiled binary: arguments, exit status, and stream separation.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn engine(args: &[&std::ffi::OsStr]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_payments-engine"))
        .args(args)
        .output()
        .expect("binary runs")
}

fn run_fixture(name: &str) -> Output {
    engine(&[fixture(name).as_os_str()])
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8(bytes.to_vec()).unwrap()
}

#[test]
fn prints_the_report_on_stdout_and_nothing_on_stderr() {
    let out = run_fixture("basic.csv");
    assert_eq!(out.status.code(), Some(0));
    assert_eq!(
        text(&out.stdout),
        fs::read_to_string(fixture("basic.expected.csv")).unwrap()
    );
    assert_eq!(text(&out.stderr), "");
}

#[test]
fn a_fatal_record_exits_1_with_empty_stdout_and_names_the_line() {
    let out = run_fixture("fatal_negative_amount.csv");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stdout), "");
    let err = text(&out.stderr);
    assert!(err.starts_with("error: line 3: invalid amount"), "{err}");
}

#[test]
fn overflow_exits_1_with_empty_stdout_and_names_the_line() {
    let out = run_fixture("fatal_overflow.csv");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stdout), "");
    assert!(
        text(&out.stderr).starts_with("error: line 3: amount exceeds the supported range"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn an_invalid_client_id_names_the_line_and_the_column() {
    let path = Path::new(env!("CARGO_TARGET_TMPDIR")).join("invalid_client.csv");
    fs::write(
        &path,
        "type,client,tx,amount\ndeposit,1,1,1\ndeposit,70000,2,1\n",
    )
    .unwrap();
    let out = engine(&[path.as_os_str()]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stdout), "");
    assert!(
        text(&out.stderr).starts_with("error: line 3: invalid client"),
        "{}",
        text(&out.stderr)
    );
}

#[test]
fn a_wrong_header_exits_1_with_empty_stdout() {
    let out = run_fixture("wrong_header.csv");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stdout), "");
    assert!(text(&out.stderr).contains("unexpected header"));
}

#[test]
fn empty_and_header_only_files_print_only_the_header() {
    for name in ["empty.csv", "header_only.csv"] {
        let out = run_fixture(name);
        assert_eq!(out.status.code(), Some(0), "{name}");
        assert_eq!(
            text(&out.stdout),
            "client,available,held,total,locked\n",
            "{name}"
        );
    }
}

#[test]
fn a_missing_file_exits_1_with_empty_stdout() {
    let out = run_fixture("does_not_exist.csv");
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stdout), "");
    assert!(text(&out.stderr).starts_with("error: cannot open"));
}

#[test]
fn wrong_argument_counts_exit_2_with_usage() {
    let path = fixture("basic.csv");
    for args in [vec![], vec![path.as_os_str(), path.as_os_str()]] {
        let out = engine(&args);
        assert_eq!(out.status.code(), Some(2));
        assert_eq!(text(&out.stdout), "");
        assert!(text(&out.stderr).contains("usage:"));
    }
}

#[test]
fn a_directory_path_exits_1() {
    let out = engine(&[fixture("").as_os_str()]);
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(text(&out.stdout), "");
}

#[cfg(target_os = "linux")]
#[test]
fn a_full_output_device_exits_1_without_panicking() {
    let out = Command::new(env!("CARGO_BIN_EXE_payments-engine"))
        .arg(fixture("basic.csv"))
        .stdout(
            fs::OpenOptions::new()
                .write(true)
                .open("/dev/full")
                .unwrap(),
        )
        .stderr(Stdio::piped())
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = text(&out.stderr);
    assert!(err.starts_with("error: cannot write output"), "{err}");
    assert!(!err.contains("panicked"), "{err}");
}

#[cfg(target_os = "linux")]
#[test]
fn an_unwritable_stderr_does_not_cause_a_panic() {
    let out = Command::new(env!("CARGO_BIN_EXE_payments-engine"))
        .arg(fixture("does_not_exist.csv"))
        .stderr(
            fs::OpenOptions::new()
                .write(true)
                .open("/dev/full")
                .unwrap(),
        )
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
}
