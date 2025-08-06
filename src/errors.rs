use thiserror::Error;
use uuid::Uuid;

use crate::decimal::{Money, Rate};
use crate::types::FacilityStatus;

#[derive(Error, Debug)]
pub enum FacilityError {
    #[error("insufficient funds: available {available}, requested {requested}")]
    InsufficientFunds {
        available: Money,
        requested: Money,
    },
    
    #[error("ltv breach: {ltv} exceeds threshold {threshold}")]
    LtvBreach {
        ltv: Rate,
        threshold: Rate,
    },
    
    #[error("invalid payment amount: {amount}")]
    InvalidPaymentAmount {
        amount: Money,
    },
    
    #[error("facility not active: current status is {status:?}")]
    FacilityNotActive {
        status: FacilityStatus,
    },
    
    #[error("overdraft limit exceeded: limit {limit}, requested {requested}")]
    OverdraftLimitExceeded {
        limit: Money,
        requested: Money,
    },
    
    #[error("credit limit exceeded: limit {limit}, requested {requested}")]
    CreditLimitExceeded {
        limit: Money,
        requested: Money,
    },
    
    #[error("milestone not found: {id}")]
    MilestoneNotFound {
        id: Uuid,
    },
    
    #[error("milestone not approved for disbursement: {name}")]
    MilestoneNotApproved {
        name: String,
    },
    
    #[error("invalid configuration: {message}")]
    InvalidConfiguration {
        message: String,
    },
    
    #[error("liquidation in progress")]
    LiquidationInProgress,
    
    #[error("payment schedule not applicable for facility type")]
    PaymentScheduleNotApplicable,
    
    #[error("collateral required for this operation")]
    CollateralRequired,
    
    #[error("no collateral associated with facility")]
    NoCollateral,
    
    #[error("invalid date: {message}")]
    InvalidDate {
        message: String,
    },
    
    #[error("calculation error: {message}")]
    CalculationError {
        message: String,
    },
    
    #[error("facility already settled")]
    FacilityAlreadySettled,
    
    #[error("facility already charged off")]
    FacilityChargedOff,
    
    #[error("invalid interest rate: {rate}")]
    InvalidInterestRate {
        rate: Rate,
    },
    
    #[error("payment less than minimum: minimum {minimum}, provided {provided}")]
    PaymentBelowMinimum {
        minimum: Money,
        provided: Money,
    },
    
    #[error("operation not supported for facility type")]
    OperationNotSupported,
    
    #[error("draw period has ended")]
    DrawPeriodEnded,
    
    #[error("invalid draw amount: {amount}")]
    InvalidDrawAmount {
        amount: Money,
    },
    
    #[error("below minimum drawdown: minimum {minimum}, requested {requested}")]
    BelowMinimumDrawdown {
        minimum: Money,
        requested: Money,
    },
    
    #[error("exceeds credit limit: available {available}, requested {requested}")]
    ExceedsCreditLimit {
        available: Money,
        requested: Money,
    },
    
    #[error("invalid collateral: {message}")]
    InvalidCollateral {
        message: String,
    },
    
    #[error("insufficient collateral: available {available}, required {required}")]
    InsufficientCollateral {
        available: rust_decimal::Decimal,
        required: rust_decimal::Decimal,
    },
    
    #[error("margin call expired: deadline {deadline}, current time {current_time}")]
    MarginCallExpired {
        deadline: chrono::DateTime<chrono::Utc>,
        current_time: chrono::DateTime<chrono::Utc>,
    },
    
    #[error("invalid state: current {current}, expected {expected}")]
    InvalidState {
        current: String,
        expected: String,
    },
}

pub type Result<T> = std::result::Result<T, FacilityError>;