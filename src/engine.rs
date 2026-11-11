//! Account state and the rules for each transaction. No I/O happens here.

use std::collections::{BTreeMap, HashMap};

use crate::money::{Amount, Money};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ClientId(pub u16);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TxId(pub u32);

/// One input record, already validated.
///
/// Deposits and withdrawals always carry an amount; the dispute events never do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transaction {
    Deposit {
        client: ClientId,
        tx: TxId,
        amount: Amount,
    },
    Withdrawal {
        client: ClientId,
        tx: TxId,
        amount: Amount,
    },
    Dispute {
        client: ClientId,
        tx: TxId,
    },
    Resolve {
        client: ClientId,
        tx: TxId,
    },
    Chargeback {
        client: ClientId,
        tx: TxId,
    },
}

impl Transaction {
    pub fn client(&self) -> ClientId {
        match *self {
            Transaction::Deposit { client, .. }
            | Transaction::Withdrawal { client, .. }
            | Transaction::Dispute { client, .. }
            | Transaction::Resolve { client, .. }
            | Transaction::Chargeback { client, .. } => client,
        }
    }
}

/// The normal result of applying a transaction. Processing continues either way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Applied,
    Ignored(IgnoreReason),
}

/// Why a well-formed transaction was refused. None of these change any balance,
/// lock or dispute state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IgnoreReason {
    AccountLocked,
    InsufficientFunds,
    DuplicateTransaction,
    UnknownTransaction,
    ClientMismatch,
    InvalidDisputeState,
}

/// Fatal: applying the transaction would exceed the numeric range.
/// The failing transaction has changed nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Overflow;

/// One client's balances. Fields are private and change only through the
/// methods below, each of which transcribes one rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Account {
    available: Money,
    held: Money,
    total: Money,
    locked: bool,
}

impl Account {
    pub fn available(&self) -> Money {
        self.available
    }
    pub fn held(&self) -> Money {
        self.held
    }
    pub fn total(&self) -> Money {
        self.total
    }
    pub fn locked(&self) -> bool {
        self.locked
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DisputeState {
    Undisputed,
    Disputed,
    ChargedBack,
}

/// A deposit that was applied, kept because it may be disputed later.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Deposit {
    client: ClientId,
    amount: Money,
    state: DisputeState,
}

#[derive(Debug, Default)]
pub struct Engine {
    accounts: BTreeMap<ClientId, Account>,
    deposits: HashMap<TxId, Deposit>,
}

impl Engine {
    pub fn new() -> Engine {
        Engine::default()
    }

    /// Accounts in ascending client order.
    pub fn accounts(&self) -> impl Iterator<Item = (ClientId, &Account)> {
        self.accounts.iter().map(|(id, account)| (*id, account))
    }

    /// Applies one transaction.
    ///
    /// Order of work: ensure the account exists, refuse everything on a locked
    /// account, check the operation's rules, compute new balances with checked
    /// arithmetic, and only then write balances, history and lock together.
    pub fn apply(&mut self, transaction: Transaction) -> Result<Outcome, Overflow> {
        let account = self.accounts.entry(transaction.client()).or_default();
        if account.locked {
            return Ok(Outcome::Ignored(IgnoreReason::AccountLocked));
        }

        match transaction {
            Transaction::Deposit { client, tx, amount } => {
                let amount = amount.money();
                let std::collections::hash_map::Entry::Vacant(slot) = self.deposits.entry(tx)
                else {
                    return Ok(Outcome::Ignored(IgnoreReason::DuplicateTransaction));
                };
                *account = account.deposit(amount)?;
                slot.insert(Deposit {
                    client,
                    amount,
                    state: DisputeState::Undisputed,
                });
            }
            Transaction::Withdrawal { amount, .. } => {
                let amount = amount.money();
                if account.available < amount {
                    return Ok(Outcome::Ignored(IgnoreReason::InsufficientFunds));
                }
                *account = account.withdraw(amount)?;
            }
            Transaction::Dispute { client, tx }
            | Transaction::Resolve { client, tx }
            | Transaction::Chargeback { client, tx } => {
                let Some(deposit) = self.deposits.get_mut(&tx) else {
                    return Ok(Outcome::Ignored(IgnoreReason::UnknownTransaction));
                };
                if deposit.client != client {
                    return Ok(Outcome::Ignored(IgnoreReason::ClientMismatch));
                }
                let (updated, next_state) = match (transaction, deposit.state) {
                    (Transaction::Dispute { .. }, DisputeState::Undisputed) => {
                        (account.hold(deposit.amount)?, DisputeState::Disputed)
                    }
                    (Transaction::Resolve { .. }, DisputeState::Disputed) => {
                        (account.release(deposit.amount)?, DisputeState::Undisputed)
                    }
                    (Transaction::Chargeback { .. }, DisputeState::Disputed) => (
                        account.charge_back(deposit.amount)?,
                        DisputeState::ChargedBack,
                    ),
                    _ => return Ok(Outcome::Ignored(IgnoreReason::InvalidDisputeState)),
                };
                *account = updated;
                deposit.state = next_state;
            }
        }
        Ok(Outcome::Applied)
    }
}

/// Each method returns the new balances without modifying `self`, so a caller
/// can commit them together with other changes or not at all.
impl Account {
    /// Credit: available and total go up.
    fn deposit(self, amount: Money) -> Result<Account, Overflow> {
        Ok(Account {
            available: add(self.available, amount)?,
            total: add(self.total, amount)?,
            ..self
        })
    }

