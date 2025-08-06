use chrono::{DateTime, Utc};
use hourglass_rs::SafeTimeProvider;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::decimal::{Money, Rate};
use crate::errors::Result;
use crate::events::{Event, EventStore};
use crate::types::{FacilityId, OverpaymentStrategy};

use super::PaymentContext;

/// overpayment handler
pub struct OverpaymentHandler {
    facility_id: FacilityId,
    strategy: OverpaymentStrategy,
}

impl OverpaymentHandler {
    pub fn new(facility_id: FacilityId, strategy: OverpaymentStrategy) -> Self {
        Self {
            facility_id,
            strategy,
        }
    }
    
    /// handle overpayment based on strategy
    pub fn handle(
        &self,
        overpayment: Money,
        context: &mut PaymentContext,
        loan_params: &LoanParameters,
        time_provider: &SafeTimeProvider,
        events: &mut EventStore,
    ) -> Result<OverpaymentResult> {
        match self.strategy {
            OverpaymentStrategy::ReducePrincipal => {
                self.reduce_principal(overpayment, context, time_provider, events)
            }
            OverpaymentStrategy::ReduceEmi => {
                self.reduce_emi(overpayment, context, loan_params, time_provider, events)
            }
            OverpaymentStrategy::ReduceTerm => {
                self.reduce_term(overpayment, context, loan_params, time_provider, events)
            }
            OverpaymentStrategy::ReduceLimit => {
                self.reduce_credit_limit(overpayment, context, loan_params, time_provider, events)
            }
        }
    }
    
    /// directly reduce principal balance
    fn reduce_principal(
        &self,
        overpayment: Money,
        context: &mut PaymentContext,
        time_provider: &SafeTimeProvider,
        events: &mut EventStore,
    ) -> Result<OverpaymentResult> {
        let old_principal = context.outstanding_principal;
        context.outstanding_principal -= overpayment;
        
        events.emit(Event::OverpaymentReceived {
            facility_id: self.facility_id,
            amount: overpayment,
            strategy: OverpaymentStrategy::ReducePrincipal,
            timestamp: time_provider.now(),
        });
        
        Ok(OverpaymentResult {
            strategy: OverpaymentStrategy::ReducePrincipal,
            amount_applied: overpayment,
            old_principal,
            new_principal: context.outstanding_principal,
            old_emi: None,
            new_emi: None,
            old_term_months: None,
            new_term_months: None,
            savings: self.calculate_interest_savings(overpayment, Money::ZERO),
        })
    }
    
    /// recalculate EMI to reduce monthly payment
    fn reduce_emi(
        &self,
        overpayment: Money,
        context: &mut PaymentContext,
        loan_params: &LoanParameters,
        time_provider: &SafeTimeProvider,
        events: &mut EventStore,
    ) -> Result<OverpaymentResult> {
        let old_principal = context.outstanding_principal;
        let old_emi = loan_params.emi_amount;
        
        // reduce principal
        context.outstanding_principal -= overpayment;
        
        // recalculate EMI with same remaining term
        let new_emi = calculate_emi(
            context.outstanding_principal,
            loan_params.interest_rate,
            loan_params.remaining_months,
        );
        
        events.emit(Event::OverpaymentReceived {
            facility_id: self.facility_id,
            amount: overpayment,
            strategy: OverpaymentStrategy::ReduceEmi,
            timestamp: time_provider.now(),
        });
        
        let monthly_savings = old_emi - new_emi;
        let total_savings = monthly_savings * Decimal::from(loan_params.remaining_months);
        
        Ok(OverpaymentResult {
            strategy: OverpaymentStrategy::ReduceEmi,
            amount_applied: overpayment,
            old_principal,
            new_principal: context.outstanding_principal,
            old_emi: Some(old_emi),
            new_emi: Some(new_emi),
            old_term_months: Some(loan_params.remaining_months),
            new_term_months: Some(loan_params.remaining_months),
            savings: total_savings,
        })
    }
    
