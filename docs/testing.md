# Testing

## Running

```bash
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test --locked
cargo test --release --locked
```

Both test profiles matter: unchecked integer arithmetic panics in debug builds and wraps in release builds, and the boundary tests assert exact strings in both.

## Layout

| Where | What |
|---|---|
| `src/money.rs` (unit) | Amount parsing, rejection, rendering, overflow |
| `src/engine.rs` (unit) | Every rule, the dispute lifecycle, the lock, zero amounts, overflow, atomicity, invariants |
| `src/csv_io.rs` (unit) | Record decoding and the report writer |
| `src/lib.rs` (unit) | `run`: fatal errors, header handling, empty input, I/O failures |
| `tests/fixtures.rs` | Each `tests/fixtures/<name>.csv` through the library, compared with `<name>.expected.csv` |
| `tests/cli.rs` | The compiled binary: exit status, stdout and stderr |

Fixture comparison is exact. Row order is not required by the format, but this program always writes clients in ascending order, so exact comparison is safe and also checks the header, the four decimals and the boolean spelling.

Expected balances are literal values calculated by hand. They are never computed by the code under test. The engine tests also check the invariants after every step: `total = available + held`, `held >= 0`, and `held` equal to the sum of the client's disputed deposits (computed from the deposit history, independently of the account).

## What is covered

"Stated" rows check behaviour the requirements state. "Assumption" rows check the choices listed in the README. "Defensive" rows cover input outside the stated guarantees.

| Kind | Behaviour | Tests |
|---|---|---|
| Stated | Deposit and withdrawal move available and total | `deposit_and_withdrawal_move_available_and_total` |
| Stated | Exact-balance withdrawal applies; one unit more is ignored | `withdrawal_of_exact_balance_succeeds_and_one_unit_more_is_ignored` |
| Stated | Dispute holds, resolve releases, chargeback reverses and locks | `dispute_holds_resolve_releases_chargeback_reverses_and_locks` |
| Stated | References to unknown transactions are ignored | `references_to_unknown_transactions_are_ignored` |
| Stated | Resolve or chargeback of an undisputed deposit is ignored | `resolve_or_chargeback_of_an_undisputed_deposit_is_ignored` |
| Stated | Held funds are not available for withdrawal | `held_funds_are_not_available_for_withdrawal` |
| Stated | Whitespace and zero to four decimals accepted | `trims_whitespace_around_every_field`, `parses_every_precision_from_zero_to_four_places`, fixture `precision_whitespace` |
| Stated | Boundary ids 0, 65535 and 4294967295 | `accepts_boundary_ids`, fixture `precision_whitespace` |
| Stated | Four-decimal rendering, including both numeric extremes | `renders_exactly_four_decimals_with_correct_sign` |
| Stated | Repeated small amounts add up exactly | `repeated_small_additions_are_exact` |
| Assumption | Withdrawals cannot be disputed | `withdrawals_cannot_be_disputed` |
| Assumption | Disputing spent funds makes available, then total, negative | `disputing_spent_funds_makes_available_then_total_negative`, fixture `chargeback_spent_deposit` |
| Assumption | A locked account ignores deposits, withdrawals and every dispute event, including one on a second open dispute | `locked_account_ignores_deposits_and_withdrawals`, `locked_account_ignores_every_dispute_event_including_a_second_open_dispute`, fixture `chargeback_spent_deposit` |
| Assumption | A lock affects only its own client | `lock_affects_only_its_own_client` |
| Assumption | Repeated dispute events never move funds twice; re-dispute after resolve applies | `repeated_dispute_events_never_move_funds_twice`, `a_charged_back_deposit_is_final`, fixtures `dispute_resolve`, `cross_client_and_repeats` |
| Assumption | References naming another client's deposit are ignored (all three events) | `references_naming_another_clients_deposit_are_ignored`, fixture `cross_client_and_repeats` |
| Assumption | An ignored operation still creates a zero account for a new client | `an_ignored_operation_still_creates_a_zero_account_for_a_new_client` |
| Assumption | Several disputes hold their sum and resolve independently | `several_disputes_hold_their_sum_and_resolve_independently` |
| Assumption | A deposit reusing a recorded id is ignored; one refused on a locked account is never recorded | `a_deposit_reusing_a_recorded_id_is_ignored`, `a_deposit_ignored_on_a_locked_account_is_never_recorded` |
| Assumption | Zero amounts: recorded, full dispute lifecycle, ordinary withdrawal rule | `parses_zero`, `a_zero_deposit_creates_the_account_and_is_recorded`, `a_zero_deposit_goes_through_the_whole_dispute_lifecycle`, `a_zero_withdrawal_uses_the_ordinary_sufficiency_rule`, fixture `zero_amounts` |
| Assumption | Lenient exact forms `5.`, `.5`, `007.50`, `2.50000` | `parses_lenient_exact_forms` |
| Assumption | Dispute event with the amount omitted, or with a value that is ignored | `accepts_a_dispute_event_with_the_amount_field_omitted`, `ignores_a_value_in_the_amount_field_of_a_dispute_event` |
| Assumption | Empty and header-only input, blank lines, byte-order mark, spaced header | `empty_and_header_only_inputs_give_a_header_only_report`, `blank_lines_are_skipped`, `a_utf8_byte_order_mark_is_accepted`, `a_header_with_spaces_is_accepted`, fixtures `empty`, `header_only` |
| Assumption | Output ordered by client id | `renders_the_report_in_client_order_with_four_decimals` |
| Assumption | Ignored operations change no balance, lock or dispute state (only a new zero account may appear) | `ignored_operations_change_no_balance_lock_or_dispute_state` |
| Assumption | Dispute then resolve restores the exact prior state; clients are isolated | `dispute_then_resolve_restores_the_exact_prior_state`, `clients_are_isolated_from_each_other` |
| Defensive | Overflow on deposit or dispute changes nothing and stops the run | `a_deposit_that_would_overflow_changes_nothing`, `a_dispute_that_would_overflow_held_changes_nothing`, `reports_overflow_at_both_ends`, `overflow_stops_the_run_and_names_its_line`, `overflow_exits_1_with_empty_stdout_and_names_the_line` |
| Defensive | Malformed amounts, ids, types and field counts are fatal, one form at a time | `rejects_malformed_text`, `rejects_inexact_values`, `rejects_values_out_of_range`, `rejects_records_that_cannot_be_interpreted` |
| Defensive | A fatal record stops the run with no report | `a_fatal_record_stops_the_run_and_names_its_line`, `a_fatal_record_exits_1_with_empty_stdout_and_names_the_line`, `an_invalid_client_id_names_the_line_and_the_column` |
| Defensive | Invalid UTF-8 and a wrong header stop the run | `invalid_utf8_stops_the_run`, `a_wrong_header_stops_the_run`, `a_wrong_header_exits_1_with_empty_stdout` |
| Defensive | Quoted fields and CRLF line endings | `accepts_quoted_fields_and_crlf_line_endings` |
| Defensive | Read failure after valid records: error, nothing written | `a_read_failure_after_valid_records_writes_nothing` |
| Defensive | Write failure and flush-only failure are reported | `a_write_failure_is_reported`, `a_flush_failure_is_reported`, `a_full_output_device_exits_1_without_panicking` |
| Defensive | Ids written with a sign or in hexadecimal are fatal | `rejects_records_that_cannot_be_interpreted` |
| Defensive | An unwritable standard error does not cause a panic | `an_unwritable_stderr_does_not_cause_a_panic` |
| Defensive | Argument count, missing file, directory path | `wrong_argument_counts_exit_2_with_usage`, `a_missing_file_exits_1_with_empty_stdout`, `a_directory_path_exits_1` |

