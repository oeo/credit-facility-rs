use chrono::{DateTime, Datelike, Duration, Utc};
use hourglass_rs::SafeTimeProvider;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::decimal::{Money, Rate};
use crate::errors::Result;
use crate::types::AmortizationMethod;

/// scheduled payment in amortization schedule
#[derive(Debug, Clone, PartialEq)]
pub struct ScheduledPayment {
    pub payment_number: u32,
    pub payment_date: DateTime<Utc>,
    pub beginning_balance: Money,
    pub payment_amount: Money,
    pub principal_portion: Money,
    pub interest_portion: Money,
    pub ending_balance: Money,
    pub cumulative_interest: Money,
    pub cumulative_principal: Money,
}

/// amortization schedule
#[derive(Debug, Clone)]
pub struct AmortizationSchedule {
    pub facility_id: uuid::Uuid,
    pub principal: Money,
    pub interest_rate: Rate,
    pub term_months: u32,
    pub start_date: DateTime<Utc>,
    pub amortization_method: AmortizationMethod,
    pub payments: Vec<ScheduledPayment>,
    pub total_interest: Money,
    pub total_payment: Money,
}

impl AmortizationSchedule {
    /// generate payment schedule
    pub fn generate(
        facility_id: uuid::Uuid,
        principal: Money,
        interest_rate: Rate,
        term_months: u32,
        start_date: DateTime<Utc>,
        amortization_method: AmortizationMethod,
        time_provider: &SafeTimeProvider,
    ) -> Result<Self> {
        let calculator = AmortizationCalculator::new(amortization_method);
        
        let payments = calculator.calculate_schedule(
            principal,
            interest_rate,
            term_months,
            start_date,
            time_provider,
        )?;
        
        let total_interest = payments
            .iter()
            .map(|p| p.interest_portion)
            .fold(Money::ZERO, |acc, x| acc + x);
        
        let total_payment = payments
            .iter()
            .map(|p| p.payment_amount)
            .fold(Money::ZERO, |acc, x| acc + x);
        
        Ok(Self {
            facility_id,
            principal,
            interest_rate,
            term_months,
            start_date,
            amortization_method,
            payments,
            total_interest,
            total_payment,
        })
    }
    
    /// get payment for specific period
    pub fn get_payment(&self, payment_number: u32) -> Option<&ScheduledPayment> {
        self.payments.get((payment_number - 1) as usize)
    }
    
    /// get remaining balance after payment
    pub fn balance_after_payment(&self, payment_number: u32) -> Money {
        self.get_payment(payment_number)
            .map(|p| p.ending_balance)
            .unwrap_or(self.principal)
    }
    
    /// recalculate schedule after prepayment
    pub fn recalculate_after_prepayment(
        &mut self,
        prepayment_amount: Money,
        after_payment_number: u32,
        strategy: RecalculationStrategy,
        time_provider: &SafeTimeProvider,
    ) -> Result<()> {
        let remaining_balance = self.balance_after_payment(after_payment_number) - prepayment_amount;
        let remaining_months = self.term_months - after_payment_number;
        
        match strategy {
            RecalculationStrategy::ReduceEmi => {
                // recalculate with same term, lower EMI
                let calculator = AmortizationCalculator::new(self.amortization_method);
                let new_payments = calculator.calculate_schedule(
                    remaining_balance,
                    self.interest_rate,
                    remaining_months,
                    self.payments[after_payment_number as usize].payment_date,
                    time_provider,
                )?;
                
                // replace future payments
                self.payments.truncate(after_payment_number as usize);
                self.payments.extend(new_payments);
            }
            RecalculationStrategy::ReduceTerm => {
                // keep EMI same, reduce term
                let current_emi = self.payments[0].payment_amount;
                let new_term = calculate_term_for_payment(
                    remaining_balance,
                    self.interest_rate,
                    current_emi,
                );
                
                let calculator = AmortizationCalculator::new(self.amortization_method);
                let new_payments = calculator.calculate_schedule(
                    remaining_balance,
                    self.interest_rate,
                    new_term,
                    self.payments[after_payment_number as usize].payment_date,
                    time_provider,
                )?;
                
                self.payments.truncate(after_payment_number as usize);
                self.payments.extend(new_payments);
                self.term_months = after_payment_number + new_term;
            }
        }
        
        // recalculate totals
        self.total_interest = self.payments
            .iter()
            .map(|p| p.interest_portion)
            .fold(Money::ZERO, |acc, x| acc + x);
        
        self.total_payment = self.payments
            .iter()
            .map(|p| p.payment_amount)
            .fold(Money::ZERO, |acc, x| acc + x);
        
        Ok(())
    }
}

