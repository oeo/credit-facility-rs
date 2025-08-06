use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};

use crate::decimal::{Money, Rate};
use crate::types::{FacilityId, FacilityStatus, OverpaymentStrategy};
use rust_decimal::Decimal;

/// all events that can be emitted by the facility
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Event {
    // lifecycle events
    FacilityOriginated {
        facility_id: FacilityId,
        amount: Money,
        collateral_type: String,
        collateral_amount: Decimal,
    },
    FacilityActivated {
        facility_id: FacilityId,
        first_disbursement: Money,
        timestamp: DateTime<Utc>,
    },
    FacilityMatured {
        facility_id: FacilityId,
        final_payment: Money,
        timestamp: DateTime<Utc>,
    },
    FacilitySettled {
        facility_id: FacilityId,
        settlement_amount: Money,
        timestamp: DateTime<Utc>,
    },
    FacilityChargedOff {
        facility_id: FacilityId,
        loss_amount: Money,
        timestamp: DateTime<Utc>,
    },

    // payment events
    PaymentDue {
        facility_id: FacilityId,
        amount: Money,
        due_date: NaiveDate,
        principal_portion: Money,
        interest_portion: Money,
    },
    PaymentReceived {
        facility_id: FacilityId,
        amount: Money,
        applied_to_fees: Money,
        applied_to_interest: Money,
        applied_to_principal: Money,
        timestamp: DateTime<Utc>,
    },
    PaymentMissed {
        facility_id: FacilityId,
        expected_amount: Money,
        due_date: NaiveDate,
    },
    OverpaymentReceived {
        facility_id: FacilityId,
        amount: Money,
        strategy: OverpaymentStrategy,
        timestamp: DateTime<Utc>,
    },

    // interest events
    InterestAccrued {
        facility_id: FacilityId,
        amount: Money,
        timestamp: DateTime<Utc>,
    },
    InterestCapitalized {
        facility_id: FacilityId,
        amount: Money,
        new_principal: Money,
        reason: String,
        timestamp: DateTime<Utc>,
    },
    InterestRateChanged {
        facility_id: FacilityId,
        old_rate: Rate,
        new_rate: Rate,
        reason: String,
        timestamp: DateTime<Utc>,
    },
    PenaltyInterestApplied {
        facility_id: FacilityId,
        amount: Money,
        days_overdue: u32,
        timestamp: DateTime<Utc>,
    },

    // revolving events
    FundsDrawn {
        facility_id: FacilityId,
        amount: Money,
        new_outstanding: Money,
        available_credit: Money,
        timestamp: DateTime<Utc>,
    },
    CreditLimitChanged {
        facility_id: FacilityId,
        old_limit: Money,
        new_limit: Money,
        timestamp: DateTime<Utc>,
    },
    OverlimitOccurred {
        facility_id: FacilityId,
        amount_over: Money,
        fees_applied: Money,
        timestamp: DateTime<Utc>,
    },
    CommitmentFeeCharged {
        facility_id: FacilityId,
        undrawn_amount: Money,
        fee: Money,
        timestamp: DateTime<Utc>,
    },

    // overdraft events
    OverdraftActivated {
        facility_id: FacilityId,
        account_balance: Money,
        overdraft_amount: Money,
        timestamp: DateTime<Utc>,
    },
    OverdraftIncreased {
        facility_id: FacilityId,
        additional_amount: Money,
        new_total: Money,
        timestamp: DateTime<Utc>,
    },
    BufferZoneBreached {
        facility_id: FacilityId,
        amount_over_buffer: Money,
        timestamp: DateTime<Utc>,
    },
    OverdraftCleared {
        facility_id: FacilityId,
        repayment_amount: Money,
        timestamp: DateTime<Utc>,
    },

    // collateral events
    CollateralValueUpdated {
        facility_id: FacilityId,
        old_value: Money,
        new_value: Money,
        source: String,
        timestamp: DateTime<Utc>,
    },
    LtvCalculated {
        facility_id: FacilityId,
        ltv_ratio: Rate,
        collateral_value: Money,
        total_debt: Money,
        timestamp: DateTime<Utc>,
    },
    LtvWarningBreached {
        facility_id: FacilityId,
        ltv_ratio: Rate,
        threshold: Rate,
        timestamp: DateTime<Utc>,
    },
    MarginCallRequired {
        facility_id: FacilityId,
        current_ltv: Rate,
        required_ltv: Rate,
        deadline: DateTime<Utc>,
        options: Vec<String>,
    },
    MarginCallResolved {
        facility_id: FacilityId,
        new_ltv: Rate,
        timestamp: DateTime<Utc>,
    },
    CollateralAdded {
        facility_id: FacilityId,
        amount: Decimal,
        new_total: Decimal,
        new_ltv: Rate,
        timestamp: DateTime<Utc>,
    },
    CollateralReleased {
        facility_id: FacilityId,
        collateral_type: String,
        amount: Decimal,
        timestamp: DateTime<Utc>,
    },
    LtvLiquidationBreached {
        facility_id: FacilityId,
        ltv_ratio: Rate,
        threshold: Rate,
        timestamp: DateTime<Utc>,
    },

    // grace period events
    GracePeriodStarted {
        facility_id: FacilityId,
        payment_due_date: NaiveDate,
        grace_ends_at: NaiveDate,
        timestamp: DateTime<Utc>,
    },
    GracePeriodReminder {
        facility_id: FacilityId,
        days_remaining: u32,
        timestamp: DateTime<Utc>,
    },
    GracePeriodExpired {
        facility_id: FacilityId,
        days_overdue: u32,
        timestamp: DateTime<Utc>,
    },
    LateFeeApplied {
        facility_id: FacilityId,
        fee_amount: Money,
        days_overdue: u32,
        timestamp: DateTime<Utc>,
    },

    // liquidation events
    LiquidationTriggered {
        facility_id: FacilityId,
        ltv_ratio: Rate,
        collateral_value: Money,
        debt_amount: Money,
        timestamp: DateTime<Utc>,
    },
    LiquidationPending {
        facility_id: FacilityId,
        notice_period_ends: DateTime<Utc>,
        timestamp: DateTime<Utc>,
    },
    CollateralSaleInitiated {
        facility_id: FacilityId,
        method: String,
        expected_proceeds: Money,
        timestamp: DateTime<Utc>,
    },
    LiquidationCompleted {
        facility_id: FacilityId,
        proceeds: Money,
        remaining_debt: Money,
        timestamp: DateTime<Utc>,
    },
    DeficiencyBalance {
        facility_id: FacilityId,
        amount: Money,
        recovery_action: String,
        timestamp: DateTime<Utc>,
    },

    // status change events
    StatusChanged {
        facility_id: FacilityId,
        old_status: FacilityStatus,
        new_status: FacilityStatus,
        reason: String,
        timestamp: DateTime<Utc>,
    },
}

/// event store for collecting events during operations
#[derive(Debug, Default)]
pub struct EventStore {
    events: Vec<Event>,
}

impl EventStore {
    pub fn new() -> Self {
        Self {
            events: Vec::new(),
        }
    }

    pub fn emit(&mut self, event: Event) {
        self.events.push(event);
    }

    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub fn clear(&mut self) {
        self.events.clear();
    }
}