    /// Debit: available and total go down.
    fn withdraw(self, amount: Money) -> Result<Account, Overflow> {
        Ok(Account {
            available: sub(self.available, amount)?,
            total: sub(self.total, amount)?,
            ..self
        })
    }

    /// Dispute: funds move from available to held; total is unchanged.
    fn hold(self, amount: Money) -> Result<Account, Overflow> {
        Ok(Account {
            available: sub(self.available, amount)?,
            held: add(self.held, amount)?,
            ..self
        })
    }

    /// Resolve: funds move from held back to available; total is unchanged.
    fn release(self, amount: Money) -> Result<Account, Overflow> {
        Ok(Account {
            available: add(self.available, amount)?,
            held: sub(self.held, amount)?,
            ..self
        })
    }

    /// Chargeback: held and total go down, and the account locks.
    fn charge_back(self, amount: Money) -> Result<Account, Overflow> {
        Ok(Account {
            held: sub(self.held, amount)?,
            total: sub(self.total, amount)?,
            locked: true,
            ..self
        })
    }
}

fn add(a: Money, b: Money) -> Result<Money, Overflow> {
    a.checked_add(b).ok_or(Overflow)
}

fn sub(a: Money, b: Money) -> Result<Money, Overflow> {
    a.checked_sub(b).ok_or(Overflow)
}

#[cfg(test)]
mod tests {
    use super::*;
    use IgnoreReason::*;
    use Outcome::*;

    const APPLIED: Result<Outcome, Overflow> = Ok(Applied);

    fn ignored(reason: IgnoreReason) -> Result<Outcome, Overflow> {
        Ok(Ignored(reason))
    }

    fn amount(s: &str) -> Amount {
        s.parse().expect("test amount")
    }
    fn deposit(client: u16, tx: u32, value: &str) -> Transaction {
        Transaction::Deposit {
            client: ClientId(client),
            tx: TxId(tx),
            amount: amount(value),
        }
    }
    fn withdrawal(client: u16, tx: u32, value: &str) -> Transaction {
        Transaction::Withdrawal {
            client: ClientId(client),
            tx: TxId(tx),
            amount: amount(value),
        }
    }
    fn dispute(client: u16, tx: u32) -> Transaction {
        Transaction::Dispute {
            client: ClientId(client),
            tx: TxId(tx),
        }
    }
    fn resolve(client: u16, tx: u32) -> Transaction {
        Transaction::Resolve {
            client: ClientId(client),
            tx: TxId(tx),
        }
    }
    fn chargeback(client: u16, tx: u32) -> Transaction {
        Transaction::Chargeback {
            client: ClientId(client),
            tx: TxId(tx),
        }
    }

    /// Applies one transaction and checks the invariants afterwards.
    fn step(engine: &mut Engine, t: Transaction) -> Result<Outcome, Overflow> {
        let result = engine.apply(t);
        check_invariants(engine);
        result
    }

    /// Applies several transactions, expecting none to overflow.
    fn steps(engine: &mut Engine, ts: &[Transaction]) {
        for t in ts {
            step(engine, *t).expect("no overflow");
        }
    }

