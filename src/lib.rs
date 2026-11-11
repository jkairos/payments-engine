//! A small payments engine: reads transactions as CSV, prints account balances as CSV.
//!
//! [`run`] is the whole program apart from argument handling: it streams the
//! input through the [`engine::Engine`] and writes the report only after the
//! entire input was processed successfully.
#![forbid(unsafe_code)]

pub mod csv_io;
pub mod engine;
pub mod money;

use std::fmt;
use std::io;

pub use csv_io::RecordError;

/// A failure that stops the run. No report is written for any variant except
/// [`Error::Write`], where part of the report may already have been written.
#[derive(Debug)]
pub enum Error {
    /// Reading the input failed: an I/O error or invalid UTF-8.
    Read(csv::Error),
    /// The first row is not `type,client,tx,amount`.
    Header(String),
    /// A record could not be interpreted.
    Record { line: u64, cause: RecordError },
    /// A valid operation would exceed the numeric range.
    Overflow { line: u64 },
    /// Writing or flushing the report failed.
    Write(io::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Read(e) => write!(f, "cannot read input: {e}"),
            Error::Header(found) => write!(
                f,
                "unexpected header {found:?}, expected \"type,client,tx,amount\""
            ),
            Error::Record { line, cause } => write!(f, "line {line}: {cause}"),
            Error::Overflow { line } => {
                write!(f, "line {line}: amount exceeds the supported range")
            }
            Error::Write(e) => write!(f, "cannot write output: {e}"),
        }
    }
}

impl std::error::Error for Error {}

