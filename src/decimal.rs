use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{Add, AddAssign, Div, Mul, Sub, SubAssign};
use std::str::FromStr;

/// Money type with 8 decimal places precision for satoshi-level accuracy
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub struct Money(Decimal);

impl Money {
    pub const ZERO: Money = Money(Decimal::ZERO);
    pub const ONE: Money = Money(Decimal::ONE);
    pub const SATOSHI: Money = Money(Decimal::from_parts(1, 0, 0, false, 8));
    
    /// create from decimal
    pub fn from_decimal(d: Decimal) -> Self {
        Money(d.round_dp(8))
    }
    
    /// create from string with exact parsing
    pub fn from_str_exact(s: &str) -> Result<Self, rust_decimal::Error> {
        Ok(Money(Decimal::from_str(s)?.round_dp(8)))
    }
    
    /// create from integer amount (dollars, euros, etc)
    pub fn from_major(amount: i64) -> Self {
        Money(Decimal::from(amount))
    }
    
    /// create from minor amount (cents, satoshis, etc)
    pub fn from_minor(amount: i64, scale: u32) -> Self {
        let d = Decimal::from(amount) / Decimal::from(10_u64.pow(scale));
        Money(d.round_dp(8))
    }
    
    /// get underlying decimal
    pub fn as_decimal(&self) -> Decimal {
        self.0
    }
    
    /// round to specified decimal places
    pub fn round_dp(&self, dp: u32) -> Self {
        Money(self.0.round_dp(dp))
    }
    
    /// check if zero
    pub fn is_zero(&self) -> bool {
        self.0.is_zero()
    }
    
    /// check if positive
    pub fn is_positive(&self) -> bool {
        self.0.is_sign_positive()
    }
    
    /// check if negative
    pub fn is_negative(&self) -> bool {
        self.0.is_sign_negative()
    }
    
    /// absolute value
    pub fn abs(&self) -> Self {
        Money(self.0.abs())
    }
    
    /// minimum of two values
    pub fn min(self, other: Self) -> Self {
        Money(self.0.min(other.0))
    }
    
    /// maximum of two values
    pub fn max(self, other: Self) -> Self {
        Money(self.0.max(other.0))
    }
    
    /// calculate percentage (e.g., 5% of $100)
    pub fn percentage(&self, rate: Decimal) -> Self {
        Money((self.0 * rate / Decimal::from(100)).round_dp(8))
    }
    
    /// apply annual rate for given days
    pub fn apply_rate(&self, annual_rate: Decimal, days: u32) -> Self {
        let daily_rate = annual_rate / Decimal::from(365);
        let interest = self.0 * daily_rate * Decimal::from(days);
        Money(interest.round_dp(8))
    }
    
    /// compound interest calculation
    pub fn compound(&self, rate: Decimal, periods: u32) -> Self {
        let mut factor = Decimal::ONE;
        for _ in 0..periods {
            factor = factor * (Decimal::ONE + rate);
        }
        Money((self.0 * factor).round_dp(8))
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl FromStr for Money {
    type Err = rust_decimal::Error;
    
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Money::from_str_exact(s)
    }
}

impl From<Decimal> for Money {
    fn from(d: Decimal) -> Self {
        Money::from_decimal(d)
    }
}

impl From<i32> for Money {
    fn from(i: i32) -> Self {
        Money::from_major(i as i64)
    }
}

impl From<u32> for Money {
    fn from(i: u32) -> Self {
        Money::from_major(i as i64)
    }
}

impl Add for Money {
    type Output = Money;
    
    fn add(self, other: Money) -> Money {
        Money((self.0 + other.0).round_dp(8))
    }
}

impl AddAssign for Money {
    fn add_assign(&mut self, other: Money) {
        self.0 = (self.0 + other.0).round_dp(8);
    }
}

impl Sub for Money {
    type Output = Money;
    
    fn sub(self, other: Money) -> Money {
        Money((self.0 - other.0).round_dp(8))
    }
}

impl SubAssign for Money {
    fn sub_assign(&mut self, other: Money) {
        self.0 = (self.0 - other.0).round_dp(8);
    }
}

impl Mul<Decimal> for Money {
    type Output = Money;
    
    fn mul(self, other: Decimal) -> Money {
        Money((self.0 * other).round_dp(8))
    }
}

impl Div<Decimal> for Money {
    type Output = Money;
    
    fn div(self, other: Decimal) -> Money {
        Money((self.0 / other).round_dp(8))
    }
}

/// rate type for interest rates, percentages, and ratios
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub struct Rate(Decimal);

impl Rate {
    pub const ZERO: Rate = Rate(Decimal::ZERO);
    pub const ONE: Rate = Rate(Decimal::ONE);
    
    /// create from decimal (e.g., 0.05 for 5%)
    pub fn from_decimal(d: Decimal) -> Self {
        Rate(d)
    }
    
    /// create from percentage (e.g., 5 for 5%)
    pub fn from_percentage(p: u32) -> Self {
        Rate(Decimal::from(p) / Decimal::from(100))
    }
    
    /// create from basis points (e.g., 500 for 5%)
    pub fn from_bps(bps: u32) -> Self {
        Rate(Decimal::from(bps) / Decimal::from(10000))
    }
    
    /// get as decimal
    pub fn as_decimal(&self) -> Decimal {
        self.0
    }
    
    /// get as percentage
    pub fn as_percentage(&self) -> Decimal {
        self.0 * Decimal::from(100)
    }
    
    /// get as basis points
    pub fn as_bps(&self) -> Decimal {
        self.0 * Decimal::from(10000)
    }
    
    /// daily rate from annual rate
    pub fn daily_rate(&self) -> Rate {
        Rate(self.0 / Decimal::from(365))
    }
    
    /// monthly rate from annual rate
    pub fn monthly_rate(&self) -> Rate {
        Rate(self.0 / Decimal::from(12))
    }
}

impl fmt::Display for Rate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}%", self.as_percentage())
    }
}

impl From<Decimal> for Rate {
    fn from(d: Decimal) -> Self {
        Rate::from_decimal(d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_money_precision() {
        let m = Money::from_str_exact("100.123456789").unwrap();
        assert_eq!(m.to_string(), "100.12345679"); // rounded to 8 places
    }
    
    #[test]
    fn test_satoshi_precision() {
        let btc = Money::from_minor(100_000_000, 8); // 1 BTC in satoshis
        assert_eq!(btc, Money::from_major(1));
        
        let sat = Money::from_minor(1, 8); // 1 satoshi
        assert_eq!(sat, Money::SATOSHI);
    }
    
    #[test]
    fn test_interest_calculation() {
        let principal = Money::from_major(10_000);
        let rate = Rate::from_percentage(5); // 5% annual
        
        let daily_interest = principal.apply_rate(rate.as_decimal(), 1);
        assert_eq!(daily_interest.round_dp(2).to_string(), "1.37");
        
        let annual_interest = principal.apply_rate(rate.as_decimal(), 365);
        assert_eq!(annual_interest.round_dp(2).to_string(), "500.00");
    }
    
    #[test]
    fn test_compound_interest() {
        let principal = Money::from_major(1_000);
        let monthly_rate = Rate::from_percentage(12).monthly_rate();
        
        // compound monthly for 12 months
        let final_amount = principal.compound(monthly_rate.as_decimal(), 12);
        assert!(final_amount > principal);
    }
}