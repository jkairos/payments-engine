//! Process boundary: one argument, the input file; the report on stdout; errors on stderr.

use std::env;
use std::fs::File;
use std::io::{self, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    // `args_os` avoids a panic on paths that are not valid Unicode.
    let args: Vec<_> = env::args_os().skip(1).collect();
    let [path] = args.as_slice() else {
        report("usage: payments-engine <transactions.csv>");
        return ExitCode::from(2);
    };

    let file = match File::open(path) {
        Ok(file) => file,
        Err(e) => {
            report(&format!(
                "error: cannot open {}: {e}",
                path.to_string_lossy()
            ));
            return ExitCode::FAILURE;
        }
    };

    match payments_engine::run(file, io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            report(&format!("error: {e}"));
            ExitCode::FAILURE
        }
    }
}

/// Writes one diagnostic line to stderr without panicking. If stderr itself is
/// unwritable there is nowhere left to report to; the exit status still signals
/// the failure.
fn report(message: &str) {
    let _ = writeln!(io::stderr(), "{message}");
}
