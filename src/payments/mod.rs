pub mod amortization;
pub mod overpayment;
pub mod waterfall;

use chrono::{DateTime, Utc};
use hourglass_rs::SafeTimeProvider;

use crate::decimal::Money;
use crate::errors::{FacilityError, Result};
use crate::events::EventStore;
use crate::types::{FacilityId, OverpaymentStrategy};

pub use amortization::{AmortizationCalculator, AmortizationSchedule, ScheduledPayment};
pub use overpayment::{OverpaymentHandler, OverpaymentResult};
pub use waterfall::{
    PaymentProcessor, PaymentResult, PaymentWaterfall, WaterfallPriority,
};

/// payment request
#[derive(Debug, Clone, PartialEq)]
pub struct PaymentRequest {
    pub facility_id: FacilityId,
    pub amount: Money,
    pub payment_date: DateTime<Utc>,
    pub reference: String,
    pub is_principal_only: bool,
}

/// payment context with current balances
#[derive(Debug, Clone)]
pub struct PaymentContext {
    pub facility_id: FacilityId,
    pub accrued_fees: Money,
    pub accrued_penalties: Money,
    pub accrued_interest: Money,
    pub outstanding_principal: Money,
    pub minimum_payment: Option<Money>,
    pub payment_due_date: Option<DateTime<Utc>>,
    pub days_overdue: u32,
}

impl PaymentContext {
    pub fn total_outstanding(&self) -> Money {
        self.accrued_fees + self.accrued_penalties + self.accrued_interest + self.outstanding_principal
    }
    
    pub fn is_overdue(&self) -> bool {
        self.days_overdue > 0
    }
    
    pub fn validate_payment(&self, amount: Money) -> Result<()> {
        if amount.is_zero() || amount.is_negative() {
            return Err(FacilityError::InvalidPaymentAmount { amount });
        }
        
        if let Some(minimum) = self.minimum_payment {
            if amount < minimum && amount < self.total_outstanding() {
                return Err(FacilityError::PaymentBelowMinimum {
                    minimum,
                    provided: amount,
                });
            }
        }
        
        Ok(())
    }
}

/// partial payment handling
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum PartialPaymentStrategy {
    /// hold in suspense until sufficient for minimum payment
    HoldInSuspense,
    /// apply immediately even if below minimum
    ApplyImmediately,
    /// reject partial payments
    Reject,
}

/// suspense account for holding partial payments
#[derive(Debug, Clone)]
pub struct SuspenseAccount {
    pub facility_id: FacilityId,
    pub balance: Money,
    pub deposits: Vec<SuspenseDeposit>,
}

#[derive(Debug, Clone)]
pub struct SuspenseDeposit {
    pub amount: Money,
    pub deposit_date: DateTime<Utc>,
    pub reference: String,
}

impl SuspenseAccount {
    pub fn new(facility_id: FacilityId) -> Self {
        Self {
            facility_id,
            balance: Money::ZERO,
            deposits: Vec::new(),
        }
    }
    
    pub fn add_deposit(&mut self, amount: Money, date: DateTime<Utc>, reference: String) {
        self.balance += amount;
        self.deposits.push(SuspenseDeposit {
            amount,
            deposit_date: date,
            reference,
        });
    }
    
    pub fn can_release(&self, minimum_payment: Money) -> bool {
        self.balance >= minimum_payment
    }
    
    pub fn release_funds(&mut self) -> Money {
        let amount = self.balance;
        self.balance = Money::ZERO;
        self.deposits.clear();
        amount
    }
}

/// trait for payment processing
pub trait PaymentProcessable {
    fn process_payment(
        &mut self,
        payment: PaymentRequest,
        context: &PaymentContext,
        time_provider: &SafeTimeProvider,
        events: &mut EventStore,
    ) -> Result<PaymentResult>;
    
    fn handle_overpayment(
        &mut self,
        excess: Money,
        strategy: OverpaymentStrategy,
        context: &mut PaymentContext,
        time_provider: &SafeTimeProvider,
        events: &mut EventStore,
    ) -> Result<OverpaymentResult>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use hourglass_rs::TimeSource;
    use uuid::Uuid;
    
    #[test]
    fn test_payment_context_validation() {
        let context = PaymentContext {
            facility_id: Uuid::new_v4(),
            accrued_fees: Money::from_major(50),
            accrued_penalties: Money::from_major(25),
            accrued_interest: Money::from_major(100),
            outstanding_principal: Money::from_major(10_000),
            minimum_payment: Some(Money::from_major(200)),
            payment_due_date: None,
            days_overdue: 0,
        };
        
        // test zero payment
        assert!(context.validate_payment(Money::ZERO).is_err());
        
        // test below minimum
        assert!(context.validate_payment(Money::from_major(150)).is_err());
        
        // test valid payment
        assert!(context.validate_payment(Money::from_major(200)).is_ok());
        
        // test full payment
        assert!(context.validate_payment(context.total_outstanding()).is_ok());
    }
    
    #[test]
    fn test_suspense_account() {
        let facility_id = Uuid::new_v4();
        let mut suspense = SuspenseAccount::new(facility_id);
        
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc::now()
        ));
        let now = time.now();
        
        // add deposits
        suspense.add_deposit(Money::from_major(50), now, "payment1".to_string());
        suspense.add_deposit(Money::from_major(75), now, "payment2".to_string());
        
        assert_eq!(suspense.balance, Money::from_major(125));
        assert_eq!(suspense.deposits.len(), 2);
        
        // check release threshold
        assert!(!suspense.can_release(Money::from_major(200)));
        assert!(suspense.can_release(Money::from_major(125)));
        
        // release funds
        let released = suspense.release_funds();
        assert_eq!(released, Money::from_major(125));
        assert_eq!(suspense.balance, Money::ZERO);
        assert!(suspense.deposits.is_empty());
    }
}