/// amortization calculator
pub struct AmortizationCalculator {
    method: AmortizationMethod,
}

impl AmortizationCalculator {
    pub fn new(method: AmortizationMethod) -> Self {
        Self { method }
    }
    
    /// calculate full amortization schedule
    pub fn calculate_schedule(
        &self,
        principal: Money,
        annual_rate: Rate,
        term_months: u32,
        start_date: DateTime<Utc>,
        _time_provider: &SafeTimeProvider,
    ) -> Result<Vec<ScheduledPayment>> {
        match self.method {
            AmortizationMethod::EqualInstallments => {
                self.calculate_equal_installments(principal, annual_rate, term_months, start_date)
            }
            AmortizationMethod::DecliningPrincipal => {
                self.calculate_declining_principal(principal, annual_rate, term_months, start_date)
            }
            AmortizationMethod::InterestOnly => {
                self.calculate_interest_only(principal, annual_rate, term_months, start_date)
            }
        }
    }
    
    /// equal installments (EMI)
    fn calculate_equal_installments(
        &self,
        principal: Money,
        annual_rate: Rate,
        term_months: u32,
        start_date: DateTime<Utc>,
    ) -> Result<Vec<ScheduledPayment>> {
        let monthly_rate = annual_rate.as_decimal() / dec!(12);
        let emi = calculate_emi_amount(principal, annual_rate, term_months);
        
        let mut payments = Vec::new();
        let mut balance = principal;
        let mut cumulative_interest = Money::ZERO;
        let mut cumulative_principal = Money::ZERO;
        
        for i in 1..=term_months {
            let payment_date = add_months(start_date, i);
            let interest = balance.as_decimal() * monthly_rate;
            let interest_portion = Money::from_decimal(interest);
            let principal_portion = emi - interest_portion;
            
            cumulative_interest += interest_portion;
            cumulative_principal += principal_portion;
            
            let ending_balance = (balance - principal_portion).max(Money::ZERO);
            
            payments.push(ScheduledPayment {
                payment_number: i,
                payment_date,
                beginning_balance: balance,
                payment_amount: emi,
                principal_portion,
                interest_portion,
                ending_balance,
                cumulative_interest,
                cumulative_principal,
            });
            
            balance = ending_balance;
        }
        
        // adjust last payment for rounding
        if let Some(last) = payments.last_mut() {
            if last.ending_balance > Money::ZERO && last.ending_balance < Money::from_major(1) {
                last.principal_portion += last.ending_balance;
                last.payment_amount += last.ending_balance;
                last.ending_balance = Money::ZERO;
            }
        }
        
        Ok(payments)
    }
    
    /// declining principal (equal principal payments)
    fn calculate_declining_principal(
        &self,
        principal: Money,
        annual_rate: Rate,
        term_months: u32,
        start_date: DateTime<Utc>,
    ) -> Result<Vec<ScheduledPayment>> {
        let monthly_rate = annual_rate.as_decimal() / dec!(12);
        let principal_payment = principal / Decimal::from(term_months);
        
        let mut payments = Vec::new();
        let mut balance = principal;
        let mut cumulative_interest = Money::ZERO;
        let mut cumulative_principal = Money::ZERO;
        
        for i in 1..=term_months {
            let payment_date = add_months(start_date, i);
            let interest = balance.as_decimal() * monthly_rate;
            let interest_portion = Money::from_decimal(interest);
            let payment_amount = principal_payment + interest_portion;
            
            cumulative_interest += interest_portion;
            cumulative_principal += principal_payment;
            
            let ending_balance = (balance - principal_payment).max(Money::ZERO);
            
            payments.push(ScheduledPayment {
                payment_number: i,
                payment_date,
                beginning_balance: balance,
                payment_amount,
                principal_portion: principal_payment,
                interest_portion,
                ending_balance,
                cumulative_interest,
                cumulative_principal,
            });
            
            balance = ending_balance;
        }
        
        Ok(payments)
    }
    
