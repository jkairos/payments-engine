//! Fixed-point money with four decimal places.

use std::fmt;
use std::str::FromStr;

/// Number of fractional digits carried by every amount.
const SCALE_DIGITS: usize = 4;
/// `10^SCALE_DIGITS`: the number of units in one whole currency unit.
const SCALE: i64 = 10_000;

/// A signed amount of money, stored as an integer count of ten-thousandths.
///
/// Balances use this type because they can become negative (see the README).
/// Every arithmetic operation is checked; nothing ever wraps or rounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub struct Money(i64);

impl Money {
    pub const ZERO: Money = Money(0);
    pub const MAX: Money = Money(i64::MAX);
    pub const MIN: Money = Money(i64::MIN);

    /// Builds a value from a raw count of ten-thousandths.
    pub const fn from_units(units: i64) -> Money {
        Money(units)
    }

    pub fn checked_add(self, other: Money) -> Option<Money> {
        self.0.checked_add(other.0).map(Money)
    }

    pub fn checked_sub(self, other: Money) -> Option<Money> {
        self.0.checked_sub(other.0).map(Money)
    }
}

impl fmt::Display for Money {
    /// Always renders exactly four decimals, e.g. `-0.5000`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `unsigned_abs` keeps `i64::MIN` representable.
        let sign = if self.0 < 0 { "-" } else { "" };
        let magnitude = self.0.unsigned_abs();
        let scale = SCALE as u64;
        write!(
            f,
            "{sign}{}.{:0width$}",
            magnitude / scale,
            magnitude % scale,
            width = SCALE_DIGITS
        )
    }
}

/// A transaction amount: never negative, zero allowed.
///
/// The only way to obtain one is parsing, so the engine never sees a
/// negative, rounded or out-of-range amount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Amount(Money);

impl Amount {
    pub fn money(self) -> Money {
        self.0
    }
}

/// Why a string is not a valid [`Amount`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmountError {
    /// Not of the form `digits[.digits]` (covers empty, signs, exponents).
    Syntax,
    /// More than four fractional digits, and the extra digits are not all zero.
    TooPrecise,
    /// Larger than the largest representable balance.
    OutOfRange,
}

impl fmt::Display for AmountError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = match self {
            AmountError::Syntax => "not an unsigned decimal number",
            AmountError::TooPrecise => "more than four decimal places",
            AmountError::OutOfRange => "amount too large",
        };
        f.write_str(text)
    }
}

impl std::error::Error for AmountError {}

impl FromStr for Amount {
    type Err = AmountError;

    fn from_str(s: &str) -> Result<Amount, AmountError> {
        let (whole, fraction) = s.split_once('.').unwrap_or((s, ""));
        let all_digits = |part: &str| part.bytes().all(|b| b.is_ascii_digit());
        // Checking the characters ourselves matters: `str::parse::<i64>` would accept a sign.
        if whole.len() + fraction.len() == 0 || !all_digits(whole) || !all_digits(fraction) {
            return Err(AmountError::Syntax);
        }

        // Digits beyond the fourth are allowed only if they are zeros (exact value, no rounding).
        let (kept, extra) = fraction.split_at(fraction.len().min(SCALE_DIGITS));
        if extra.bytes().any(|b| b != b'0') {
            return Err(AmountError::TooPrecise);
        }

        let units = accumulate(whole.bytes().chain(kept.bytes()))
            .and_then(|n| n.checked_mul(10_i64.pow((SCALE_DIGITS - kept.len()) as u32)))
            .ok_or(AmountError::OutOfRange)?;
        Ok(Amount(Money(units)))
    }
}

/// Folds ASCII digits into an integer, returning `None` on overflow.
fn accumulate(mut digits: impl Iterator<Item = u8>) -> Option<i64> {
    digits.try_fold(0_i64, |acc, d| {
        acc.checked_mul(10)?.checked_add(i64::from(d - b'0'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(s: &str) -> Result<String, AmountError> {
        s.parse::<Amount>().map(|a| a.money().to_string())
    }

    #[test]
    fn parses_every_precision_from_zero_to_four_places() {
        assert_eq!(parse("1"), Ok("1.0000".into()));
        assert_eq!(parse("2.5"), Ok("2.5000".into()));
        assert_eq!(parse("3.25"), Ok("3.2500".into()));
        assert_eq!(parse("4.125"), Ok("4.1250".into()));
        assert_eq!(parse("5.0625"), Ok("5.0625".into()));
        assert_eq!(parse("0.0001"), Ok("0.0001".into()));
        assert_eq!(
            parse("922337203685477.5807"),
            Ok("922337203685477.5807".into())
        );
    }

    #[test]
    fn parses_zero() {
        assert_eq!(parse("0"), Ok("0.0000".into()));
        assert_eq!(parse("0.0"), Ok("0.0000".into()));
        assert_eq!(parse("0.0000"), Ok("0.0000".into()));
    }

    #[test]
    fn parses_lenient_exact_forms() {
        assert_eq!(parse("5."), Ok("5.0000".into()));
        assert_eq!(parse(".5"), Ok("0.5000".into()));
        assert_eq!(parse("007.50"), Ok("7.5000".into()));
        assert_eq!(parse("2.50000"), Ok("2.5000".into()));
    }

    #[test]
    fn rejects_malformed_text() {
        for s in [
            "", ".", "-1", "-0", "+1", "1e3", "1,5", "1 0", "NaN", "1.2.3", " 1", "١",
        ] {
            assert_eq!(parse(s), Err(AmountError::Syntax), "input {s:?}");
        }
    }

    #[test]
    fn rejects_inexact_values() {
        assert_eq!(parse("1.00001"), Err(AmountError::TooPrecise));
        assert_eq!(parse("0.00005"), Err(AmountError::TooPrecise));
    }

    #[test]
    fn rejects_values_out_of_range() {
        assert_eq!(parse("922337203685477.5808"), Err(AmountError::OutOfRange));
        assert_eq!(
            parse("9999999999999999999999999999999999999999"),
            Err(AmountError::OutOfRange)
        );
    }

    #[test]
    fn renders_exactly_four_decimals_with_correct_sign() {
        assert_eq!(Money::ZERO.to_string(), "0.0000");
        assert_eq!(Money::from_units(1).to_string(), "0.0001");
        assert_eq!(Money::from_units(15_000).to_string(), "1.5000");
        assert_eq!(Money::from_units(-5_000).to_string(), "-0.5000");
        assert_eq!(Money::from_units(-1).to_string(), "-0.0001");
        assert_eq!(Money::MAX.to_string(), "922337203685477.5807");
        assert_eq!(Money::MIN.to_string(), "-922337203685477.5808");
    }

    #[test]
    fn repeated_small_additions_are_exact() {
        let unit = Money::from_units(1);
        let mut sum = Money::ZERO;
        for _ in 0..10_000 {
            sum = sum.checked_add(unit).unwrap();
        }
        assert_eq!(sum.to_string(), "1.0000");
    }

    #[test]
    fn reports_overflow_at_both_ends() {
        let unit = Money::from_units(1);
        assert_eq!(Money::MAX.checked_add(unit), None);
        assert_eq!(Money::MIN.checked_sub(unit), None);
        assert_eq!(
            Money::MAX.checked_sub(unit),
            Some(Money::from_units(i64::MAX - 1))
        );
    }
}