    /// I01: total = available + held, held >= 0.
    /// I02: held equals the sum of the client's disputed deposits (two independent structures).
    fn check_invariants(engine: &Engine) {
        for (client, account) in engine.accounts() {
            assert_eq!(
                account.available.checked_add(account.held),
                Some(account.total)
            );
            assert!(account.held >= Money::ZERO);
            let disputed = engine
                .deposits
                .values()
                .filter(|d| d.client == client && d.state == DisputeState::Disputed)
                .try_fold(Money::ZERO, |sum, d| sum.checked_add(d.amount))
                .unwrap();
            assert_eq!(account.held, disputed, "client {client:?}");
        }
    }

    /// Balances as rendered strings plus the lock flag: the oracle is literal text.
    fn row(engine: &Engine, client: u16) -> (String, String, String, bool) {
        let a = engine
            .accounts
            .get(&ClientId(client))
            .expect("account exists");
        (
            a.available.to_string(),
            a.held.to_string(),
            a.total.to_string(),
            a.locked,
        )
    }

    fn expect(
        available: &str,
        held: &str,
        total: &str,
        locked: bool,
    ) -> (String, String, String, bool) {
        (available.into(), held.into(), total.into(), locked)
    }

    // --- Stated behaviour (A03-A11, A16) ---

    #[test]
    fn deposit_and_withdrawal_move_available_and_total() {
        let mut e = Engine::new();
        assert_eq!(step(&mut e, deposit(1, 1, "250.00")), APPLIED);
        assert_eq!(row(&e, 1), expect("250.0000", "0.0000", "250.0000", false));
        assert_eq!(step(&mut e, withdrawal(1, 2, "99.9999")), APPLIED);
        assert_eq!(row(&e, 1), expect("150.0001", "0.0000", "150.0001", false));
    }

    #[test]
    fn withdrawal_of_exact_balance_succeeds_and_one_unit_more_is_ignored() {
        let mut e = Engine::new();
        steps(&mut e, &[deposit(1, 1, "40.5")]);
        assert_eq!(
            step(&mut e, withdrawal(1, 2, "40.5001")),
            ignored(InsufficientFunds)
        );
        assert_eq!(row(&e, 1), expect("40.5000", "0.0000", "40.5000", false));
        assert_eq!(step(&mut e, withdrawal(1, 3, "40.5")), APPLIED);
        assert_eq!(row(&e, 1), expect("0.0000", "0.0000", "0.0000", false));
    }

    #[test]
    fn dispute_holds_resolve_releases_chargeback_reverses_and_locks() {
        let mut e = Engine::new();
        steps(&mut e, &[deposit(1, 1, "80")]);
        assert_eq!(step(&mut e, dispute(1, 1)), APPLIED);
        assert_eq!(row(&e, 1), expect("0.0000", "80.0000", "80.0000", false));
        assert_eq!(step(&mut e, resolve(1, 1)), APPLIED);
        assert_eq!(row(&e, 1), expect("80.0000", "0.0000", "80.0000", false));
        assert_eq!(step(&mut e, dispute(1, 1)), APPLIED);
        assert_eq!(step(&mut e, chargeback(1, 1)), APPLIED);
        assert_eq!(row(&e, 1), expect("0.0000", "0.0000", "0.0000", true));
    }

    #[test]
    fn references_to_unknown_transactions_are_ignored() {
        let mut e = Engine::new();
        steps(&mut e, &[deposit(1, 1, "10")]);
        for t in [dispute(1, 99), resolve(1, 99), chargeback(1, 99)] {
            assert_eq!(step(&mut e, t), ignored(UnknownTransaction));
        }
        assert_eq!(row(&e, 1), expect("10.0000", "0.0000", "10.0000", false));
    }

    #[test]
    fn resolve_or_chargeback_of_an_undisputed_deposit_is_ignored() {
        let mut e = Engine::new();
        steps(&mut e, &[deposit(1, 1, "10")]);
        assert_eq!(step(&mut e, resolve(1, 1)), ignored(InvalidDisputeState));
        assert_eq!(step(&mut e, chargeback(1, 1)), ignored(InvalidDisputeState));
        assert_eq!(row(&e, 1), expect("10.0000", "0.0000", "10.0000", false));
    }

    #[test]
    fn held_funds_are_not_available_for_withdrawal() {
        let mut e = Engine::new();
        steps(&mut e, &[deposit(1, 1, "0.5"), dispute(1, 1)]);
        assert_eq!(
            step(&mut e, withdrawal(1, 2, "0.5")),
            ignored(InsufficientFunds)
        );
        assert_eq!(row(&e, 1), expect("0.0000", "0.5000", "0.5000", false));
    }

