use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::decimal::{Money, Rate};

/// unique identifier for a facility
pub type FacilityId = Uuid;

/// term loan types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TermLoanType {
    Mortgage,
    PersonalLoan,
    AutoLoan,
    StudentLoan,
    BusinessLoan,
}

/// open-term loan types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum OpenTermType {
    BitcoinBacked,
    AssetBacked,
}

/// revolving facility types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RevolvingType {
    CreditCard,
    LineOfCredit,
    HELOC,
}

/// facility status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FacilityStatus {
    /// loan created but not yet disbursed
    Originated,
    /// loan active and performing
    Active,
    /// payment missed but within grace period
    GracePeriod,
    /// past grace period, late fees apply
    Delinquent,
    /// seriously delinquent or covenant breach
    Default,
    /// liquidation in process
    Liquidating,
    /// collateral sold
    Liquidated,
    /// fully paid off
    Settled,
    /// written off as loss
    ChargedOff,
}

/// amortization method for term loans
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AmortizationMethod {
    /// each payment reduces principal, interest on remaining
    DecliningPrincipal,
    /// equal payment amounts throughout term
    EqualInstallments,
    /// pay interest only, principal at maturity
    InterestOnly,
}


/// payment schedule
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum PaymentSchedule {
    /// monthly payment on specific day
    Monthly { day_of_month: u8 },
    /// weekly payment on specific day
    Weekly { day_of_week: u8 },
    /// daily payments
    Daily,
    /// no fixed schedule, pay on demand
    OnDemand,
    /// interest only payments
    InterestOnly { payment_day: u8 },
    /// no payment required (open-term loans)
    None,
}

/// overpayment application method
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum OverpaymentStrategy {
    /// reduce future payment amounts
    ReduceEmi,
    /// reduce loan term
    ReduceTerm,
    /// one-time principal reduction
    ReducePrincipal,
    /// reduce credit limit (revolving)
    ReduceLimit,
}

/// collateral position
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CollateralPosition {
    pub asset_type: String,
    pub asset_amount: Decimal,
    pub current_value: Money,
    pub initial_value: Money,
    pub last_valuation: DateTime<Utc>,
    pub valuation_source: String,
}

/// ltv thresholds
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct LtvThresholds {
    pub initial_ltv: Rate,
    pub warning_ltv: Rate,
    pub margin_call_ltv: Rate,
    pub liquidation_ltv: Rate,
}

/// ltv status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LtvStatus {
    Healthy,
    Warning,
    MarginCall,
    Liquidation,
}

/// payment application result
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct PaymentApplication {
    pub to_fees: Money,
    pub to_penalties: Money,
    pub to_interest: Money,
    pub to_principal: Money,
    pub excess: Money,
}

impl PaymentApplication {
    pub fn total_applied(&self) -> Money {
        self.to_fees + self.to_penalties + self.to_interest + self.to_principal
    }
}

/// deficiency balance after liquidation
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeficiencyBalance {
    pub original_loan_id: FacilityId,
    pub liquidation_date: DateTime<Utc>,
    pub collateral_proceeds: Money,
    pub remaining_debt: Money,
    pub recovery_status: RecoveryStatus,
}

/// recovery status for deficiency
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryStatus {
    Pursuing,
    PaymentPlan,
    LegalAction,
    WrittenOff,
}