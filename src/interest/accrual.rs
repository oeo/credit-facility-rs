use chrono::{DateTime, Datelike, NaiveDate, Utc};
use hourglass_rs::SafeTimeProvider;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::decimal::{Money, Rate};
use crate::errors::Result;
use crate::interest::{InterestCalculation, InterestCalculator};

/// day count convention for interest calculations
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DayCountConvention {
    /// actual days / 365
    Actual365,
    /// actual days / 360
    Actual360,
    /// 30 days per month / 360 days per year
    Thirty360,
    /// actual days / actual days in year (handles leap years)
    ActualActual,
}

/// engine for accruing interest
pub struct AccrualEngine {
    pub convention: DayCountConvention,
}

impl AccrualEngine {
    pub fn new(convention: DayCountConvention) -> Self {
        Self { convention }
    }
    
    /// calculate days between dates based on convention
    pub fn calculate_days(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> u32 {
        match self.convention {
            DayCountConvention::Actual365 | 
            DayCountConvention::Actual360 | 
            DayCountConvention::ActualActual => {
                (end - start).num_days() as u32
            }
            DayCountConvention::Thirty360 => {
                self.days_30_360(start.date_naive(), end.date_naive())
            }
        }
    }
    
    /// calculate 30/360 days between dates
    fn days_30_360(&self, start: NaiveDate, end: NaiveDate) -> u32 {
        let y1 = start.year();
        let y2 = end.year();
        let m1 = start.month() as i32;
        let m2 = end.month() as i32;
        let d1 = start.day().min(30) as i32;
        let d2 = if d1 == 30 { end.day().min(30) as i32 } else { end.day() as i32 };
        
        let days = 360 * (y2 - y1) + 30 * (m2 - m1) + (d2 - d1);
        days.max(0) as u32
    }
    
    /// get year basis for the convention
    pub fn year_basis(&self, year: i32) -> u32 {
        match self.convention {
            DayCountConvention::Actual365 => 365,
            DayCountConvention::Actual360 | DayCountConvention::Thirty360 => 360,
            DayCountConvention::ActualActual => {
                if is_leap_year(year) { 366 } else { 365 }
            }
        }
    }
    
    /// calculate simple interest (no compounding)
    pub fn calculate_simple_interest(
        &self,
        principal: Money,
        annual_rate: Rate,
        days: u32,
        year_basis: u32,
    ) -> Money {
        let daily_rate = annual_rate.as_decimal() / Decimal::from(year_basis);
        let interest = principal.as_decimal() * daily_rate * Decimal::from(days);
        Money::from_decimal(interest)
    }
    
    /// accrue daily interest with time provider
    pub fn accrue_daily(
        &self,
        principal: Money,
        annual_rate: Rate,
        last_accrual: DateTime<Utc>,
        time_provider: &SafeTimeProvider,
    ) -> Vec<DailyAccrual> {
        let mut accruals = Vec::new();
        let current_date = time_provider.now();
        let days = self.calculate_days(last_accrual, current_date);
        
        if days == 0 {
            return accruals;
        }
        
        let year_basis = self.year_basis(current_date.year());
        let daily_rate = annual_rate.as_decimal() / Decimal::from(year_basis);
        
        let mut accrual_date = last_accrual;
        for _ in 0..days {
            accrual_date = accrual_date + chrono::Duration::days(1);
            let interest = principal.as_decimal() * daily_rate;
            
            accruals.push(DailyAccrual {
                date: accrual_date,
                principal_base: principal,
                interest_amount: Money::from_decimal(interest),
                daily_rate: Rate::from_decimal(daily_rate),
            });
        }
        
        accruals
    }
    
    /// accrue monthly interest with time provider
    pub fn accrue_monthly(
        &self,
        principal: Money,
        annual_rate: Rate,
        last_accrual: DateTime<Utc>,
        time_provider: &SafeTimeProvider,
    ) -> Option<MonthlyAccrual> {
        let current_date = time_provider.now();
        
        // check if we've crossed a month boundary
        if !self.should_accrue_monthly(last_accrual, current_date) {
            return None;
        }
        
        let monthly_rate = annual_rate.as_decimal() / dec!(12);
        let interest = principal.as_decimal() * monthly_rate;
        
        Some(MonthlyAccrual {
            date: current_date,
            principal_base: principal,
            interest_amount: Money::from_decimal(interest),
            monthly_rate: Rate::from_decimal(monthly_rate),
        })
    }
    
    fn should_accrue_monthly(&self, last_accrual: DateTime<Utc>, current: DateTime<Utc>) -> bool {
        last_accrual.month() != current.month() || last_accrual.year() != current.year()
    }
}

impl InterestCalculator for AccrualEngine {
    fn calculate_interest(
        &self,
        principal: Money,
        rate: Rate,
        start_date: DateTime<Utc>,
        end_date: DateTime<Utc>,
    ) -> Result<InterestCalculation> {
        let days = self.calculate_days(start_date, end_date);
        let year_basis = self.year_basis(end_date.year());
        let daily_rate = self.get_daily_rate(rate);
        
        let interest = self.calculate_simple_interest(principal, rate, days, year_basis);
        
        Ok(InterestCalculation {
            interest_amount: interest,
            daily_rate,
            days,
            principal_base: principal,
            calculation_method: format!("{:?}", self.convention),
        })
    }
    