    // --- Chosen policies (P01-P10) ---

    #[test]
    fn withdrawals_cannot_be_disputed() {
        let mut e = Engine::new();
        steps(&mut e, &[deposit(1, 1, "10"), withdrawal(1, 2, "4")]);
        assert_eq!(step(&mut e, dispute(1, 2)), ignored(UnknownTransaction));
        assert_eq!(row(&e, 1), expect("6.0000", "0.0000", "6.0000", false));
    }

    #[test]
    fn disputing_spent_funds_makes_available_then_total_negative() {
        let mut e = Engine::new();
        steps(&mut e, &[deposit(1, 1, "100"), withdrawal(1, 2, "80")]);
        assert_eq!(step(&mut e, dispute(1, 1)), APPLIED);
        assert_eq!(row(&e, 1), expect("-80.0000", "100.0000", "20.0000", false));
        assert_eq!(step(&mut e, chargeback(1, 1)), APPLIED);
        assert_eq!(row(&e, 1), expect("-80.0000", "0.0000", "-80.0000", true));
    }

    #[test]
    fn locked_account_ignores_deposits_and_withdrawals() {
        let mut e = Engine::new();
        steps(
            &mut e,
            &[
                deposit(1, 1, "10"),
                deposit(1, 2, "5"),
                dispute(1, 1),
                chargeback(1, 1),
            ],
        );
        assert_eq!(step(&mut e, deposit(1, 3, "50")), ignored(AccountLocked));
        assert_eq!(step(&mut e, withdrawal(1, 4, "1")), ignored(AccountLocked));
        assert_eq!(row(&e, 1), expect("5.0000", "0.0000", "5.0000", true));
    }

    #[test]
    fn locked_account_ignores_every_dispute_event_including_a_second_open_dispute() {
        let mut e = Engine::new();
        steps(
            &mut e,
            &[
                deposit(1, 1, "100"),
                deposit(1, 2, "30"),
                deposit(1, 3, "7"),
                dispute(1, 1),
                dispute(1, 2),
                chargeback(1, 1),
            ],
        );
        let frozen = expect("7.0000", "30.0000", "37.0000", true);
        assert_eq!(row(&e, 1), frozen);
        for t in [
            resolve(1, 2),
            chargeback(1, 2),
            dispute(1, 3),
            dispute(1, 1),
        ] {
            assert_eq!(step(&mut e, t), ignored(AccountLocked));
            assert_eq!(row(&e, 1), frozen);
        }
    }

    #[test]
    fn lock_affects_only_its_own_client() {
        let mut e = Engine::new();
        steps(
            &mut e,
            &[deposit(1, 1, "10"), dispute(1, 1), chargeback(1, 1)],
        );
        assert_eq!(step(&mut e, deposit(2, 2, "3")), APPLIED);
        assert_eq!(row(&e, 2), expect("3.0000", "0.0000", "3.0000", false));
    }

    #[test]
    fn repeated_dispute_events_never_move_funds_twice() {
        let mut e = Engine::new();
        steps(&mut e, &[deposit(1, 1, "10"), deposit(1, 2, "5")]);
        assert_eq!(step(&mut e, dispute(1, 1)), APPLIED);
        assert_eq!(step(&mut e, dispute(1, 1)), ignored(InvalidDisputeState));
        assert_eq!(row(&e, 1), expect("5.0000", "10.0000", "15.0000", false));
        assert_eq!(step(&mut e, resolve(1, 1)), APPLIED);
        assert_eq!(step(&mut e, resolve(1, 1)), ignored(InvalidDisputeState));
        assert_eq!(row(&e, 1), expect("15.0000", "0.0000", "15.0000", false));
        assert_eq!(
            step(&mut e, dispute(1, 1)),
            APPLIED,
            "re-dispute after resolve applies"
        );
        assert_eq!(step(&mut e, chargeback(1, 1)), APPLIED);
        // Locked now: a second chargeback cannot reverse the deposit twice.
        assert_eq!(step(&mut e, chargeback(1, 1)), ignored(AccountLocked));
        assert_eq!(row(&e, 1), expect("5.0000", "0.0000", "5.0000", true));
    }

