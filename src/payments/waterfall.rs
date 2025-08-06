use chrono::{DateTime, Utc};
use hourglass_rs::SafeTimeProvider;

use crate::decimal::Money;
use crate::errors::Result;
use crate::events::{Event, EventStore};
use crate::types::{FacilityId, PaymentApplication};

use super::{PaymentContext, PaymentRequest};

/// waterfall priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum WaterfallPriority {
    First = 1,
    Second = 2,
    Third = 3,
    Fourth = 4,
    Fifth = 5,
    Sixth = 6,
}

/// payment waterfall configuration
#[derive(Debug, Clone)]
pub struct PaymentWaterfall {
    pub fees_priority: WaterfallPriority,
    pub penalties_priority: WaterfallPriority,
    pub interest_priority: WaterfallPriority,
    pub principal_priority: WaterfallPriority,
    pub allow_overpayment: bool,
}

impl PaymentWaterfall {
    /// standard waterfall: fees -> penalties -> interest -> principal
    pub fn standard() -> Self {
        Self {
            fees_priority: WaterfallPriority::First,
            penalties_priority: WaterfallPriority::Second,
            interest_priority: WaterfallPriority::Third,
            principal_priority: WaterfallPriority::Fourth,
            allow_overpayment: true,
        }
    }
    
    /// interest-first waterfall for certain products
    pub fn interest_first() -> Self {
        Self {
            fees_priority: WaterfallPriority::Second,
            penalties_priority: WaterfallPriority::Third,
            interest_priority: WaterfallPriority::First,
            principal_priority: WaterfallPriority::Fourth,
            allow_overpayment: true,
        }
    }
    
    /// principal-only waterfall (must have zero other balances)
    pub fn principal_only() -> Self {
        Self {
            fees_priority: WaterfallPriority::First,
            penalties_priority: WaterfallPriority::Second,
            interest_priority: WaterfallPriority::Third,
            principal_priority: WaterfallPriority::Fourth,
            allow_overpayment: false,
        }
    }
}

/// payment processor
pub struct PaymentProcessor {
    waterfall: PaymentWaterfall,
}

impl PaymentProcessor {
    pub fn new(waterfall: PaymentWaterfall) -> Self {
        Self { waterfall }
    }
    
    /// process payment through waterfall
    pub fn process(
        &self,
        payment: PaymentRequest,
        context: &mut PaymentContext,
        time_provider: &SafeTimeProvider,
        events: &mut EventStore,
    ) -> Result<PaymentResult> {
        // validate payment
        context.validate_payment(payment.amount)?;
        
        let mut remaining = payment.amount;
        let mut application = PaymentApplication {
            to_fees: Money::ZERO,
            to_penalties: Money::ZERO,
            to_interest: Money::ZERO,
            to_principal: Money::ZERO,
            excess: Money::ZERO,
        };
        
        // create priority order
        let mut priorities = vec![
            (self.waterfall.fees_priority, PaymentComponent::Fees),
            (self.waterfall.penalties_priority, PaymentComponent::Penalties),
            (self.waterfall.interest_priority, PaymentComponent::Interest),
            (self.waterfall.principal_priority, PaymentComponent::Principal),
        ];
        priorities.sort_by_key(|&(priority, _)| priority);
        
        // apply payment in priority order
        for (_, component) in priorities {
            remaining = self.apply_to_component(
                component,
                remaining,
                context,
                &mut application,
            );
            
            if remaining.is_zero() {
                break;
            }
        }
        
        // handle excess
        if remaining > Money::ZERO {
            if self.waterfall.allow_overpayment {
                application.excess = remaining;
            } else {
                // return excess if overpayment not allowed
                application.excess = remaining;
            }
        }
        
        // emit payment event
        events.emit(Event::PaymentReceived {
            facility_id: payment.facility_id,
            amount: payment.amount,
            applied_to_fees: application.to_fees,
            applied_to_interest: application.to_interest,
            applied_to_principal: application.to_principal,
            timestamp: time_provider.now(),
        });
        
        Ok(PaymentResult {
            payment_id: payment.facility_id,
            amount_applied: application.total_applied(),
            application,
            remaining_balance: context.total_outstanding(),
            payment_date: payment.payment_date,
        })
    }
    