    fn get_daily_rate(&self, annual_rate: Rate) -> Rate {
        let year_basis = match self.convention {
            DayCountConvention::Actual365 => 365,
            DayCountConvention::Actual360 | DayCountConvention::Thirty360 => 360,
            DayCountConvention::ActualActual => 365, // default to 365
        };
        
        Rate::from_decimal(annual_rate.as_decimal() / Decimal::from(year_basis))
    }
}

/// daily accrual record
#[derive(Debug, Clone, PartialEq)]
pub struct DailyAccrual {
    pub date: DateTime<Utc>,
    pub principal_base: Money,
    pub interest_amount: Money,
    pub daily_rate: Rate,
}

/// monthly accrual record
#[derive(Debug, Clone, PartialEq)]
pub struct MonthlyAccrual {
    pub date: DateTime<Utc>,
    pub principal_base: Money,
    pub interest_amount: Money,
    pub monthly_rate: Rate,
}

/// check if year is a leap year
fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, TimeZone};
    use hourglass_rs::TimeSource;
    
    #[test]
    fn test_day_count_conventions() {
        let engine_365 = AccrualEngine::new(DayCountConvention::Actual365);
        let engine_360 = AccrualEngine::new(DayCountConvention::Actual360);
        let engine_30_360 = AccrualEngine::new(DayCountConvention::Thirty360);
        
        let start = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let end = Utc.with_ymd_and_hms(2024, 2, 1, 0, 0, 0).unwrap();
        
        assert_eq!(engine_365.calculate_days(start, end), 31);
        assert_eq!(engine_360.calculate_days(start, end), 31);
        assert_eq!(engine_30_360.calculate_days(start, end), 30);
    }
    
    #[test]
    fn test_30_360_convention() {
        let engine = AccrualEngine::new(DayCountConvention::Thirty360);
        
        let start = NaiveDate::from_ymd_opt(2024, 1, 31).unwrap();
        let end = NaiveDate::from_ymd_opt(2024, 2, 29).unwrap();
        
        assert_eq!(engine.days_30_360(start, end), 29);
        
        let start = NaiveDate::from_ymd_opt(2024, 2, 28).unwrap();
        let end = NaiveDate::from_ymd_opt(2024, 3, 31).unwrap();
        
        assert_eq!(engine.days_30_360(start, end), 33);
    }
    
    #[test]
    fn test_leap_year() {
        assert!(is_leap_year(2024));
        assert!(!is_leap_year(2023));
        assert!(is_leap_year(2000));
        assert!(!is_leap_year(1900));
    }
    
    #[test]
    fn test_simple_interest() {
        let engine = AccrualEngine::new(DayCountConvention::Actual365);
        let principal = Money::from_major(10_000);
        let rate = Rate::from_percentage(5);
        
        let interest = engine.calculate_simple_interest(principal, rate, 30, 365);
        
        let expected = Money::from_str_exact("41.10").unwrap();
        assert_eq!(interest.round_dp(2), expected);
    }
    
    #[test]
    fn test_daily_accrual_with_time_manipulation() {
        let engine = AccrualEngine::new(DayCountConvention::Actual365);
        let principal = Money::from_major(10_000);
        let rate = Rate::from_percentage(5);
        
        // create test time provider
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        let control = time.test_control().unwrap();
        
        let start = time.now();
        
        // advance 3 days
        control.advance(Duration::days(3));
        
        let accruals = engine.accrue_daily(principal, rate, start, &time);
        
        assert_eq!(accruals.len(), 3);
        
        let daily_interest = Money::from_str_exact("1.37").unwrap();
        for accrual in &accruals {
            assert_eq!(accrual.interest_amount.round_dp(2), daily_interest);
            assert_eq!(accrual.principal_base, principal);
        }
    }
    
    #[test]
    fn test_monthly_accrual_with_time_manipulation() {
        let engine = AccrualEngine::new(DayCountConvention::Actual365);
        let principal = Money::from_major(100_000);
        let rate = Rate::from_percentage(5);
        
        // start on january 15th
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 15, 0, 0, 0).unwrap()
        ));
        let control = time.test_control().unwrap();
        
        let last_accrual = time.now();
        
        // advance within same month - no accrual
        control.advance(Duration::days(10));
        let result = engine.accrue_monthly(principal, rate, last_accrual, &time);
        assert!(result.is_none());
        
        // advance to next month - should accrue
        control.advance(Duration::days(10)); // now in february
        let result = engine.accrue_monthly(principal, rate, last_accrual, &time);
        assert!(result.is_some());
        
        let accrual = result.unwrap();
        let expected_interest = Money::from_str_exact("416.67").unwrap();
        assert_eq!(accrual.interest_amount.round_dp(2), expected_interest);
    }
    
    #[test]
    fn test_leap_year_handling() {
        let engine = AccrualEngine::new(DayCountConvention::ActualActual);
        let principal = Money::from_major(10_000);
        let rate = Rate::from_percentage(5);
        
        // test leap year (2024)
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 2, 28, 0, 0, 0).unwrap()
        ));
        let control = time.test_control().unwrap();
        
        let start = time.now();
        
        // advance through feb 29
        control.advance(Duration::days(1));
        
        let accruals = engine.accrue_daily(principal, rate, start, &time);
        assert_eq!(accruals.len(), 1);
        
        // daily rate should use 366 days for leap year
        let year_basis = engine.year_basis(2024);
        assert_eq!(year_basis, 366);
        
        let expected_daily_rate = Rate::from_decimal(rate.as_decimal() / dec!(366));
        assert_eq!(accruals[0].daily_rate, expected_daily_rate);
    }
    
    #[test]
    fn test_year_end_accrual() {
        let engine = AccrualEngine::new(DayCountConvention::Actual365);
        let principal = Money::from_major(10_000);
        let rate = Rate::from_percentage(5);
        
        // start on december 30th
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2023, 12, 30, 0, 0, 0).unwrap()
        ));
        let control = time.test_control().unwrap();
        
        let start = time.now();
        
        // advance through year boundary
        control.advance(Duration::days(3)); // now january 2nd
        
        let accruals = engine.accrue_daily(principal, rate, start, &time);
        assert_eq!(accruals.len(), 3);
        
        // verify dates cross year boundary
        assert_eq!(accruals[0].date.year(), 2023);
        assert_eq!(accruals[0].date.day(), 31);
        assert_eq!(accruals[1].date.year(), 2024);
        assert_eq!(accruals[1].date.day(), 1);
        assert_eq!(accruals[2].date.year(), 2024);
        assert_eq!(accruals[2].date.day(), 2);
    }
}