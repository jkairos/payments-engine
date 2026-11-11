//! CSV boundary: reader configuration, record validation and the account report.

use std::fmt;
use std::io;

use serde::Deserialize;

use crate::engine::{ClientId, Engine, Transaction, TxId};
use crate::money::{Amount, AmountError};

/// The header every input file must start with (after trimming whitespace).
pub const INPUT_HEADER: [&str; 4] = ["type", "client", "tx", "amount"];
/// The header of the account report.
pub const OUTPUT_HEADER: [&str; 5] = ["client", "available", "held", "total", "locked"];

/// Why one input record could not be interpreted.
#[derive(Debug)]
pub enum RecordError {
    /// A record must have 3 fields (dispute events) or 4.
    FieldCount(usize),
    /// A field could not be decoded, e.g. an id that is not a valid u16/u32.
    Field(csv::Error),
    /// A client or transaction id that is not a plain unsigned decimal in range.
    InvalidId(&'static str),
    UnknownType(String),
    MissingAmount,
    Amount(AmountError),
}

impl fmt::Display for RecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            RecordError::FieldCount(n) => write!(f, "expected 3 or 4 fields, found {n}"),
            RecordError::Field(e) => match e.kind() {
                // Name the column rather than csv's zero-based field index.
                csv::ErrorKind::Deserialize { err, .. } => {
                    let column = err
                        .field()
                        .and_then(|i| INPUT_HEADER.get(i as usize))
                        .unwrap_or(&"field");
                    write!(f, "invalid {column}: {}", err.kind())
                }
                _ => write!(f, "{e}"),
            },
            RecordError::InvalidId(column) => write!(f, "invalid {column}"),
            RecordError::UnknownType(t) => write!(f, "unknown transaction type {t:?}"),
            RecordError::MissingAmount => f.write_str("missing amount"),
            RecordError::Amount(e) => write!(f, "invalid amount: {e}"),
        }
    }
}

/// Field layout of one input row. Borrows from the reused record buffer.
#[derive(Debug, Deserialize)]
struct Row<'a> {
    #[serde(rename = "type")]
    kind: &'a str,
    client: &'a str,
    tx: &'a str,
    #[serde(borrow, default)]
    amount: Option<&'a str>,
}

/// Builds the input reader: header row, surrounding whitespace trimmed,
/// and records allowed to omit the trailing amount field.
pub fn reader<R: io::Read>(input: R) -> csv::Reader<R> {
    csv::ReaderBuilder::new()
        .trim(csv::Trim::All)
        .flexible(true)
        .from_reader(input)
}

/// Converts one raw record into a validated transaction.
pub fn parse_record(
    record: &csv::StringRecord,
    headers: &csv::StringRecord,
) -> Result<Transaction, RecordError> {
    if !(3..=4).contains(&record.len()) {
        return Err(RecordError::FieldCount(record.len()));
    }
    let row: Row = record
        .deserialize(Some(headers))
        .map_err(RecordError::Field)?;
    let client = ClientId(parse_id(row.client).ok_or(RecordError::InvalidId("client"))?);
    let tx = TxId(parse_id(row.tx).ok_or(RecordError::InvalidId("tx"))?);
    let amount = || -> Result<Amount, RecordError> {
        match row.amount {
            None | Some("") => Err(RecordError::MissingAmount),
            Some(text) => text.parse().map_err(RecordError::Amount),
        }
    };
    // The amount field of a dispute event is never read.
    Ok(match row.kind {
        "deposit" => Transaction::Deposit {
            client,
            tx,
            amount: amount()?,
        },
        "withdrawal" => Transaction::Withdrawal {
            client,
            tx,
            amount: amount()?,
        },
        "dispute" => Transaction::Dispute { client, tx },
        "resolve" => Transaction::Resolve { client, tx },
        "chargeback" => Transaction::Chargeback { client, tx },
        other => return Err(RecordError::UnknownType(other.to_owned())),
    })
}