/// Processes every record of `input` and writes the account report to `output`.
pub fn run<R: io::Read, W: io::Write>(input: R, output: W) -> Result<(), Error> {
    let mut reader = csv_io::reader(input);
    let headers = reader.headers().map_err(Error::Read)?.clone();
    // An empty input has an empty header record and is treated as an empty batch.
    if !headers.is_empty() && headers.iter().ne(csv_io::INPUT_HEADER) {
        return Err(Error::Header(headers.iter().collect::<Vec<_>>().join(",")));
    }

    let mut engine = engine::Engine::new();
    let mut record = csv::StringRecord::new();
    // One record at a time: the file is never held in memory.
    while reader.read_record(&mut record).map_err(Error::Read)? {
        let line = record.position().map_or(0, |p| p.line());
        let transaction = csv_io::parse_record(&record, &headers)
            .map_err(|cause| Error::Record { line, cause })?;
        // Ignored outcomes are normal; only overflow stops the run.
        engine
            .apply(transaction)
            .map_err(|engine::Overflow| Error::Overflow { line })?;
    }

    csv_io::write_report(&engine, output).map_err(Error::Write)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run_str(input: &[u8]) -> (Result<(), Error>, String) {
        let mut out = Vec::new();
        let result = run(input, &mut out);
        (result, String::from_utf8(out).unwrap())
    }

    #[test]
    fn a_fatal_record_stops_the_run_and_names_its_line() {
        let (result, out) = run_str(
            b"type,client,tx,amount\ndeposit,1,1,10.0\ndeposit,1,2,-5.0\ndeposit,1,3,7.0\n",
        );
        match result {
            Err(Error::Record {
                line: 3,
                cause: RecordError::Amount(_),
            }) => {}
            other => panic!("unexpected {other:?}"),
        }
        assert_eq!(out, "");
    }

    #[test]
    fn overflow_stops_the_run_and_names_its_line() {
        let (result, out) = run_str(
            b"type,client,tx,amount\ndeposit,1,1,922337203685477.5807\ndeposit,1,2,0.0001\n",
        );
        assert!(
            matches!(result, Err(Error::Overflow { line: 3 })),
            "{result:?}"
        );
        assert_eq!(out, "");
    }

    #[test]
    fn invalid_utf8_stops_the_run() {
        let (result, out) = run_str(b"type,client,tx,amount\ndeposit,1,1,10\ndep\xffosit,1,2,1\n");
        assert!(matches!(result, Err(Error::Read(_))), "{result:?}");
        let message = result.unwrap_err().to_string();
        assert!(
            message.contains("line 3") || message.contains("line: 3"),
            "{message}"
        );
        assert_eq!(out, "");
    }

    #[test]
    fn a_wrong_header_stops_the_run() {
        let (result, out) = run_str(b"kind,customer,id,value\ndeposit,1,1,1\n");
        assert!(matches!(result, Err(Error::Header(_))), "{result:?}");
        assert_eq!(out, "");
    }

    #[test]
    fn a_header_with_spaces_is_accepted() {
        let (result, out) = run_str(b"type, client, tx, amount\ndeposit, 1, 1, 1.0\n");
        result.unwrap();
        assert_eq!(
            out,
            "client,available,held,total,locked\n1,1.0000,0.0000,1.0000,false\n"
        );
    }

    #[test]
    fn empty_and_header_only_inputs_give_a_header_only_report() {
        for input in [
            &b""[..],
            b"type,client,tx,amount\n",
            b"type,client,tx,amount",
        ] {
            let (result, out) = run_str(input);
            result.unwrap();
            assert_eq!(out, "client,available,held,total,locked\n");
        }
    }

    #[test]
    fn blank_lines_are_skipped() {
        let (result, out) = run_str(b"type,client,tx,amount\n\ndeposit,1,1,1\n\n");
        result.unwrap();
        assert_eq!(
            out,
            "client,available,held,total,locked\n1,1.0000,0.0000,1.0000,false\n"
        );
    }

    #[test]
    fn a_utf8_byte_order_mark_is_accepted() {
        let (result, out) = run_str(b"\xef\xbb\xbftype,client,tx,amount\ndeposit,1,1,1\n");
        result.unwrap();
        assert_eq!(
            out,
            "client,available,held,total,locked\n1,1.0000,0.0000,1.0000,false\n"
        );
    }

    /// Yields some valid bytes, then fails.
    struct FailingReader {
        data: &'static [u8],
    }

    impl io::Read for FailingReader {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            if self.data.is_empty() {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "disk on fire",
                ));
            }
            let n = self.data.len().min(buf.len());
            buf[..n].copy_from_slice(&self.data[..n]);
            self.data = &self.data[n..];
            Ok(n)
        }
    }

    #[test]
    fn a_read_failure_after_valid_records_writes_nothing() {
        let input = FailingReader {
            data: b"type,client,tx,amount\ndeposit,1,1,1\ndeposit,2,2,1\n",
        };
        let mut out = Vec::new();
        let result = run(input, &mut out);
        assert!(matches!(result, Err(Error::Read(_))), "{result:?}");
        assert!(out.is_empty());
    }

    /// Fails on `write`, on `flush`, or both.
    struct FailingWriter {
        fail_on_write: bool,
        fail_on_flush: bool,
    }

    impl io::Write for FailingWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            if self.fail_on_write {
                Err(io::Error::new(io::ErrorKind::BrokenPipe, "write refused"))
            } else {
                Ok(buf.len())
            }
        }
        fn flush(&mut self) -> io::Result<()> {
            if self.fail_on_flush {
                Err(io::Error::new(io::ErrorKind::BrokenPipe, "flush refused"))
            } else {
                Ok(())
            }
        }
    }

    const ONE_DEPOSIT: &[u8] = b"type,client,tx,amount\ndeposit,1,1,1\n";

    #[test]
    fn a_write_failure_is_reported() {
        let result = run(
            ONE_DEPOSIT,
            FailingWriter {
                fail_on_write: true,
                fail_on_flush: false,
            },
        );
        match result {
            Err(Error::Write(e)) => assert!(e.to_string().contains("write refused"), "{e}"),
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn a_flush_failure_is_reported() {
        let result = run(
            ONE_DEPOSIT,
            FailingWriter {
                fail_on_write: false,
                fail_on_flush: true,
            },
        );
        match result {
            Err(Error::Write(e)) => assert!(e.to_string().contains("flush refused"), "{e}"),
            other => panic!("unexpected {other:?}"),
        }
    }
}