    fn apply_to_component(
        &self,
        component: PaymentComponent,
        available: Money,
        context: &mut PaymentContext,
        application: &mut PaymentApplication,
    ) -> Money {
        let (balance, applied_field) = match component {
            PaymentComponent::Fees => (&mut context.accrued_fees, &mut application.to_fees),
            PaymentComponent::Penalties => (&mut context.accrued_penalties, &mut application.to_penalties),
            PaymentComponent::Interest => (&mut context.accrued_interest, &mut application.to_interest),
            PaymentComponent::Principal => (&mut context.outstanding_principal, &mut application.to_principal),
        };
        
        let payment = available.min(*balance);
        *balance -= payment;
        *applied_field = payment;
        
        available - payment
    }
}

#[derive(Debug, Clone, Copy)]
enum PaymentComponent {
    Fees,
    Penalties,
    Interest,
    Principal,
}

/// payment result
#[derive(Debug, Clone, PartialEq)]
pub struct PaymentResult {
    pub payment_id: FacilityId,
    pub amount_applied: Money,
    pub application: PaymentApplication,
    pub remaining_balance: Money,
    pub payment_date: DateTime<Utc>,
}

/// facility-specific waterfall configurations
pub mod facility_waterfalls {
    use super::*;
    
    /// credit card waterfall with balance type priorities
    pub struct CreditCardWaterfall {
        _base_waterfall: PaymentWaterfall,
        balance_priorities: Vec<(String, WaterfallPriority)>,
    }
    
    impl CreditCardWaterfall {
        pub fn new() -> Self {
            Self {
                _base_waterfall: PaymentWaterfall::standard(),
                balance_priorities: vec![
                    ("cash_advance".to_string(), WaterfallPriority::First),
                    ("purchase".to_string(), WaterfallPriority::Second),
                    ("balance_transfer".to_string(), WaterfallPriority::Third),
                    ("promotional".to_string(), WaterfallPriority::Fourth),
                ],
            }
        }
        
        pub fn process_by_balance_type(
            &self,
            payment: Money,
            balances: &mut CreditCardBalances,
        ) -> PaymentApplication {
            let mut remaining = payment;
            let mut application = PaymentApplication::default();
            
            // sort balances by priority
            let mut sorted_balances = balances.to_vec();
            sorted_balances.sort_by_key(|(balance_type, _)| {
                self.balance_priorities
                    .iter()
                    .find(|(bt, _)| bt == balance_type)
                    .map(|(_, priority)| *priority)
                    .unwrap_or(WaterfallPriority::Sixth)
            });
            
            // apply to each balance type
            for (balance_type, balance) in sorted_balances.iter_mut() {
                let payment_amount = remaining.min(*balance);
                *balance -= payment_amount;
                
                match balance_type.as_str() {
                    "cash_advance" => application.to_principal += payment_amount,
                    _ => application.to_principal += payment_amount,
                }
                
                remaining -= payment_amount;
                if remaining.is_zero() {
                    break;
                }
            }
            
            application.excess = remaining;
            application
        }
    }
    
    pub type CreditCardBalances = Vec<(String, Money)>;
    
    /// mortgage waterfall with escrow handling
    pub struct MortgageWaterfall {
        _base_waterfall: PaymentWaterfall,
        include_escrow: bool,
    }
    
    impl MortgageWaterfall {
        pub fn new(include_escrow: bool) -> Self {
            Self {
                _base_waterfall: PaymentWaterfall::standard(),
                include_escrow,
            }
        }
        