/// Parses a plain unsigned decimal id. `str::parse` alone would also accept a
/// leading `+`, and csv's own integer decoding would accept `0x` hexadecimal.
fn parse_id<T: std::str::FromStr>(text: &str) -> Option<T> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

/// Writes one row per account, in ascending client order, then flushes.
pub fn write_report<W: io::Write>(engine: &Engine, output: W) -> io::Result<()> {
    let mut writer = csv::Writer::from_writer(output);
    writer.write_record(OUTPUT_HEADER)?;
    for (client, account) in engine.accounts() {
        writer.write_record([
            client.0.to_string(),
            account.available().to_string(),
            account.held().to_string(),
            account.total().to_string(),
            account.locked().to_string(),
        ])?;
    }
    // Flush explicitly: errors raised while dropping a buffered writer are lost.
    writer.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Result<Transaction, RecordError> {
        let text = format!("type,client,tx,amount\n{line}\n");
        let mut rdr = reader(text.as_bytes());
        let headers = rdr.headers().unwrap().clone();
        let record = rdr.records().next().expect("one record").unwrap();
        parse_record(&record, &headers)
    }

    fn amount(s: &str) -> Amount {
        s.parse().unwrap()
    }

    #[test]
    fn parses_each_transaction_type() {
        let (c, t) = (ClientId(1), TxId(2));
        assert_eq!(
            parse("deposit,1,2,3.5").unwrap(),
            Transaction::Deposit {
                client: c,
                tx: t,
                amount: amount("3.5")
            }
        );
        assert_eq!(
            parse("withdrawal,1,2,3").unwrap(),
            Transaction::Withdrawal {
                client: c,
                tx: t,
                amount: amount("3")
            }
        );
        assert_eq!(
            parse("dispute,1,2,").unwrap(),
            Transaction::Dispute { client: c, tx: t }
        );
        assert_eq!(
            parse("resolve,1,2,").unwrap(),
            Transaction::Resolve { client: c, tx: t }
        );
        assert_eq!(
            parse("chargeback,1,2,").unwrap(),
            Transaction::Chargeback { client: c, tx: t }
        );
    }

    #[test]
    fn trims_whitespace_around_every_field() {
        assert_eq!(
            parse("  deposit ,  1 ,   2 ,  3.5  ").unwrap(),
            Transaction::Deposit {
                client: ClientId(1),
                tx: TxId(2),
                amount: amount("3.5")
            }
        );
    }

    #[test]
    fn accepts_a_dispute_event_with_the_amount_field_omitted() {
        assert_eq!(
            parse("dispute,1,2").unwrap(),
            Transaction::Dispute {
                client: ClientId(1),
                tx: TxId(2)
            }
        );
    }

    #[test]
    fn ignores_a_value_in_the_amount_field_of_a_dispute_event() {
        assert_eq!(
            parse("resolve,1,2,not-a-number").unwrap(),
            Transaction::Resolve {
                client: ClientId(1),
                tx: TxId(2)
            }
        );
    }

    #[test]
    fn accepts_boundary_ids() {
        assert_eq!(
            parse("deposit,65535,4294967295,1").unwrap(),
            Transaction::Deposit {
                client: ClientId(65535),
                tx: TxId(u32::MAX),
                amount: amount("1")
            }
        );
        assert!(matches!(
            parse("deposit,0,0,1"),
            Ok(Transaction::Deposit { .. })
        ));
    }

    #[test]
    fn accepts_quoted_fields_and_crlf_line_endings() {
        let text = "type,client,tx,amount\r\n\"deposit\",\"1\",\"2\",\"3.5\"\r\n";
        let mut rdr = reader(text.as_bytes());
        let headers = rdr.headers().unwrap().clone();
        let record = rdr.records().next().unwrap().unwrap();
        assert!(matches!(
            parse_record(&record, &headers),
            Ok(Transaction::Deposit { .. })
        ));
    }

    #[test]
    fn rejects_records_that_cannot_be_interpreted() {
        type Check = fn(&RecordError) -> bool;
        let cases: &[(&str, Check)] = &[
            ("deposit,1,5,", |e| matches!(e, RecordError::MissingAmount)),
            ("deposit,1,5", |e| matches!(e, RecordError::MissingAmount)),
            ("withdrawal,1,8,abc", |e| {
                matches!(e, RecordError::Amount(AmountError::Syntax))
            }),
            ("deposit,1,4,1e3", |e| {
                matches!(e, RecordError::Amount(AmountError::Syntax))
            }),
            ("deposit,1,2,-5.0", |e| {
                matches!(e, RecordError::Amount(AmountError::Syntax))
            }),
            ("deposit,1,2,+5.0", |e| {
                matches!(e, RecordError::Amount(AmountError::Syntax))
            }),
            ("deposit,1,2,-0", |e| {
                matches!(e, RecordError::Amount(AmountError::Syntax))
            }),
            ("deposit,1,4,1.00001", |e| {
                matches!(e, RecordError::Amount(AmountError::TooPrecise))
            }),
            ("deposit,1,4,922337203685477.5808", |e| {
                matches!(e, RecordError::Amount(AmountError::OutOfRange))
            }),
            (
                "transfer,1,6,1.0",
                |e| matches!(e, RecordError::UnknownType(t) if t == "transfer"),
            ),
            ("Deposit,1,6,1.0", |e| {
                matches!(e, RecordError::UnknownType(_))
            }),
            ("deposit,70000,7,1.0", |e| {
                matches!(e, RecordError::InvalidId("client"))
            }),
            ("deposit,-1,7,1.0", |e| {
                matches!(e, RecordError::InvalidId("client"))
            }),
            ("deposit,+7,7,1.0", |e| {
                matches!(e, RecordError::InvalidId("client"))
            }),
            ("deposit,0x10,7,1.0", |e| {
                matches!(e, RecordError::InvalidId("client"))
            }),
            ("deposit,,7,1.0", |e| {
                matches!(e, RecordError::InvalidId("client"))
            }),
            ("deposit,1,x,1.0", |e| {
                matches!(e, RecordError::InvalidId("tx"))
            }),
            ("deposit,1,+1,1.0", |e| {
                matches!(e, RecordError::InvalidId("tx"))
            }),
            ("deposit,1,0xFF,1.0", |e| {
                matches!(e, RecordError::InvalidId("tx"))
            }),
            ("deposit,1,4294967296,1.0", |e| {
                matches!(e, RecordError::InvalidId("tx"))
            }),
            ("deposit,1,11,1.5,extra", |e| {
                matches!(e, RecordError::FieldCount(5))
            }),
            ("deposit,1", |e| matches!(e, RecordError::FieldCount(2))),
            ("dispute,1", |e| matches!(e, RecordError::FieldCount(2))),
        ];
        for (line, expected) in cases {
            match parse(line) {
                Err(e) => assert!(expected(&e), "{line:?} gave {e:?}"),
                Ok(t) => panic!("{line:?} was accepted as {t:?}"),
            }
        }
    }

    #[test]
    fn renders_the_report_in_client_order_with_four_decimals() {
        let mut engine = Engine::new();
        for line in ["deposit,12,1,1", "deposit,3,2,0.5", "withdrawal,7,3,1"] {
            engine.apply(parse(line).unwrap()).unwrap();
        }
        let mut out = Vec::new();
        write_report(&engine, &mut out).unwrap();
        assert_eq!(
            String::from_utf8(out).unwrap(),
            "client,available,held,total,locked\n\
             3,0.5000,0.0000,0.5000,false\n\
             7,0.0000,0.0000,0.0000,false\n\
             12,1.0000,0.0000,1.0000,false\n"
        );
    }
}