Not implemented: a generated large-input test and a randomised tally test.

## Fixtures

Each input has a hand-calculated expected output beside it.

| File | Shows | Final rows |
|---|---|---|
| `basic.csv` | Deposits, withdrawals, exact-balance and ignored withdrawals | `3`: 0.0000; `7`: 150.0002; `12`: 999.7500 |
| `dispute_resolve.csv` | Dispute, resolve, ignored references, re-dispute | `1`: 80.0000 available, 20.2500 held; `2`: 5.5000 held |
| `chargeback_spent_deposit.csv` | Spent deposit, two disputes, lock, operations after the lock | `5`: −110.0000 available, 30.0000 held, −80.0000 total, locked; `6`: 9.9900 |
| `cross_client_and_repeats.csv` | Wrong owner, repeats, reused id, new clients | `20`: 50.0000; `21`: 0.0000, locked; `30`, `31`: zero |
| `precision_whitespace.csv` | Spacing, precision, boundary ids, largest amount | `0`: 0.5000; `2`: 15.9375; `65535`: 922337203685477.5807 |
| `zero_amounts.csv` | Zero deposit, withdrawal and dispute lifecycle | `40`: zero, locked; `41`: −10.0000 available, 10.0000 held; `42`: zero |
| `empty.csv`, `header_only.csv` | No transactions | Header line only |
| `fatal_negative_amount.csv` | A record that stops the run | No report; exit 1 |
| `fatal_overflow.csv` | An overflow that stops the run | No report; exit 1 |
| `wrong_header.csv` | Unexpected header | No report; exit 1 |

Arithmetic for `chargeback_spent_deposit.csv`: deposits 130 − withdrawal 110 − charged back 100 = −80 total; available 130 − 110 − 100 − 30 = −110; held 30 (second dispute, still open when the account locked). The resolve after the lock is ignored.

## Fresh-clone check

```bash
git clone https://github.com/jkairos/payments-engine.git
cd payments-engine
cargo build --locked
cp tests/fixtures/basic.csv transactions.csv
cargo run -- transactions.csv > accounts.csv
diff -u tests/fixtures/basic.expected.csv accounts.csv
```

## Verification results

Local runs on 2026-11-10, Linux x86_64:

| Check | Toolchain | Result |
|---|---|---|
| `cargo fmt --all -- --check` | 1.93.1 | No differences |
| `cargo clippy --all-targets --locked -- -D warnings` | 1.93.1 | No warnings |
| `cargo test --locked` | 1.93.1 | 74 passed, 0 failed |
| `cargo test --release --locked` | 1.93.1 | 74 passed, 0 failed |
| `cargo test --locked` and `cargo test --release --locked` | 1.73.0 | 74 passed, 0 failed in each |
| `cargo run -- transactions.csv > accounts.csv` with `basic.csv` | 1.93.1 | Output identical to `basic.expected.csv` |
| Build, both test profiles and the command above from a fresh copy of the tracked files with an empty Cargo home | 1.93.1 | All passed |

Hosted and clone checks for commit `d1d54c7`:

| Check | Result |
|---|---|
| GitHub Actions, stable and 1.73 jobs | Both passed ([run 42932195497](https://github.com/jkairos/payments-engine/actions/runs/42932195497)) |
| Anonymous HTTPS clone, empty Cargo home: `cargo build --locked`, both test profiles, `cargo run -- tests/fixtures/basic.csv > accounts.csv`, and the `transactions.csv` interface | 74 passed in each profile; both outputs identical to `basic.expected.csv` |
