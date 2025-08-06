use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::decimal::{Money, Rate};
use crate::errors::Result;
use crate::interest::{InterestCalculation, InterestCalculator};

/// compounding frequency
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CompoundingFrequency {
    Daily,
    Weekly,
    Monthly,
    Quarterly,
    SemiAnnual,
    Annual,
    Continuous,
}

impl CompoundingFrequency {
    /// get number of compounding periods per year
    pub fn periods_per_year(&self) -> u32 {
        match self {
            CompoundingFrequency::Daily => 365,
            CompoundingFrequency::Weekly => 52,
            CompoundingFrequency::Monthly => 12,
            CompoundingFrequency::Quarterly => 4,
            CompoundingFrequency::SemiAnnual => 2,
            CompoundingFrequency::Annual => 1,
            CompoundingFrequency::Continuous => 0, // special case
        }
    }
}

/// engine for compound interest calculations
pub struct CompoundingEngine {
    pub frequency: CompoundingFrequency,
}

impl CompoundingEngine {
    pub fn new(frequency: CompoundingFrequency) -> Self {
        Self { frequency }
    }
    
    /// calculate compound interest
    pub fn calculate_compound(
        &self,
        principal: Money,
        annual_rate: Rate,
        time_years: Decimal,
    ) -> Money {
        match self.frequency {
            CompoundingFrequency::Continuous => {
                self.calculate_continuous(principal, annual_rate, time_years)
            }
            _ => {
                self.calculate_discrete(principal, annual_rate, time_years)
            }
        }
    }
    
    /// calculate discrete compound interest
    fn calculate_discrete(
        &self,
        principal: Money,
        annual_rate: Rate,
        time_years: Decimal,
    ) -> Money {
        let n = Decimal::from(self.frequency.periods_per_year());
        let rate_decimal = annual_rate.as_decimal();
        
        let periods = (n * time_years).round();
        let period_rate = rate_decimal / n;
        
        // calculate (1 + r)^n using iteration
        let mut compound_factor = Decimal::ONE;
        let base = Decimal::ONE + period_rate;
        let periods_int = periods.to_string().parse::<i32>().unwrap_or(0);
        for _ in 0..periods_int {
            compound_factor *= base;
        }
        let final_amount = principal.as_decimal() * compound_factor;
        
        Money::from_decimal(final_amount - principal.as_decimal())
    }
    
    /// calculate continuous compound interest (e^rt)
    fn calculate_continuous(
        &self,
        principal: Money,
        annual_rate: Rate,
        time_years: Decimal,
    ) -> Money {
        // use approximation for e^x = 1 + x + x^2/2! + x^3/3! + ...
        let rate_decimal = annual_rate.as_decimal();
        let x = rate_decimal * time_years;
        
        // taylor series approximation for e^x (first 10 terms for accuracy)
        let mut compound_factor = Decimal::ONE;
        let mut term = Decimal::ONE;
        for i in 1..10 {
            term = term * x / Decimal::from(i);
            compound_factor = compound_factor + term;
        }
        
        let final_amount = principal.as_decimal() * compound_factor;
        Money::from_decimal(final_amount - principal.as_decimal())
    }
    
    /// calculate compound interest for a specific number of days
    pub fn compound_for_days(
        &self,
        principal: Money,
        annual_rate: Rate,
        days: u32,
    ) -> Money {
        let time_years = Decimal::from(days) / dec!(365);
        self.calculate_compound(principal, annual_rate, time_years)
    }
    
    /// calculate monthly compound interest
    pub fn compound_monthly(
        &self,
        principal: Money,
        annual_rate: Rate,
        months: u32,
    ) -> Money {
        let monthly_rate = annual_rate.as_decimal() / dec!(12);
        // calculate (1 + r)^n using iteration
        let mut compound_factor = Decimal::ONE;
        let base = Decimal::ONE + monthly_rate;
        for _ in 0..months {
            compound_factor *= base;
        }
        let final_amount = principal.as_decimal() * compound_factor;
        
        Money::from_decimal(final_amount - principal.as_decimal())
    }
    
    /// calculate daily compound interest
    pub fn compound_daily(
        &self,
        principal: Money,
        annual_rate: Rate,
        days: u32,
    ) -> Money {
        let daily_rate = annual_rate.as_decimal() / dec!(365);
        // calculate (1 + r)^n using iteration
        let mut compound_factor = Decimal::ONE;
        let base = Decimal::ONE + daily_rate;
        for _ in 0..days {
            compound_factor *= base;
        }
        let final_amount = principal.as_decimal() * compound_factor;
        
        Money::from_decimal(final_amount - principal.as_decimal())
    }
}

impl InterestCalculator for CompoundingEngine {
    fn calculate_interest(
        &self,
        principal: Money,
        rate: Rate,
        start_date: DateTime<Utc>,
        end_date: DateTime<Utc>,
    ) -> Result<InterestCalculation> {
        let days = (end_date - start_date).num_days() as u32;
        let time_years = Decimal::from(days) / dec!(365);
        
        let interest = self.calculate_compound(principal, rate, time_years);
        let daily_rate = self.get_daily_rate(rate);
        
        Ok(InterestCalculation {
            interest_amount: interest,
            daily_rate,
            days,
            principal_base: principal,
            calculation_method: format!("{:?} compounding", self.frequency),
        })
    }
    