    /// recalculate term to finish loan earlier
    fn reduce_term(
        &self,
        overpayment: Money,
        context: &mut PaymentContext,
        loan_params: &LoanParameters,
        time_provider: &SafeTimeProvider,
        events: &mut EventStore,
    ) -> Result<OverpaymentResult> {
        let old_principal = context.outstanding_principal;
        let old_term = loan_params.remaining_months;
        
        // reduce principal
        context.outstanding_principal -= overpayment;
        
        // calculate new term with same EMI
        let new_term = calculate_term_for_emi(
            context.outstanding_principal,
            loan_params.interest_rate,
            loan_params.emi_amount,
        );
        
        events.emit(Event::OverpaymentReceived {
            facility_id: self.facility_id,
            amount: overpayment,
            strategy: OverpaymentStrategy::ReduceTerm,
            timestamp: time_provider.now(),
        });
        
        let months_saved = old_term.saturating_sub(new_term);
        let savings = loan_params.emi_amount * Decimal::from(months_saved);
        
        Ok(OverpaymentResult {
            strategy: OverpaymentStrategy::ReduceTerm,
            amount_applied: overpayment,
            old_principal,
            new_principal: context.outstanding_principal,
            old_emi: Some(loan_params.emi_amount),
            new_emi: Some(loan_params.emi_amount),
            old_term_months: Some(old_term),
            new_term_months: Some(new_term),
            savings,
        })
    }
    
    /// reduce credit limit (for revolving facilities)
    fn reduce_credit_limit(
        &self,
        overpayment: Money,
        context: &mut PaymentContext,
        loan_params: &LoanParameters,
        time_provider: &SafeTimeProvider,
        events: &mut EventStore,
    ) -> Result<OverpaymentResult> {
        let old_limit = loan_params.credit_limit.unwrap_or(Money::ZERO);
        let new_limit = (old_limit - overpayment).max(context.outstanding_principal);
        
        events.emit(Event::CreditLimitChanged {
            facility_id: self.facility_id,
            old_limit,
            new_limit,
            timestamp: time_provider.now(),
        });
        
        Ok(OverpaymentResult {
            strategy: OverpaymentStrategy::ReduceLimit,
            amount_applied: overpayment,
            old_principal: context.outstanding_principal,
            new_principal: context.outstanding_principal,
            old_emi: None,
            new_emi: None,
            old_term_months: None,
            new_term_months: None,
            savings: Money::ZERO,
        })
    }
    
    fn calculate_interest_savings(&self, principal_reduction: Money, rate_reduction: Money) -> Money {
        // simplified interest savings calculation
        principal_reduction * dec!(0.1) + rate_reduction * dec!(100)
    }
}

/// loan parameters for overpayment calculations
#[derive(Debug, Clone)]
pub struct LoanParameters {
    pub interest_rate: Rate,
    pub remaining_months: u32,
    pub emi_amount: Money,
    pub credit_limit: Option<Money>,
    pub maturity_date: Option<DateTime<Utc>>,
}

/// overpayment result
#[derive(Debug, Clone, PartialEq)]
pub struct OverpaymentResult {
    pub strategy: OverpaymentStrategy,
    pub amount_applied: Money,
    pub old_principal: Money,
    pub new_principal: Money,
    pub old_emi: Option<Money>,
    pub new_emi: Option<Money>,
    pub old_term_months: Option<u32>,
    pub new_term_months: Option<u32>,
    pub savings: Money,
}

/// calculate EMI for given parameters
pub fn calculate_emi(principal: Money, annual_rate: Rate, months: u32) -> Money {
    if months == 0 {
        return principal;
    }
    
    let monthly_rate = annual_rate.as_decimal() / dec!(12);
    
    if monthly_rate.is_zero() {
        // no interest case
        return principal / Decimal::from(months);
    }
    
    // EMI = P * r * (1 + r)^n / ((1 + r)^n - 1)
    let r = monthly_rate;
    let n = months;
    
    // calculate (1 + r)^n
    let mut compound = Decimal::ONE;
    let base = Decimal::ONE + r;
    for _ in 0..n {
        compound *= base;
    }
    
    let numerator = principal.as_decimal() * r * compound;
    let denominator = compound - Decimal::ONE;
    
    Money::from_decimal(numerator / denominator)
}

