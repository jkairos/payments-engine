# payments-engine

A small Rust command-line program. It reads a CSV file of account transactions, applies them in file order, and prints each client's resulting balances as CSV.

## Usage

```bash
cargo build
cargo run -- transactions.csv > accounts.csv
```

The program takes exactly one argument, the input path. The account report goes to standard output. Errors go to standard error.

To try it on an included example:

```bash
cargo run -- tests/fixtures/basic.csv
```

## Input

A header row `type,client,tx,amount`, then one record per line. Whitespace around fields is ignored.

| type | effect | amount |
|---|---|---|
| `deposit` | credit the client's account | required |
| `withdrawal` | debit the account if enough funds are available | required |
| `dispute` | hold the funds of an earlier deposit | not used |
| `resolve` | release a disputed deposit | not used |
| `chargeback` | reverse a disputed deposit and lock the account | not used |

`client` is an unsigned 16-bit integer and `tx` an unsigned 32-bit integer, written as plain decimal digits. `amount` is a non-negative decimal: digits with an optional decimal point, no sign and no exponent, with up to four fractional digits.

```csv
type,client,tx,amount
deposit,7,101,250.00
deposit,7,205,40.5
withdrawal,7,150,99.9999
dispute,7,205,
```

## Output

One row per client, ordered by client id. Amounts always have four decimals.

```csv
client,available,held,total,locked
7,150.0001,40.5000,190.5001,false
```

## Assumptions

Where the requirements were silent I chose the following.

1. Only deposits can be disputed. The hold-then-reverse movement is the reversal of a credit.
2. A dispute applies even if the deposited money was already withdrawn. `available`, and after a chargeback `total`, can be negative. `held` cannot.
3. A chargeback locks the account. Every later operation for that client is ignored, including disputes, resolutions and chargebacks of its earlier deposits. Funds held at that moment stay held; settling them would need an administrative action outside this program. Other clients are unaffected, and a lock never clears.
4. A deposit moves between *undisputed* and *disputed*; a chargeback is final. Repeated or out-of-order events are ignored. A resolved deposit can be disputed again.
5. A dispute, resolve or chargeback must name the client that made the deposit. Otherwise it is ignored.
6. Any valid record creates the client's account if it does not exist, even if the operation is then ignored.
7. Zero is a valid amount. A zero deposit is recorded like any other and can be disputed, resolved and charged back, which locks the account. A zero withdrawal follows the normal rule: it applies when available funds are not negative.
8. Beyond the required forms, `5.`, `.5`, leading zeros and extra trailing zeros (`2.50000`) are accepted because they are exact. Nothing is ever rounded. The largest supported value is 922337203685477.5807.
9. Transaction ids are trusted to be unique. The one exception: a deposit that reuses the id of a recorded deposit is ignored rather than allowed to overwrite it.
10. A dispute, resolve or chargeback may leave the amount field empty or omit it. Any value there is ignored.
11. An empty file, or one with only the header, produces a report with only the header line. Empty lines and a UTF-8 byte-order mark are accepted; a line containing only spaces is not empty and is rejected as a record with the wrong number of fields. Transaction types are case-sensitive.

## Errors

| Situation | Behaviour |
|---|---|
| Operation refused by a rule: insufficient funds, unknown or foreign transaction reference, repeated or out-of-order dispute event, locked account, reused deposit id | Ignored; processing continues |
| Record that cannot be interpreted: unknown type, invalid id, missing or invalid amount, wrong field count, invalid UTF-8 | The run stops: `error: line N: cause` (or the reader's own position for invalid UTF-8), exit status 1, no report |
| Operation that would exceed the numeric range | The run stops the same way |
| Header is not `type,client,tx,amount`, or the input cannot be opened or read | `error: cause`, exit status 1, no report |
| Output cannot be written or flushed | `error: cause`, exit status 1; part of the report may already have been written |
| Wrong number of arguments | Usage message, exit status 2 |

The report is written only after the whole input has been processed, so a failure while reading never leaves a partial report.

## Correctness

- Money is an integer count of ten-thousandths with checked arithmetic. There is no floating point and no rounding.
- The types rule out invalid combinations: a deposit always carries a non-negative amount, a dispute never carries one, and client and transaction ids cannot be mixed up.
- Normal refusals and fatal errors are different types: applying a transaction returns `Ok(Applied)`, `Ok(Ignored(reason))` or `Err(Overflow)`.
- An operation either applies completely or changes nothing.
- No `unsafe` code (`#![forbid(unsafe_code)]`), and no `unwrap`, `expect`, `println!` or `eprintln!` outside tests.

Tests and sample data are described in [docs/testing.md](docs/testing.md). Design decisions are in [docs/design.md](docs/design.md).

```bash
cargo test --locked
cargo test --release --locked
```

## Performance and limits

- One pass over the input, read one record at a time; the file is never held in memory.
- Memory grows with the number of clients (at most 65,536) and the number of accepted deposits, because any deposit may be disputed later. Withdrawals and ignored operations are not remembered.
- Single-threaded.

## AI assistance

AI tools contributed to this project:

- ChatGPT/Codex: initial requirements analysis, preparation of the planning prompt, and an independent review of the plan.
- Claude Code: investigation and drafting of the plan, and implementation of the code, tests and documentation under my direction.

I reviewed the design decisions and the result. The complete transcripts of these sessions are kept outside this repository and are included with the submission.