    fn get_daily_rate(&self, annual_rate: Rate) -> Rate {
        match self.frequency {
            CompoundingFrequency::Daily => {
                Rate::from_decimal(annual_rate.as_decimal() / dec!(365))
            }
            CompoundingFrequency::Continuous => {
                // for continuous compounding, effective daily rate
                // approximation: r_daily ≈ r_annual / 365
                Rate::from_decimal(annual_rate.as_decimal() / dec!(365))
            }
            _ => {
                // approximate daily rate from annual rate
                // using simple division as approximation
                Rate::from_decimal(annual_rate.as_decimal() / dec!(365))
            }
        }
    }
}

/// calculate the future value with compound interest
pub fn future_value(
    present_value: Money,
    annual_rate: Rate,
    years: Decimal,
    frequency: CompoundingFrequency,
) -> Money {
    let engine = CompoundingEngine::new(frequency);
    let interest = engine.calculate_compound(present_value, annual_rate, years);
    present_value + interest
}

/// calculate the present value given future value
pub fn present_value(
    future_value: Money,
    annual_rate: Rate,
    years: Decimal,
    frequency: CompoundingFrequency,
) -> Money {
    match frequency {
        CompoundingFrequency::Continuous => {
            // use taylor series for e^(-x)
            let x = annual_rate.as_decimal() * years;
            let mut discount_factor = Decimal::ONE;
            let mut term = Decimal::ONE;
            for i in 1..10 {
                term = term * (-x) / Decimal::from(i);
                discount_factor = discount_factor + term;
            }
            Money::from_decimal(future_value.as_decimal() * discount_factor)
        }
        _ => {
            let n = Decimal::from(frequency.periods_per_year());
            let periods = n * years;
            let period_rate = annual_rate.as_decimal() / n;
            // calculate discount factor (1 + r)^(-n) = 1 / (1 + r)^n
            let mut compound_factor = Decimal::ONE;
            let base = Decimal::ONE + period_rate;
            let periods_int = periods.to_string().parse::<i32>().unwrap_or(0);
            for _ in 0..periods_int {
                compound_factor *= base;
            }
            let discount_factor = Decimal::ONE / compound_factor;
            Money::from_decimal(future_value.as_decimal() * discount_factor)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_monthly_compounding() {
        let engine = CompoundingEngine::new(CompoundingFrequency::Monthly);
        let principal = Money::from_major(10_000);
        let rate = Rate::from_percentage(12);
        
        let interest = engine.compound_monthly(principal, rate, 12);
        
        let expected = Money::from_str_exact("1268.25").unwrap();
        assert_eq!(interest.round_dp(2), expected);
    }
    
    #[test]
    fn test_daily_compounding() {
        let engine = CompoundingEngine::new(CompoundingFrequency::Daily);
        let principal = Money::from_major(10_000);
        let rate = Rate::from_percentage(18);
        
        let interest = engine.compound_daily(principal, rate, 30);
        
        // daily compounding for 30 days at 18% APR
        // the actual calculation may vary slightly
        assert!(interest > Money::from_major(148));
        assert!(interest < Money::from_major(150));
    }
    
    #[test]
    fn test_continuous_compounding() {
        let engine = CompoundingEngine::new(CompoundingFrequency::Continuous);
        let principal = Money::from_major(10_000);
        let rate = Rate::from_percentage(12);
        
        let interest = engine.calculate_compound(principal, rate, Decimal::ONE);
        
        // continuous compounding should be around 12.75% for 12% APR
        assert!(interest > Money::from_major(1270));
        assert!(interest < Money::from_major(1280));
    }
    
    #[test]
    fn test_compounding_comparison() {
        let principal = Money::from_major(10_000);
        let rate = Rate::from_percentage(12);
        let time = Decimal::ONE;
        
        let annual = CompoundingEngine::new(CompoundingFrequency::Annual)
            .calculate_compound(principal, rate, time);
        let monthly = CompoundingEngine::new(CompoundingFrequency::Monthly)
            .calculate_compound(principal, rate, time);
        let daily = CompoundingEngine::new(CompoundingFrequency::Daily)
            .calculate_compound(principal, rate, time);
        let continuous = CompoundingEngine::new(CompoundingFrequency::Continuous)
            .calculate_compound(principal, rate, time);
        
        // verify compounding order: annual < monthly < daily < continuous
        assert!(annual < monthly);
        assert!(monthly < daily);
        // continuous should be close to daily for practical purposes
        assert!(continuous >= daily);
        
        // verify expected ranges
        assert_eq!(annual, Money::from_major(1200));
        assert!(monthly > Money::from_major(1260));
        assert!(daily > Money::from_major(1270));
    }
    
    #[test]
    fn test_future_value() {
        let pv = Money::from_major(10_000);
        let rate = Rate::from_percentage(5);
        let years = dec!(3);
        
        let fv = future_value(pv, rate, years, CompoundingFrequency::Annual);
        
        let expected = Money::from_str_exact("11576.25").unwrap();
        assert_eq!(fv.round_dp(2), expected);
    }
    
    #[test]
    fn test_present_value() {
        let fv = Money::from_major(11_576);
        let rate = Rate::from_percentage(5);
        let years = dec!(3);
        
        let pv = present_value(fv, rate, years, CompoundingFrequency::Annual);
        
        assert!(pv < Money::from_major(10_001));
        assert!(pv > Money::from_major(9_999));
    }
}