/// calculate term for given EMI
pub fn calculate_term_for_emi(principal: Money, annual_rate: Rate, emi: Money) -> u32 {
    if emi <= Money::ZERO || principal <= Money::ZERO {
        return 0;
    }
    
    let monthly_rate = annual_rate.as_decimal() / dec!(12);
    
    if monthly_rate.is_zero() {
        // no interest case
        return (principal.as_decimal() / emi.as_decimal())
            .round()
            .to_string()
            .parse()
            .unwrap_or(0);
    }
    
    // n = log(EMI / (EMI - P*r)) / log(1 + r)
    let r = monthly_rate;
    let p = principal.as_decimal();
    let e = emi.as_decimal();
    
    let interest_portion = p * r;
    if e <= interest_portion {
        // EMI not sufficient to cover interest
        return 0;
    }
    
    // approximate using iteration
    let mut remaining = principal;
    let mut months = 0;
    
    while remaining > Money::ZERO && months < 360 {
        let interest = remaining.as_decimal() * r;
        let principal_payment = e - interest;
        
        if principal_payment <= Decimal::ZERO {
            break;
        }
        
        remaining = Money::from_decimal((remaining.as_decimal() - principal_payment).max(Decimal::ZERO));
        months += 1;
    }
    
    months
}

/// balloon payment calculator
pub struct BalloonPaymentCalculator;

