/// serialization support for facilities
use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};
use crate::decimal::{Money, Rate};
use crate::types::{FacilityStatus, FacilityId};
use crate::facility::Facility;
use rust_decimal::Decimal;

/// serializable view of a facility's state
#[derive(Debug, Serialize, Deserialize)]
pub struct FacilityView {
    pub id: FacilityId,
    pub account_number: String,
    pub customer_id: String,
    pub status: FacilityStatus,
    pub origination_date: DateTime<Utc>,
    pub activation_date: Option<DateTime<Utc>>,
    pub financial: FinancialView,
    pub payments: PaymentView,
    pub metadata: MetadataView,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct FinancialView {
    pub original_commitment: Money,
    pub outstanding_principal: Money,
    pub accrued_interest: Money,
    pub accrued_fees: Money,
    pub accrued_penalties: Money,
    pub total_outstanding: Money,
    pub total_disbursed: Money,
    pub interest_rate: Rate,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PaymentView {
    pub total_principal_paid: Money,
    pub total_interest_paid: Money,
    pub total_fees_paid: Money,
    pub last_payment_date: Option<DateTime<Utc>>,
    pub last_payment_amount: Option<Money>,
    pub next_payment_due: Option<DateTime<Utc>>,
    pub next_payment_amount: Option<Money>,
    pub payment_count: u32,
    pub missed_payment_count: u32,
    pub days_past_due: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MetadataView {
    pub facility_type: String,
    pub term_months: Option<u32>,
    pub interest_calculation_method: String,
    pub day_count_convention: String,
}

impl FacilityView {
    pub fn from_facility(facility: &Facility) -> Self {
        FacilityView {
            id: facility.id,
            account_number: facility.state.account_number.clone(),
            customer_id: facility.state.customer_id.clone(),
            status: facility.state.status,
            origination_date: facility.state.origination_date,
            activation_date: facility.state.activation_date,
            financial: FinancialView {
                original_commitment: facility.state.original_commitment,
                outstanding_principal: facility.state.outstanding_principal,
                accrued_interest: facility.state.accrued_interest,
                accrued_fees: facility.state.accrued_fees,
                accrued_penalties: facility.state.accrued_penalties,
                total_outstanding: facility.state.total_outstanding(),
                total_disbursed: facility.state.total_disbursed,
                interest_rate: facility.config.financial_terms.interest_rate,
            },
            payments: PaymentView {
                total_principal_paid: (facility.state.total_payments_received - 
                    (facility.state.total_interest_paid + facility.state.total_fees_charged)).max(Money::ZERO),
                total_interest_paid: facility.state.total_interest_paid,
                total_fees_paid: facility.state.total_fees_paid,
                last_payment_date: facility.state.last_payment_date,
                last_payment_amount: facility.state.last_payment_amount,
                next_payment_due: facility.state.next_payment_due,
                next_payment_amount: facility.state.next_payment_amount,
                payment_count: facility.state.payment_count,
                missed_payment_count: facility.state.missed_payment_count,
                days_past_due: facility.state.days_past_due,
            },
            metadata: MetadataView {
                facility_type: format!("{:?}", facility.config.facility_type),
                term_months: facility.config.financial_terms.term_months,
                interest_calculation_method: "DecliningBalance".to_string(),
                day_count_convention: format!("{:?}", facility.config.interest_config.day_count_convention),
            },
        }
    }
    
    /// convert to pretty-printed json string
    pub fn to_json_pretty(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }
}

/// term loan specific view
#[derive(Debug, Serialize, Deserialize)]
pub struct TermLoanView {
    pub facility: FacilityView,
    pub current_payment_number: u32,
    pub has_amortization_schedule: bool,
}

/// revolving facility specific view
#[derive(Debug, Serialize, Deserialize)]
pub struct RevolvingView {
    pub facility: FacilityView,
    pub credit_limit: Money,
    pub available_credit: Money,
    pub utilization_rate: Rate,
    pub is_in_draw_period: bool,
}

/// open-term loan specific view
#[derive(Debug, Serialize, Deserialize)]
pub struct OpenTermView {
    pub facility: FacilityView,
    pub btc_collateral: Decimal,
    pub btc_price: Money,
    pub collateral_value: Money,
    pub ltv_ratio: Rate,
    pub margin_call_active: bool,
}

/// overdraft specific view
#[derive(Debug, Serialize, Deserialize)]
pub struct OverdraftView {
    pub facility: FacilityView,
    pub overdraft_limit: Money,
    pub buffer_zone: Money,
    pub linked_account_balance: Money,
    pub is_active: bool,
    pub available_funds: Money,
}