    /// interest only with balloon payment
    fn calculate_interest_only(
        &self,
        principal: Money,
        annual_rate: Rate,
        term_months: u32,
        start_date: DateTime<Utc>,
    ) -> Result<Vec<ScheduledPayment>> {
        let monthly_rate = annual_rate.as_decimal() / dec!(12);
        let interest_payment = Money::from_decimal(principal.as_decimal() * monthly_rate);
        
        let mut payments = Vec::new();
        let mut cumulative_interest = Money::ZERO;
        
        for i in 1..=term_months {
            let payment_date = add_months(start_date, i);
            let is_last = i == term_months;
            
            cumulative_interest += interest_payment;
            
            let (payment_amount, principal_portion, ending_balance) = if is_last {
                // balloon payment
                (interest_payment + principal, principal, Money::ZERO)
            } else {
                (interest_payment, Money::ZERO, principal)
            };
            
            payments.push(ScheduledPayment {
                payment_number: i,
                payment_date,
                beginning_balance: principal,
                payment_amount,
                principal_portion,
                interest_portion: interest_payment,
                ending_balance,
                cumulative_interest,
                cumulative_principal: if is_last { principal } else { Money::ZERO },
            });
        }
        
        Ok(payments)
    }
}

/// recalculation strategy after prepayment
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecalculationStrategy {
    ReduceEmi,
    ReduceTerm,
}

/// calculate EMI amount
fn calculate_emi_amount(principal: Money, annual_rate: Rate, months: u32) -> Money {
    if months == 0 {
        return principal;
    }
    
    let monthly_rate = annual_rate.as_decimal() / dec!(12);
    
    if monthly_rate.is_zero() {
        return principal / Decimal::from(months);
    }
    
    // EMI = P * r * (1 + r)^n / ((1 + r)^n - 1)
    let r = monthly_rate;
    let n = months;
    
    let mut compound = Decimal::ONE;
    let base = Decimal::ONE + r;
    for _ in 0..n {
        compound *= base;
    }
    
    let numerator = principal.as_decimal() * r * compound;
    let denominator = compound - Decimal::ONE;
    
    Money::from_decimal(numerator / denominator)
}

/// calculate term for given payment amount
fn calculate_term_for_payment(principal: Money, annual_rate: Rate, payment: Money) -> u32 {
    let monthly_rate = annual_rate.as_decimal() / dec!(12);
    
    if monthly_rate.is_zero() {
        return (principal.as_decimal() / payment.as_decimal())
            .round()
            .to_string()
            .parse()
            .unwrap_or(0);
    }
    
    // iterative calculation
    let mut remaining = principal;
    let mut months = 0;
    
    while remaining > Money::ZERO && months < 360 {
        let interest = remaining.as_decimal() * monthly_rate;
        let principal_payment = payment.as_decimal() - interest;
        
        if principal_payment <= Decimal::ZERO {
            break;
        }
        
        remaining = Money::from_decimal((remaining.as_decimal() - principal_payment).max(Decimal::ZERO));
        months += 1;
    }
    
    months
}

