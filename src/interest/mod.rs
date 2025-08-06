pub mod accrual;
pub mod compound;
pub mod penalty;

use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

use crate::decimal::{Money, Rate};
use crate::errors::Result;

pub use accrual::{AccrualEngine, DailyAccrual, DayCountConvention, MonthlyAccrual};
pub use compound::{CompoundingEngine, CompoundingFrequency};
pub use penalty::{PenaltyConfig, PenaltyEngine};

/// interest calculation result
#[derive(Debug, Clone, PartialEq)]
pub struct InterestCalculation {
    pub interest_amount: Money,
    pub daily_rate: Rate,
    pub days: u32,
    pub principal_base: Money,
    pub calculation_method: String,
}

/// interest capitalization event
#[derive(Debug, Clone, PartialEq)]
pub struct CapitalizationResult {
    pub amount_capitalized: Money,
    pub new_principal: Money,
    pub reason: String,
    pub timestamp: DateTime<Utc>,
}

/// trait for interest calculations
pub trait InterestCalculator {
    fn calculate_interest(
        &self,
        principal: Money,
        rate: Rate,
        start_date: DateTime<Utc>,
        end_date: DateTime<Utc>,
    ) -> Result<InterestCalculation>;
    
    fn get_daily_rate(&self, annual_rate: Rate) -> Rate;
}

/// capitalize accrued interest into principal
pub fn capitalize_interest(
    principal: Money,
    accrued_interest: Money,
    reason: &str,
    timestamp: DateTime<Utc>,
) -> CapitalizationResult {
    CapitalizationResult {
        amount_capitalized: accrued_interest,
        new_principal: principal + accrued_interest,
        reason: reason.to_string(),
        timestamp,
    }
}

/// calculate effective annual percentage yield (APY)
pub fn calculate_apy(apr: Rate, compounding_frequency: u32) -> Rate {
    let n = Decimal::from(compounding_frequency);
    let apr_decimal = apr.as_decimal();
    
    // calculate (1 + r/n)^n - 1 using iteration
    let mut compound_factor = Decimal::ONE;
    let base = Decimal::ONE + apr_decimal / n;
    for _ in 0..compounding_frequency {
        compound_factor *= base;
    }
    let apy = compound_factor - Decimal::ONE;
    Rate::from_decimal(apy)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    
    #[test]
    fn test_apy_calculation() {
        let apr = Rate::from_percentage(18);
        
        let daily_apy = calculate_apy(apr, 365);
        assert!(daily_apy.as_percentage() > dec!(19.7));
        assert!(daily_apy.as_percentage() < dec!(19.8));
        
        let monthly_apy = calculate_apy(apr, 12);
        assert!(monthly_apy.as_percentage() > dec!(19.5));
        assert!(monthly_apy.as_percentage() < dec!(19.6));
    }
    
    #[test]
    fn test_capitalization() {
        let principal = Money::from_major(10_000);
        let interest = Money::from_major(500);
        let now = Utc::now();
        
        let result = capitalize_interest(principal, interest, "Monthly capitalization", now);
        
        assert_eq!(result.amount_capitalized, interest);
        assert_eq!(result.new_principal, Money::from_major(10_500));
        assert_eq!(result.reason, "Monthly capitalization");
    }
}