impl BalloonPaymentCalculator {
    /// calculate balloon payment amount
    pub fn calculate(
        original_principal: Money,
        regular_payment: Money,
        interest_rate: Rate,
        term_months: u32,
        payments_made: u32,
    ) -> Money {
        let monthly_rate = interest_rate.as_decimal() / dec!(12);
        let remaining_months = term_months.saturating_sub(payments_made);
        
        if remaining_months == 0 {
            return Money::ZERO;
        }
        
        // calculate remaining balance after regular payments
        let mut balance = original_principal;
        for _ in 0..payments_made {
            let interest = balance.as_decimal() * monthly_rate;
            let principal = regular_payment.as_decimal() - interest;
            balance = Money::from_decimal((balance.as_decimal() - principal).max(Decimal::ZERO));
        }
        
        balance
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hourglass_rs::TimeSource;
    use uuid::Uuid;
    
    #[test]
    fn test_emi_calculation() {
        let principal = Money::from_major(100_000);
        let rate = Rate::from_percentage(12);
        let months = 12;
        
        let emi = calculate_emi(principal, rate, months);
        
        // approximate EMI for $100k at 12% for 12 months
        assert!(emi > Money::from_major(8800));
        assert!(emi < Money::from_major(8900));
    }
    
    #[test]
    fn test_emi_zero_interest() {
        let principal = Money::from_major(12_000);
        let rate = Rate::from_percentage(0);
        let months = 12;
        
        let emi = calculate_emi(principal, rate, months);
        
        assert_eq!(emi, Money::from_major(1000));
    }
    
    #[test]
    fn test_term_calculation() {
        let principal = Money::from_major(100_000);
        let rate = Rate::from_percentage(12);
        let emi = Money::from_major(10_000);
        
        let term = calculate_term_for_emi(principal, rate, emi);
        
        // should be around 11 months
        assert!(term >= 10);
        assert!(term <= 12);
    }
    
    #[test]
    fn test_reduce_principal_strategy() {
        let facility_id = Uuid::new_v4();
        let handler = OverpaymentHandler::new(facility_id, OverpaymentStrategy::ReducePrincipal);
        
        let mut context = PaymentContext {
            facility_id,
            accrued_fees: Money::ZERO,
            accrued_penalties: Money::ZERO,
            accrued_interest: Money::ZERO,
            outstanding_principal: Money::from_major(100_000),
            minimum_payment: None,
            payment_due_date: None,
            days_overdue: 0,
        };
        
        let loan_params = LoanParameters {
            interest_rate: Rate::from_percentage(10),
            remaining_months: 120,
            emi_amount: Money::from_major(1000),
            credit_limit: None,
            maturity_date: None,
        };
        
        let time = SafeTimeProvider::new(TimeSource::Test(Utc::now()));
        let mut events = EventStore::new();
        
        let result = handler.handle(
            Money::from_major(10_000),
            &mut context,
            &loan_params,
            &time,
            &mut events,
        ).unwrap();
        
        assert_eq!(result.old_principal, Money::from_major(100_000));
        assert_eq!(result.new_principal, Money::from_major(90_000));
        assert_eq!(context.outstanding_principal, Money::from_major(90_000));
    }
    
    #[test]
    fn test_reduce_emi_strategy() {
        let facility_id = Uuid::new_v4();
        let handler = OverpaymentHandler::new(facility_id, OverpaymentStrategy::ReduceEmi);
        
        let mut context = PaymentContext {
            facility_id,
            accrued_fees: Money::ZERO,
            accrued_penalties: Money::ZERO,
            accrued_interest: Money::ZERO,
            outstanding_principal: Money::from_major(100_000),
            minimum_payment: None,
            payment_due_date: None,
            days_overdue: 0,
        };
        
        let loan_params = LoanParameters {
            interest_rate: Rate::from_percentage(10),
            remaining_months: 120,
            emi_amount: Money::from_major(1322),
            credit_limit: None,
            maturity_date: None,
        };
        
        let time = SafeTimeProvider::new(TimeSource::Test(Utc::now()));
        let mut events = EventStore::new();
        
        let result = handler.handle(
            Money::from_major(10_000),
            &mut context,
            &loan_params,
            &time,
            &mut events,
        ).unwrap();
        
        assert_eq!(result.new_principal, Money::from_major(90_000));
        assert!(result.new_emi.unwrap() < result.old_emi.unwrap());
    }
    
    #[test]
    fn test_reduce_term_strategy() {
        let facility_id = Uuid::new_v4();
        let handler = OverpaymentHandler::new(facility_id, OverpaymentStrategy::ReduceTerm);
        
        let mut context = PaymentContext {
            facility_id,
            accrued_fees: Money::ZERO,
            accrued_penalties: Money::ZERO,
            accrued_interest: Money::ZERO,
            outstanding_principal: Money::from_major(100_000),
            minimum_payment: None,
            payment_due_date: None,
            days_overdue: 0,
        };
        
        let loan_params = LoanParameters {
            interest_rate: Rate::from_percentage(10),
            remaining_months: 120,
            emi_amount: Money::from_major(1322),
            credit_limit: None,
            maturity_date: None,
        };
        
        let time = SafeTimeProvider::new(TimeSource::Test(Utc::now()));
        let mut events = EventStore::new();
        
        let result = handler.handle(
            Money::from_major(10_000),
            &mut context,
            &loan_params,
            &time,
            &mut events,
        ).unwrap();
        
        assert_eq!(result.new_principal, Money::from_major(90_000));
        assert!(result.new_term_months.unwrap() < result.old_term_months.unwrap());
        assert_eq!(result.old_emi, result.new_emi); // EMI stays the same
    }
    
    #[test]
    fn test_balloon_payment() {
        let original = Money::from_major(100_000);
        let payment = Money::from_major(500);
        let rate = Rate::from_percentage(5);
        
        let balloon = BalloonPaymentCalculator::calculate(
            original,
            payment,
            rate,
            60,  // 5 year term
            59,  // 59 payments made
        );
        
        // should have significant balloon payment
        assert!(balloon > Money::from_major(80_000));
    }
}