/// add months to date
fn add_months(date: DateTime<Utc>, months: u32) -> DateTime<Utc> {
    let mut result = date;
    for _ in 0..months {
        let days_in_month = days_in_month(result.year(), result.month());
        result = result + Duration::days(days_in_month as i64);
    }
    result
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 30,
    }
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use hourglass_rs::TimeSource;
    use uuid::Uuid;
    
    #[test]
    fn test_equal_installments_schedule() {
        let facility_id = Uuid::new_v4();
        let principal = Money::from_major(100_000);
        let rate = Rate::from_percentage(12);
        let term = 12;
        let start_date = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        
        let time = SafeTimeProvider::new(TimeSource::Test(start_date));
        
        let schedule = AmortizationSchedule::generate(
            facility_id,
            principal,
            rate,
            term,
            start_date,
            AmortizationMethod::EqualInstallments,
            &time,
        ).unwrap();
        
        assert_eq!(schedule.payments.len(), 12);
        
        // verify first payment
        let first = &schedule.payments[0];
        assert_eq!(first.beginning_balance, principal);
        assert!(first.interest_portion > Money::ZERO);
        assert!(first.principal_portion > Money::ZERO);
        
        // verify last payment
        let last = &schedule.payments[11];
        assert!(last.ending_balance < Money::from_major(1));
        
        // all EMIs should be equal (except possibly last)
        let emi = schedule.payments[0].payment_amount;
        for payment in &schedule.payments[..11] {
            assert!((payment.payment_amount - emi).abs() < Money::from_major(1));
        }
    }
    
    #[test]
    fn test_declining_principal_schedule() {
        let facility_id = Uuid::new_v4();
        let principal = Money::from_major(100_000);
        let rate = Rate::from_percentage(12);
        let term = 12;
        let start_date = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        
        let time = SafeTimeProvider::new(TimeSource::Test(start_date));
        
        let schedule = AmortizationSchedule::generate(
            facility_id,
            principal,
            rate,
            term,
            start_date,
            AmortizationMethod::DecliningPrincipal,
            &time,
        ).unwrap();
        
        // principal payments should be equal
        let expected_principal = principal / Decimal::from(12);
        for payment in &schedule.payments {
            assert!((payment.principal_portion - expected_principal).abs() < Money::from_major(1));
        }
        
        // interest should decline each month
        for i in 1..schedule.payments.len() {
            assert!(schedule.payments[i].interest_portion < schedule.payments[i-1].interest_portion);
        }
    }
    
    #[test]
    fn test_interest_only_schedule() {
        let facility_id = Uuid::new_v4();
        let principal = Money::from_major(100_000);
        let rate = Rate::from_percentage(12);
        let term = 12;
        let start_date = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        
        let time = SafeTimeProvider::new(TimeSource::Test(start_date));
        
        let schedule = AmortizationSchedule::generate(
            facility_id,
            principal,
            rate,
            term,
            start_date,
            AmortizationMethod::InterestOnly,
            &time,
        ).unwrap();
        
        // all payments except last should be interest only
        for payment in &schedule.payments[..11] {
            assert_eq!(payment.principal_portion, Money::ZERO);
            assert_eq!(payment.ending_balance, principal);
        }
        
        // last payment should include balloon
        let last = &schedule.payments[11];
        assert_eq!(last.principal_portion, principal);
        assert_eq!(last.ending_balance, Money::ZERO);
    }
    
    #[test]
    fn test_recalculation_after_prepayment() {
        let facility_id = Uuid::new_v4();
        let principal = Money::from_major(100_000);
        let rate = Rate::from_percentage(12);
        let term = 24;
        let start_date = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        
        let time = SafeTimeProvider::new(TimeSource::Test(start_date));
        
        let mut schedule = AmortizationSchedule::generate(
            facility_id,
            principal,
            rate,
            term,
            start_date,
            AmortizationMethod::EqualInstallments,
            &time,
        ).unwrap();
        
        let original_emi = schedule.payments[0].payment_amount;
        let original_total = schedule.total_payment;
        
        // make prepayment after 6 months
        schedule.recalculate_after_prepayment(
            Money::from_major(20_000),
            6,
            RecalculationStrategy::ReduceEmi,
            &time,
        ).unwrap();
        
        // EMI should be lower
        assert!(schedule.payments[6].payment_amount < original_emi);
        
        // total payment should be lower
        assert!(schedule.total_payment < original_total);
    }
}