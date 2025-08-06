use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::decimal::{Money, Rate};
use crate::types::{
    CollateralPosition, FacilityId, FacilityStatus,
};

/// facility state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FacilityState {
    // identification
    pub facility_id: FacilityId,
    pub account_number: String,
    pub customer_id: String,
    
    // core balances
    pub original_commitment: Money,
    pub outstanding_principal: Money,
    pub accrued_interest: Money,
    pub accrued_fees: Money,
    pub accrued_penalties: Money,
    
    // disbursement tracking
    pub total_disbursed: Money,
    pub available_commitment: Money,
    
    // payment tracking
    pub total_payments_received: Money,
    pub last_payment_amount: Option<Money>,
    pub last_payment_date: Option<DateTime<Utc>>,
    pub next_payment_due: Option<DateTime<Utc>>,
    pub next_payment_amount: Option<Money>,
    pub minimum_payment_due: Option<Money>,
    
    // interest tracking
    pub last_interest_accrual: DateTime<Utc>,
    pub total_interest_paid: Money,
    pub capitalized_interest: Money,
    
    // fee tracking
    pub total_fees_charged: Money,
    pub total_fees_paid: Money,
    pub waived_fees: Money,
    
    // dates
    pub origination_date: DateTime<Utc>,
    pub activation_date: Option<DateTime<Utc>>,
    pub maturity_date: Option<DateTime<Utc>>,
    pub last_status_change: DateTime<Utc>,
    
    // status
    pub status: FacilityStatus,
    pub days_past_due: u32,
    pub payment_count: u32,
    pub missed_payment_count: u32,
    
    // facility-specific state
    pub facility_specific: FacilitySpecificState,
    
    // collateral
    pub collateral: Option<CollateralPosition>,
    
    // suspense account
    pub suspense_balance: Money,
    
    // write-off tracking
    pub write_off_amount: Option<Money>,
    pub write_off_date: Option<DateTime<Utc>>,
    pub recovery_amount: Option<Money>,
}

/// facility-specific state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum FacilitySpecificState {
    TermLoan {
        scheduled_payment: Money,
        remaining_term_months: u32,
        balloon_payment: Option<Money>,
        prepayment_amount: Money,
    },
    OpenTerm {
        collateral_value: Money,
        collateral_amount: String, // e.g., "1.5 BTC"
        last_valuation: DateTime<Utc>,
        margin_call_active: bool,
        margin_call_deadline: Option<DateTime<Utc>>,
    },
    Revolving {
        credit_limit: Money,
        available_credit: Money,
        cash_advance_balance: Money,
        purchase_balance: Money,
        promotional_balance: Money,
        overlimit_amount: Money,
        statement_balance: Money,
        statement_date: Option<DateTime<Utc>>,
    },
    Overdraft {
        overdraft_limit: Money,
        linked_account_balance: Money,
        buffer_zone: Money,
        daily_interest_accrued: Money,
    },
}

impl FacilityState {
    /// create new facility state
    pub fn new(
        facility_id: FacilityId,
        account_number: String,
        customer_id: String,
        commitment: Money,
        origination_date: DateTime<Utc>,
        facility_type: FacilityStateType,
    ) -> Self {
        let facility_specific = match facility_type {
            FacilityStateType::TermLoan { payment, term, balloon } => {
                FacilitySpecificState::TermLoan {
                    scheduled_payment: payment,
                    remaining_term_months: term,
                    balloon_payment: balloon,
                    prepayment_amount: Money::ZERO,
                }
            }
            FacilityStateType::OpenTerm => {
                FacilitySpecificState::OpenTerm {
                    collateral_value: Money::ZERO,
                    collateral_amount: String::new(),
                    last_valuation: origination_date,
                    margin_call_active: false,
                    margin_call_deadline: None,
                }
            }
            FacilityStateType::Revolving { limit } => {
                FacilitySpecificState::Revolving {
                    credit_limit: limit,
                    available_credit: limit,
                    cash_advance_balance: Money::ZERO,
                    purchase_balance: Money::ZERO,
                    promotional_balance: Money::ZERO,
                    overlimit_amount: Money::ZERO,
                    statement_balance: Money::ZERO,
                    statement_date: None,
                }
            }
            FacilityStateType::Overdraft { limit } => {
                FacilitySpecificState::Overdraft {
                    overdraft_limit: limit,
                    linked_account_balance: Money::ZERO,
                    buffer_zone: Money::from_major(100),
                    daily_interest_accrued: Money::ZERO,
                }
            }
        };
        
        Self {
            facility_id,
            account_number,
            customer_id,
            original_commitment: commitment,
            outstanding_principal: Money::ZERO,
            accrued_interest: Money::ZERO,
            accrued_fees: Money::ZERO,
            accrued_penalties: Money::ZERO,
            total_disbursed: Money::ZERO,
            available_commitment: commitment,
            total_payments_received: Money::ZERO,
            last_payment_amount: None,
            last_payment_date: None,
            next_payment_due: None,
            next_payment_amount: None,
            minimum_payment_due: None,
            last_interest_accrual: origination_date,
            total_interest_paid: Money::ZERO,
            capitalized_interest: Money::ZERO,
            total_fees_charged: Money::ZERO,
            total_fees_paid: Money::ZERO,
            waived_fees: Money::ZERO,
            origination_date,
            activation_date: None,
            maturity_date: None,
            last_status_change: origination_date,
            status: FacilityStatus::Originated,
            days_past_due: 0,
            payment_count: 0,
            missed_payment_count: 0,
            facility_specific,
            collateral: None,
            suspense_balance: Money::ZERO,
            write_off_amount: None,
            write_off_date: None,
            recovery_amount: None,
        }
    }
    