    #[test]
    fn a_charged_back_deposit_is_final() {
        // Seen through the deposit itself, independent of the lock check order.
        let mut e = Engine::new();
        steps(
            &mut e,
            &[deposit(1, 1, "10"), dispute(1, 1), chargeback(1, 1)],
        );
        assert_eq!(e.deposits[&TxId(1)].state, DisputeState::ChargedBack);
    }

    #[test]
    fn references_naming_another_clients_deposit_are_ignored() {
        let mut e = Engine::new();
        steps(&mut e, &[deposit(1, 1, "10"), deposit(2, 2, "20")]);
        assert_eq!(step(&mut e, dispute(2, 1)), ignored(ClientMismatch));
        steps(&mut e, &[dispute(1, 1)]);
        assert_eq!(step(&mut e, resolve(2, 1)), ignored(ClientMismatch));
        assert_eq!(step(&mut e, chargeback(2, 1)), ignored(ClientMismatch));
        assert_eq!(row(&e, 1), expect("0.0000", "10.0000", "10.0000", false));
        assert_eq!(row(&e, 2), expect("20.0000", "0.0000", "20.0000", false));
    }

    #[test]
    fn an_ignored_operation_still_creates_a_zero_account_for_a_new_client() {
        let mut e = Engine::new();
        assert_eq!(
            step(&mut e, withdrawal(30, 1, "5")),
            ignored(InsufficientFunds)
        );
        assert_eq!(row(&e, 30), expect("0.0000", "0.0000", "0.0000", false));
        assert_eq!(step(&mut e, dispute(31, 1)), ignored(UnknownTransaction));
        assert_eq!(row(&e, 31), expect("0.0000", "0.0000", "0.0000", false));
    }

    #[test]
    fn a_deposit_ignored_on_a_locked_account_is_never_recorded() {
        let mut e = Engine::new();
        steps(
            &mut e,
            &[deposit(1, 1, "10"), dispute(1, 1), chargeback(1, 1)],
        );
        assert_eq!(step(&mut e, deposit(1, 2, "50")), ignored(AccountLocked));
        assert!(!e.deposits.contains_key(&TxId(2)));
    }

    #[test]
    fn a_deposit_reusing_a_recorded_id_is_ignored() {
        let mut e = Engine::new();
        steps(&mut e, &[deposit(1, 7, "15")]);
        assert_eq!(
            step(&mut e, deposit(1, 7, "99")),
            ignored(DuplicateTransaction)
        );
        steps(&mut e, &[dispute(1, 7)]);
        assert_eq!(row(&e, 1), expect("0.0000", "15.0000", "15.0000", false));
    }

    #[test]
    fn several_disputes_hold_their_sum_and_resolve_independently() {
        let mut e = Engine::new();
        steps(
            &mut e,
            &[
                deposit(1, 1, "10"),
                deposit(1, 2, "2.5"),
                dispute(1, 1),
                dispute(1, 2),
            ],
        );
        assert_eq!(row(&e, 1), expect("0.0000", "12.5000", "12.5000", false));
        steps(&mut e, &[resolve(1, 1)]);
        assert_eq!(row(&e, 1), expect("10.0000", "2.5000", "12.5000", false));
    }

    // --- Zero amounts (Z01-Z03) ---

    #[test]
    fn a_zero_deposit_creates_the_account_and_is_recorded() {
        let mut e = Engine::new();
        assert_eq!(step(&mut e, deposit(42, 7, "0.0")), APPLIED);
        assert_eq!(row(&e, 42), expect("0.0000", "0.0000", "0.0000", false));
        assert_eq!(
            step(&mut e, deposit(42, 7, "3")),
            ignored(DuplicateTransaction)
        );
        assert_eq!(row(&e, 42), expect("0.0000", "0.0000", "0.0000", false));
    }

    #[test]
    fn a_zero_deposit_goes_through_the_whole_dispute_lifecycle() {
        let mut e = Engine::new();
        steps(&mut e, &[deposit(40, 1, "0")]);
        for t in [
            dispute(40, 1),
            resolve(40, 1),
            dispute(40, 1),
            chargeback(40, 1),
        ] {
            assert_eq!(step(&mut e, t), APPLIED);
        }
        assert_eq!(row(&e, 40), expect("0.0000", "0.0000", "0.0000", true));
    }