        pub fn process_with_escrow(
            &self,
            payment: Money,
            context: &mut PaymentContext,
            escrow_shortage: Money,
            escrow_payment: Money,
        ) -> PaymentApplication {
            let mut remaining = payment;
            let mut application = PaymentApplication::default();
            
            // apply to escrow shortage first
            if escrow_shortage > Money::ZERO {
                let escrow_portion = remaining.min(escrow_shortage);
                remaining -= escrow_portion;
                // track separately, not in standard application
            }
            
            // standard waterfall for P&I
            let fees_payment = remaining.min(context.accrued_fees);
            context.accrued_fees -= fees_payment;
            application.to_fees = fees_payment;
            remaining -= fees_payment;
            
            let interest_payment = remaining.min(context.accrued_interest);
            context.accrued_interest -= interest_payment;
            application.to_interest = interest_payment;
            remaining -= interest_payment;
            
            let principal_payment = remaining.min(context.outstanding_principal);
            context.outstanding_principal -= principal_payment;
            application.to_principal = principal_payment;
            remaining -= principal_payment;
            
            // apply to escrow account
            if self.include_escrow && remaining >= escrow_payment {
                remaining -= escrow_payment;
                // track escrow separately
            }
            
            application.excess = remaining;
            application
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hourglass_rs::TimeSource;
    use uuid::Uuid;
    
    fn create_test_context() -> PaymentContext {
        PaymentContext {
            facility_id: Uuid::new_v4(),
            accrued_fees: Money::from_major(50),
            accrued_penalties: Money::from_major(25),
            accrued_interest: Money::from_major(100),
            outstanding_principal: Money::from_major(1000),
            minimum_payment: None,
            payment_due_date: None,
            days_overdue: 0,
        }
    }
    
    #[test]
    fn test_standard_waterfall() {
        let processor = PaymentProcessor::new(PaymentWaterfall::standard());
        let mut context = create_test_context();
        let mut events = EventStore::new();
        
        let time = SafeTimeProvider::new(TimeSource::Test(Utc::now()));
        
        let payment = PaymentRequest {
            facility_id: context.facility_id,
            amount: Money::from_major(125),
            payment_date: time.now(),
            reference: "test".to_string(),
            is_principal_only: false,
        };
        
        let result = processor.process(payment, &mut context, &time, &mut events).unwrap();
        
        // should apply: $50 fees, $25 penalties, $50 interest, $0 principal
        assert_eq!(result.application.to_fees, Money::from_major(50));
        assert_eq!(result.application.to_penalties, Money::from_major(25));
        assert_eq!(result.application.to_interest, Money::from_major(50));
        assert_eq!(result.application.to_principal, Money::ZERO);
        assert_eq!(result.application.excess, Money::ZERO);
        
        // verify balances updated
        assert_eq!(context.accrued_fees, Money::ZERO);
        assert_eq!(context.accrued_penalties, Money::ZERO);
        assert_eq!(context.accrued_interest, Money::from_major(50));
        assert_eq!(context.outstanding_principal, Money::from_major(1000));
    }
    
    #[test]
    fn test_waterfall_with_overpayment() {
        let processor = PaymentProcessor::new(PaymentWaterfall::standard());
        let mut context = create_test_context();
        let mut events = EventStore::new();
        
        let time = SafeTimeProvider::new(TimeSource::Test(Utc::now()));
        
        let payment = PaymentRequest {
            facility_id: context.facility_id,
            amount: Money::from_major(1300),
            payment_date: time.now(),
            reference: "test".to_string(),
            is_principal_only: false,
        };
        
        let result = processor.process(payment, &mut context, &time, &mut events).unwrap();
        
        // should apply all balances and have excess
        assert_eq!(result.application.to_fees, Money::from_major(50));
        assert_eq!(result.application.to_penalties, Money::from_major(25));
        assert_eq!(result.application.to_interest, Money::from_major(100));
        assert_eq!(result.application.to_principal, Money::from_major(1000));
        assert_eq!(result.application.excess, Money::from_major(125));
        
        // all balances should be zero
        assert_eq!(context.total_outstanding(), Money::ZERO);
    }
    
    #[test]
    fn test_interest_first_waterfall() {
        let processor = PaymentProcessor::new(PaymentWaterfall::interest_first());
        let mut context = create_test_context();
        let mut events = EventStore::new();
        
        let time = SafeTimeProvider::new(TimeSource::Test(Utc::now()));
        
        let payment = PaymentRequest {
            facility_id: context.facility_id,
            amount: Money::from_major(125),
            payment_date: time.now(),
            reference: "test".to_string(),
            is_principal_only: false,
        };
        
        let result = processor.process(payment, &mut context, &time, &mut events).unwrap();
        
        // should apply: $100 interest, $25 fees, $0 penalties
        assert_eq!(result.application.to_interest, Money::from_major(100));
        assert_eq!(result.application.to_fees, Money::from_major(25));
        assert_eq!(result.application.to_penalties, Money::ZERO);
        assert_eq!(result.application.to_principal, Money::ZERO);
    }
    
    #[test]
    fn test_mortgage_with_escrow() {
        use facility_waterfalls::MortgageWaterfall;
        
        let waterfall = MortgageWaterfall::new(true);
        let mut context = create_test_context();
        
        let escrow_shortage = Money::from_major(100);
        let escrow_payment = Money::from_major(200);
        
        let application = waterfall.process_with_escrow(
            Money::from_major(500),
            &mut context,
            escrow_shortage,
            escrow_payment,
        );
        
        // should handle escrow shortage first, then standard waterfall
        // $500 payment - $100 escrow shortage = $400 remaining
        // $400 - $50 fees - $100 interest = $250 for principal
        assert_eq!(application.to_fees, Money::from_major(50));
        assert_eq!(application.to_interest, Money::from_major(100));
        assert_eq!(application.to_principal, Money::from_major(250));
    }
}