    /// get total outstanding balance
    pub fn total_outstanding(&self) -> Money {
        self.outstanding_principal + self.accrued_interest + self.accrued_fees + self.accrued_penalties
    }
    
    /// get current exposure (for revolving)
    pub fn current_exposure(&self) -> Money {
        match &self.facility_specific {
            FacilitySpecificState::Revolving { 
                cash_advance_balance,
                purchase_balance,
                promotional_balance,
                ..
            } => *cash_advance_balance + *purchase_balance + *promotional_balance,
            _ => self.outstanding_principal,
        }
    }
    
    /// check if facility is performing
    pub fn is_performing(&self) -> bool {
        matches!(
            self.status,
            FacilityStatus::Active | FacilityStatus::Originated | FacilityStatus::GracePeriod
        )
    }
    
    /// check if facility is in default
    pub fn is_in_default(&self) -> bool {
        matches!(
            self.status,
            FacilityStatus::Default | FacilityStatus::Liquidating | FacilityStatus::Liquidated
        )
    }
    
    /// check if facility can accept payments
    pub fn can_accept_payment(&self) -> bool {
        !matches!(
            self.status,
            FacilityStatus::Settled | FacilityStatus::ChargedOff | FacilityStatus::Liquidated
        )
    }
    
    /// check if facility can disburse
    pub fn can_disburse(&self) -> bool {
        self.is_performing() && self.available_commitment > Money::ZERO
    }
    
    /// update status
    pub fn update_status(&mut self, new_status: FacilityStatus, timestamp: DateTime<Utc>) {
        self.status = new_status;
        self.last_status_change = timestamp;
    }
    
    /// record payment
    pub fn record_payment(&mut self, amount: Money, timestamp: DateTime<Utc>) {
        self.total_payments_received += amount;
        self.last_payment_amount = Some(amount);
        self.last_payment_date = Some(timestamp);
        self.payment_count += 1;
        
        // reset DPD if payment sufficient
        if let Some(minimum) = self.minimum_payment_due {
            if amount >= minimum {
                self.days_past_due = 0;
            }
        }
    }
    
    /// record disbursement
    pub fn record_disbursement(&mut self, amount: Money) {
        self.total_disbursed += amount;
        self.outstanding_principal += amount;
        self.available_commitment = (self.available_commitment - amount).max(Money::ZERO);
        
        if self.activation_date.is_none() {
            self.activation_date = Some(Utc::now());
            self.status = FacilityStatus::Active;
        }
    }
    
    /// calculate current ltv for open-term loans
    pub fn calculate_ltv(&self) -> Option<Rate> {
        if let FacilitySpecificState::OpenTerm { collateral_value, .. } = &self.facility_specific {
            if collateral_value.is_zero() {
                return None;
            }
            let total_debt = self.total_outstanding();
            let ltv = total_debt.as_decimal() / collateral_value.as_decimal();
            Some(Rate::from_decimal(ltv))
        } else {
            None
        }
    }
    
    /// update collateral value for open-term loans
    pub fn update_collateral_value(&mut self, new_value: Money, timestamp: DateTime<Utc>) {
        if let FacilitySpecificState::OpenTerm { 
            collateral_value,
            last_valuation,
            ..
        } = &mut self.facility_specific {
            *collateral_value = new_value;
            *last_valuation = timestamp;
        }
    }
    
    /// check if margin call is required
    pub fn check_margin_call(&self, margin_call_ltv: Rate) -> bool {
        if let Some(current_ltv) = self.calculate_ltv() {
            current_ltv > margin_call_ltv
        } else {
            false
        }
    }
}

/// facility state type for initialization
pub enum FacilityStateType {
    TermLoan {
        payment: Money,
        term: u32,
        balloon: Option<Money>,
    },
    OpenTerm,
    Revolving {
        limit: Money,
    },
    Overdraft {
        limit: Money,
    },
}

/// state snapshot for audit trail
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateSnapshot {
    pub snapshot_id: Uuid,
    pub facility_id: FacilityId,
    pub timestamp: DateTime<Utc>,
    pub state: FacilityState,
    pub trigger: String,
}

impl StateSnapshot {
    pub fn capture(state: &FacilityState, trigger: String) -> Self {
        Self {
            snapshot_id: Uuid::new_v4(),
            facility_id: state.facility_id,
            timestamp: Utc::now(),
            state: state.clone(),
            trigger,
        }
    }
}