    #[test]
    fn a_zero_withdrawal_uses_the_ordinary_sufficiency_rule() {
        let mut e = Engine::new();
        assert_eq!(step(&mut e, withdrawal(40, 1, "0.0000")), APPLIED);
        steps(
            &mut e,
            &[
                deposit(41, 4, "10"),
                withdrawal(41, 5, "10"),
                dispute(41, 4),
            ],
        );
        assert_eq!(row(&e, 41), expect("-10.0000", "10.0000", "0.0000", false));
        assert_eq!(
            step(&mut e, withdrawal(41, 6, "0")),
            ignored(InsufficientFunds)
        );
    }

    // --- Overflow and atomicity (T01-T03) ---

    #[test]
    fn a_deposit_that_would_overflow_changes_nothing() {
        let mut e = Engine::new();
        steps(&mut e, &[deposit(1, 1, "922337203685477.5807")]);
        let before = row(&e, 1);
        assert_eq!(step(&mut e, deposit(1, 2, "0.0001")), Err(Overflow));
        assert_eq!(row(&e, 1), before);
        assert!(!e.deposits.contains_key(&TxId(2)));
        assert_eq!(step(&mut e, dispute(1, 2)), ignored(UnknownTransaction));
    }

    #[test]
    fn a_dispute_that_would_overflow_held_changes_nothing() {
        let max = "922337203685477.5807";
        let mut e = Engine::new();
        steps(
            &mut e,
            &[
                deposit(1, 1, max),
                withdrawal(1, 2, max),
                deposit(1, 3, max),
                dispute(1, 1),
            ],
        );
        let before = row(&e, 1);
        assert_eq!(before, expect("0.0000", max, max, false));
        assert_eq!(step(&mut e, dispute(1, 3)), Err(Overflow));
        assert_eq!(row(&e, 1), before);
        assert_eq!(e.deposits[&TxId(3)].state, DisputeState::Undisputed);
    }

    #[test]
    fn ignored_operations_change_no_balance_lock_or_dispute_state() {
        // Builds one engine per reason, compares everything except a newly created zero account.
        let setup = [
            deposit(1, 1, "10"),
            deposit(1, 2, "5"),
            dispute(1, 2),
            deposit(2, 3, "1"),
            dispute(2, 3),
            chargeback(2, 3),
        ];
        let cases = [
            (withdrawal(1, 10, "100"), InsufficientFunds),
            (deposit(1, 1, "3"), DuplicateTransaction),
            (dispute(1, 99), UnknownTransaction),
            (resolve(3, 2), ClientMismatch),
            (dispute(1, 2), InvalidDisputeState),
            (deposit(2, 11, "1"), AccountLocked),
        ];
        for (t, reason) in cases {
            let mut e = Engine::new();
            steps(&mut e, &setup);
            let accounts_before = e.accounts.clone();
            let deposits_before = e.deposits.clone();
            assert_eq!(step(&mut e, t), ignored(reason));
            assert_eq!(e.deposits, deposits_before, "{reason:?}");
            for (id, account) in &e.accounts {
                match accounts_before.get(id) {
                    Some(old) => assert_eq!(account, old, "{reason:?}"),
                    None => assert_eq!(*account, Account::default(), "{reason:?}"),
                }
            }
        }
    }

    // --- Invariants (I03, I04) ---

    #[test]
    fn dispute_then_resolve_restores_the_exact_prior_state() {
        let mut e = Engine::new();
        steps(
            &mut e,
            &[
                deposit(1, 1, "3.3333"),
                deposit(1, 2, "1"),
                withdrawal(1, 3, "4"),
            ],
        );
        let before = row(&e, 1);
        assert_eq!(before.0, "0.3333");
        steps(&mut e, &[dispute(1, 1), resolve(1, 1)]);
        assert_eq!(row(&e, 1), before);
    }

    #[test]
    fn clients_are_isolated_from_each_other() {
        let x = [deposit(1, 1, "10"), withdrawal(1, 2, "3"), dispute(1, 1)];
        let y = [deposit(2, 3, "4"), dispute(2, 3), chargeback(2, 3)];
        let mut separate_x = Engine::new();
        steps(&mut separate_x, &x);
        let mut separate_y = Engine::new();
        steps(&mut separate_y, &y);
        let mut interleaved = Engine::new();
        steps(&mut interleaved, &[x[0], y[0], y[1], x[1], y[2], x[2]]);
        assert_eq!(row(&interleaved, 1), row(&separate_x, 1));
        assert_eq!(row(&interleaved, 2), row(&separate_y, 2));
    }
}
