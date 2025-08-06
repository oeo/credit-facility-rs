use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use crate::decimal::{Money, Rate};
use crate::interest::{CompoundingFrequency, DayCountConvention, PenaltyConfig};
use crate::payments::PartialPaymentStrategy;
use crate::types::{AmortizationMethod, LtvThresholds, OpenTermType, OverpaymentStrategy, PaymentSchedule, RevolvingType, TermLoanType};

/// facility configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FacilityConfig {
    pub facility_type: FacilityType,
    pub financial_terms: FinancialTerms,
    pub interest_config: InterestConfig,
    pub payment_config: PaymentConfig,
    pub fee_config: FeeConfig,
    pub collateral_config: Option<CollateralConfig>,
    pub limits: FacilityLimits,
}

/// facility type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FacilityType {
    TermLoan(TermLoanType),
    OpenTermLoan(OpenTermType),
    Revolving(RevolvingType),
    Overdraft,
}


/// financial terms
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FinancialTerms {
    pub commitment_amount: Money,
    pub interest_rate: Rate,
    pub term_months: Option<u32>,
    pub origination_date: DateTime<Utc>,
    pub first_payment_date: Option<DateTime<Utc>>,
    pub maturity_date: Option<DateTime<Utc>>,
    pub amortization_method: AmortizationMethod,
    pub balloon_payment: Option<Money>,
}

/// interest configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterestConfig {
    pub day_count_convention: DayCountConvention,
    pub compounding_frequency: CompoundingFrequency,
    pub penalty_config: Option<PenaltyConfig>,
    pub grace_period_days: u32,
    pub default_rate: Option<Rate>,
    pub variable_rate: bool,
    pub rate_index: Option<String>,
    pub margin: Option<Rate>,
}

/// payment configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PaymentConfig {
    pub payment_schedule: PaymentSchedule,
    pub minimum_payment: Option<Money>,
    pub minimum_payment_percentage: Option<Decimal>,
    pub overpayment_allowed: bool,
    pub overpayment_strategy: OverpaymentStrategy,
    pub partial_payment_strategy: PartialPaymentStrategy,
    pub autopay_enabled: bool,
}

/// fee configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeeConfig {
    pub origination_fee: Option<Money>,
    pub origination_fee_percentage: Option<Decimal>,
    pub late_fee: Option<Money>,
    pub late_fee_percentage: Option<Decimal>,
    pub overlimit_fee: Option<Money>,
    pub annual_fee: Option<Money>,
    pub prepayment_penalty: Option<PrepaymentPenalty>,
    pub commitment_fee_rate: Option<Rate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrepaymentPenalty {
    pub penalty_percentage: Decimal,
    pub penalty_months: u32,
    pub step_down: Vec<StepDown>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StepDown {
    pub after_months: u32,
    pub penalty_percentage: Decimal,
}

/// collateral configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollateralConfig {
    pub collateral_type: String,
    pub initial_value: Money,
    pub ltv_thresholds: LtvThresholds,
    pub revaluation_frequency_days: u32,
    pub liquidation_notice_days: u32,
    pub margin_call_days: u32,
}

/// facility limits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FacilityLimits {
    pub minimum_drawdown: Option<Money>,
    pub maximum_drawdown: Option<Money>,
    pub minimum_payment: Option<Money>,
    pub maximum_payment: Option<Money>,
    pub credit_limit: Option<Money>,
    pub overdraft_limit: Option<Money>,
    pub daily_transaction_limit: Option<Money>,
    pub monthly_transaction_limit: Option<Money>,
}

impl FacilityConfig {
    /// create mortgage configuration
    pub fn mortgage(
        amount: Money,
        rate: Rate,
        term_months: u32,
        origination_date: DateTime<Utc>,
        property_value: Money,
    ) -> Self {
        let ltv = (amount.as_decimal() / property_value.as_decimal() * dec!(100))
            .round_dp(2);
        
        Self {
            facility_type: FacilityType::TermLoan(TermLoanType::Mortgage),
            financial_terms: FinancialTerms {
                commitment_amount: amount,
                interest_rate: rate,
                term_months: Some(term_months),
                origination_date,
                first_payment_date: Some(origination_date + chrono::Duration::days(30)),
                maturity_date: Some(origination_date + chrono::Duration::days((term_months * 30) as i64)),
                amortization_method: AmortizationMethod::EqualInstallments,
                balloon_payment: None,
            },
            interest_config: InterestConfig {
                day_count_convention: DayCountConvention::Actual360,
                compounding_frequency: CompoundingFrequency::Monthly,
                penalty_config: Some(PenaltyConfig::new(rate, dec!(1.5))),
                grace_period_days: 15,
                default_rate: Some(Rate::from_decimal(rate.as_decimal() * dec!(1.5))),
                variable_rate: false,
                rate_index: None,
                margin: None,
            },
            payment_config: PaymentConfig {
                payment_schedule: PaymentSchedule::Monthly { day_of_month: 1 },
                minimum_payment: None,
                minimum_payment_percentage: None,
                overpayment_allowed: true,
                overpayment_strategy: OverpaymentStrategy::ReducePrincipal,
                partial_payment_strategy: PartialPaymentStrategy::HoldInSuspense,
                autopay_enabled: false,
            },
            fee_config: FeeConfig {
                origination_fee: Some(Money::from_decimal(amount.as_decimal() * dec!(0.01))),
                origination_fee_percentage: Some(dec!(1.0)),
                late_fee: Some(Money::from_major(50)),
                late_fee_percentage: None,
                overlimit_fee: None,
                annual_fee: None,
                prepayment_penalty: if term_months > 60 {
                    Some(PrepaymentPenalty {
                        penalty_percentage: dec!(2.0),
                        penalty_months: 24,
                        step_down: vec![
                            StepDown { after_months: 12, penalty_percentage: dec!(1.0) },
                            StepDown { after_months: 24, penalty_percentage: dec!(0) },
                        ],
                    })
                } else {
                    None
                },
                commitment_fee_rate: None,
            },
            collateral_config: Some(CollateralConfig {
                collateral_type: "real_estate".to_string(),
                initial_value: property_value,
                ltv_thresholds: LtvThresholds {
                    initial_ltv: Rate::from_decimal(ltv / dec!(100)),
                    warning_ltv: Rate::from_percentage(80),
                    margin_call_ltv: Rate::from_percentage(85),
                    liquidation_ltv: Rate::from_percentage(90),
                },
                revaluation_frequency_days: 365,
                liquidation_notice_days: 90,
                margin_call_days: 30,
            }),
            limits: FacilityLimits {
                minimum_drawdown: None,
                maximum_drawdown: Some(amount),
                minimum_payment: None,
                maximum_payment: None,
                credit_limit: None,
                overdraft_limit: None,
                daily_transaction_limit: None,
                monthly_transaction_limit: None,
            },
        }
    }
    
    /// create personal loan configuration
    pub fn personal_loan(
        amount: Money,
        rate: Rate,
        term_months: u32,
        origination_date: DateTime<Utc>,
    ) -> Self {
        Self {
            facility_type: FacilityType::TermLoan(TermLoanType::PersonalLoan),
            financial_terms: FinancialTerms {
                commitment_amount: amount,
                interest_rate: rate,
                term_months: Some(term_months),
                origination_date,
                first_payment_date: Some(origination_date + chrono::Duration::days(30)),
                maturity_date: Some(origination_date + chrono::Duration::days((term_months * 30) as i64)),
                amortization_method: AmortizationMethod::EqualInstallments,
                balloon_payment: None,
            },
            interest_config: InterestConfig {
                day_count_convention: DayCountConvention::Actual365,
                compounding_frequency: CompoundingFrequency::Monthly,
                penalty_config: Some(PenaltyConfig::new(rate, dec!(2.0))),
                grace_period_days: 10,
                default_rate: Some(Rate::from_decimal(rate.as_decimal() * dec!(2.0))),
                variable_rate: false,
                rate_index: None,
                margin: None,
            },
            payment_config: PaymentConfig {
                payment_schedule: PaymentSchedule::Monthly { day_of_month: 15 },
                minimum_payment: None,
                minimum_payment_percentage: None,
                overpayment_allowed: true,
                overpayment_strategy: OverpaymentStrategy::ReduceTerm,
                partial_payment_strategy: PartialPaymentStrategy::ApplyImmediately,
                autopay_enabled: false,
            },
            fee_config: FeeConfig {
                origination_fee: Some(Money::from_decimal(amount.as_decimal() * dec!(0.03))),
                origination_fee_percentage: Some(dec!(3.0)),
                late_fee: Some(Money::from_major(35)),
                late_fee_percentage: None,
                overlimit_fee: None,
                annual_fee: None,
                prepayment_penalty: None,
                commitment_fee_rate: None,
            },
            collateral_config: None,
            limits: FacilityLimits {
                minimum_drawdown: Some(Money::from_major(1000)),
                maximum_drawdown: Some(Money::from_major(50000)),
                minimum_payment: None,
                maximum_payment: None,
                credit_limit: None,
                overdraft_limit: None,
                daily_transaction_limit: None,
                monthly_transaction_limit: None,
            },
        }
    }
    
    /// create auto loan configuration
    pub fn auto_loan(
        amount: Money,
        rate: Rate,
        term_months: u32,
        origination_date: DateTime<Utc>,
        vehicle_value: Money,
        balloon_percentage: Option<Decimal>,
    ) -> Self {
        let balloon_payment = balloon_percentage
            .map(|pct| Money::from_decimal(amount.as_decimal() * pct / dec!(100)));
        
        Self {
            facility_type: FacilityType::TermLoan(TermLoanType::AutoLoan),
            financial_terms: FinancialTerms {
                commitment_amount: amount,
                interest_rate: rate,
                term_months: Some(term_months),
                origination_date,
                first_payment_date: Some(origination_date + chrono::Duration::days(45)),
                maturity_date: Some(origination_date + chrono::Duration::days((term_months * 30) as i64)),
                amortization_method: if balloon_payment.is_some() {
                    AmortizationMethod::InterestOnly
                } else {
                    AmortizationMethod::EqualInstallments
                },
                balloon_payment,
            },
            interest_config: InterestConfig {
                day_count_convention: DayCountConvention::Actual365,
                compounding_frequency: CompoundingFrequency::Monthly,
                penalty_config: Some(PenaltyConfig::new(rate, dec!(1.5))),
                grace_period_days: 10,
                default_rate: Some(Rate::from_decimal(rate.as_decimal() * dec!(1.5))),
                variable_rate: false,
                rate_index: None,
                margin: None,
            },
            payment_config: PaymentConfig {
                payment_schedule: PaymentSchedule::Monthly { day_of_month: 1 },
                minimum_payment: None,
                minimum_payment_percentage: None,
                overpayment_allowed: true,
                overpayment_strategy: OverpaymentStrategy::ReducePrincipal,
                partial_payment_strategy: PartialPaymentStrategy::HoldInSuspense,
                autopay_enabled: false,
            },
            fee_config: FeeConfig {
                origination_fee: Some(Money::from_major(500)),
                origination_fee_percentage: None,
                late_fee: Some(Money::from_major(25)),
                late_fee_percentage: None,
                overlimit_fee: None,
                annual_fee: None,
                prepayment_penalty: None,
                commitment_fee_rate: None,
            },
            collateral_config: Some(CollateralConfig {
                collateral_type: "vehicle".to_string(),
                initial_value: vehicle_value,
                ltv_thresholds: LtvThresholds {
                    initial_ltv: Rate::from_decimal(amount.as_decimal() / vehicle_value.as_decimal()),
                    warning_ltv: Rate::from_percentage(100),
                    margin_call_ltv: Rate::from_percentage(110),
                    liquidation_ltv: Rate::from_percentage(120),
                },
                revaluation_frequency_days: 90,
                liquidation_notice_days: 30,
                margin_call_days: 10,
            }),
            limits: FacilityLimits {
                minimum_drawdown: None,
                maximum_drawdown: Some(amount),
                minimum_payment: None,
                maximum_payment: None,
                credit_limit: None,
                overdraft_limit: None,
                daily_transaction_limit: None,
                monthly_transaction_limit: None,
            },
        }
    }
    
    /// create bitcoin-backed open-term loan configuration
    pub fn bitcoin_backed_loan(
        amount: Money,
        rate: Rate,
        btc_collateral: Decimal,
        btc_price: Money,
    ) -> Self {
        let collateral_value = Money::from_decimal(btc_collateral * btc_price.as_decimal());
        let initial_ltv = amount.as_decimal() / collateral_value.as_decimal();
        
        Self {
            facility_type: FacilityType::OpenTermLoan(OpenTermType::BitcoinBacked),
            financial_terms: FinancialTerms {
                commitment_amount: amount,
                interest_rate: rate,
                term_months: None, // perpetual
                origination_date: Utc::now(),
                first_payment_date: None, // no scheduled payments
                maturity_date: None, // no maturity
                amortization_method: AmortizationMethod::InterestOnly,
                balloon_payment: None,
            },
            interest_config: InterestConfig {
                day_count_convention: DayCountConvention::Actual365,
                compounding_frequency: CompoundingFrequency::Daily,
                penalty_config: None, // no penalties for open-term
                grace_period_days: 0, // no grace period needed
                default_rate: None, // ltv-based liquidation instead
                variable_rate: false,
                rate_index: None,
                margin: None,
            },
            payment_config: PaymentConfig {
                payment_schedule: PaymentSchedule::None, // no required payments
                minimum_payment: None,
                minimum_payment_percentage: None,
                overpayment_allowed: true,
                overpayment_strategy: OverpaymentStrategy::ReducePrincipal,
                partial_payment_strategy: PartialPaymentStrategy::ApplyImmediately,
                autopay_enabled: false,
            },
            fee_config: FeeConfig {
                origination_fee: Some(Money::from_decimal(amount.as_decimal() * dec!(0.01))),
                origination_fee_percentage: Some(dec!(1.0)),
                late_fee: None, // no late fees for open-term
                late_fee_percentage: None,
                overlimit_fee: None,
                annual_fee: None,
                prepayment_penalty: None,
                commitment_fee_rate: None,
            },
            collateral_config: Some(CollateralConfig {
                collateral_type: "BTC".to_string(),
                initial_value: collateral_value,
                ltv_thresholds: LtvThresholds {
                    initial_ltv: Rate::from_decimal(initial_ltv),
                    warning_ltv: Rate::from_percentage(60),
                    margin_call_ltv: Rate::from_percentage(65),
                    liquidation_ltv: Rate::from_percentage(75),
                },
                revaluation_frequency_days: 1, // daily btc price updates
                liquidation_notice_days: 0, // immediate liquidation at threshold
                margin_call_days: 3, // 3 days to respond to margin call
            }),
            limits: FacilityLimits {
                minimum_drawdown: None,
                maximum_drawdown: Some(amount),
                minimum_payment: None,
                maximum_payment: None,
                credit_limit: None,
                overdraft_limit: None,
                daily_transaction_limit: None,
                monthly_transaction_limit: None,
            },
        }
    }
    
    /// create credit card configuration
    pub fn credit_card(
        credit_limit: Money,
        rate: Rate,
        minimum_percentage: Decimal,
    ) -> Self {
        Self {
            facility_type: FacilityType::Revolving(RevolvingType::CreditCard),
            financial_terms: FinancialTerms {
                commitment_amount: credit_limit,
                interest_rate: rate,
                term_months: None, // revolving
                origination_date: Utc::now(),
                first_payment_date: None,
                maturity_date: None,
                amortization_method: AmortizationMethod::InterestOnly,
                balloon_payment: None,
            },
            interest_config: InterestConfig {
                day_count_convention: DayCountConvention::Actual365,
                compounding_frequency: CompoundingFrequency::Daily,
                penalty_config: Some(PenaltyConfig::new(rate, dec!(1.5))),
                grace_period_days: 5,
                default_rate: Some(Rate::from_percentage(29)),
                variable_rate: false,
                rate_index: None,
                margin: None,
            },
            payment_config: PaymentConfig {
                payment_schedule: PaymentSchedule::Monthly { day_of_month: 1 },
                minimum_payment: Some(Money::from_major(25)),
                minimum_payment_percentage: Some(minimum_percentage),
                overpayment_allowed: true,
                overpayment_strategy: OverpaymentStrategy::ReducePrincipal,
                partial_payment_strategy: PartialPaymentStrategy::ApplyImmediately,
                autopay_enabled: false,
            },
            fee_config: FeeConfig {
                origination_fee: None,
                origination_fee_percentage: None,
                late_fee: Some(Money::from_major(39)),
                late_fee_percentage: None,
                overlimit_fee: Some(Money::from_major(35)),
                annual_fee: Some(Money::from_major(95)),
                prepayment_penalty: None,
                commitment_fee_rate: None,
            },
            collateral_config: None,
            limits: FacilityLimits {
                minimum_drawdown: Some(Money::from_major(20)),
                maximum_drawdown: None,
                minimum_payment: Some(Money::from_major(25)),
                maximum_payment: None,
                credit_limit: Some(credit_limit),
                overdraft_limit: None,
                daily_transaction_limit: Some(Money::from_major(5000)),
                monthly_transaction_limit: None,
            },
        }
    }
    
    /// create business line of credit configuration
    pub fn line_of_credit(
        credit_limit: Money,
        rate: Rate,
        commitment_fee_rate: Rate,
    ) -> Self {
        Self {
            facility_type: FacilityType::Revolving(RevolvingType::LineOfCredit),
            financial_terms: FinancialTerms {
                commitment_amount: credit_limit,
                interest_rate: rate,
                term_months: Some(12), // annual review
                origination_date: Utc::now(),
                first_payment_date: None,
                maturity_date: None,
                amortization_method: AmortizationMethod::InterestOnly,
                balloon_payment: None,
            },
            interest_config: InterestConfig {
                day_count_convention: DayCountConvention::Actual360,
                compounding_frequency: CompoundingFrequency::Monthly,
                penalty_config: Some(PenaltyConfig::new(rate, dec!(2.0))),
                grace_period_days: 10,
                default_rate: Some(Rate::from_decimal(rate.as_decimal() + dec!(0.05))),
                variable_rate: true,
                rate_index: Some("SOFR".to_string()),
                margin: Some(Rate::from_percentage(3)),
            },
            payment_config: PaymentConfig {
                payment_schedule: PaymentSchedule::Monthly { day_of_month: 15 },
                minimum_payment: None,
                minimum_payment_percentage: Some(dec!(0)), // interest only
                overpayment_allowed: true,
                overpayment_strategy: OverpaymentStrategy::ReducePrincipal,
                partial_payment_strategy: PartialPaymentStrategy::HoldInSuspense,
                autopay_enabled: false,
            },
            fee_config: FeeConfig {
                origination_fee: Some(Money::from_decimal(credit_limit.as_decimal() * dec!(0.005))),
                origination_fee_percentage: Some(dec!(0.5)),
                late_fee: Some(Money::from_major(50)),
                late_fee_percentage: None,
                overlimit_fee: None,
                annual_fee: None,
                prepayment_penalty: None,
                commitment_fee_rate: Some(commitment_fee_rate),
            },
            collateral_config: None,
            limits: FacilityLimits {
                minimum_drawdown: Some(Money::from_major(10000)),
                maximum_drawdown: None,
                minimum_payment: None,
                maximum_payment: None,
                credit_limit: Some(credit_limit),
                overdraft_limit: None,
                daily_transaction_limit: None,
                monthly_transaction_limit: None,
            },
        }
    }
    
    /// create heloc configuration
    pub fn heloc(
        credit_limit: Money,
        rate: Rate,
        property_value: Money,
        draw_period_months: u32,
        repayment_period_months: u32,
    ) -> Self {
        let ltv = credit_limit.as_decimal() / property_value.as_decimal();
        
        Self {
            facility_type: FacilityType::Revolving(RevolvingType::HELOC),
            financial_terms: FinancialTerms {
                commitment_amount: credit_limit,
                interest_rate: rate,
                term_months: Some(draw_period_months + repayment_period_months),
                origination_date: Utc::now(),
                first_payment_date: None,
                maturity_date: None,
                amortization_method: AmortizationMethod::InterestOnly, // during draw period
                balloon_payment: None,
            },
            interest_config: InterestConfig {
                day_count_convention: DayCountConvention::Actual365,
                compounding_frequency: CompoundingFrequency::Monthly,
                penalty_config: Some(PenaltyConfig::new(rate, dec!(1.5))),
                grace_period_days: 15,
                default_rate: Some(Rate::from_decimal(rate.as_decimal() * dec!(1.5))),
                variable_rate: true,
                rate_index: Some("Prime".to_string()),
                margin: Some(Rate::from_percentage(1)),
            },
            payment_config: PaymentConfig {
                payment_schedule: PaymentSchedule::Monthly { day_of_month: 1 },
                minimum_payment: None,
                minimum_payment_percentage: Some(dec!(0)), // interest only during draw
                overpayment_allowed: true,
                overpayment_strategy: OverpaymentStrategy::ReducePrincipal,
                partial_payment_strategy: PartialPaymentStrategy::HoldInSuspense,
                autopay_enabled: false,
            },
            fee_config: FeeConfig {
                origination_fee: Some(Money::from_major(500)),
                origination_fee_percentage: None,
                late_fee: Some(Money::from_major(35)),
                late_fee_percentage: None,
                overlimit_fee: None,
                annual_fee: Some(Money::from_major(75)),
                prepayment_penalty: None,
                commitment_fee_rate: None,
            },
            collateral_config: Some(CollateralConfig {
                collateral_type: "real_estate".to_string(),
                initial_value: property_value,
                ltv_thresholds: LtvThresholds {
                    initial_ltv: Rate::from_decimal(ltv),
                    warning_ltv: Rate::from_percentage(80),
                    margin_call_ltv: Rate::from_percentage(85),
                    liquidation_ltv: Rate::from_percentage(90),
                },
                revaluation_frequency_days: 365,
                liquidation_notice_days: 90,
                margin_call_days: 30,
            }),
            limits: FacilityLimits {
                minimum_drawdown: Some(Money::from_major(500)),
                maximum_drawdown: None,
                minimum_payment: None,
                maximum_payment: None,
                credit_limit: Some(credit_limit),
                overdraft_limit: None,
                daily_transaction_limit: None,
                monthly_transaction_limit: None,
            },
        }
    }
    
    /// create overdraft configuration
    pub fn overdraft(
        overdraft_limit: Money,
        rate: Rate,
        _buffer_zone: Money,
        _linked_account_id: String,
    ) -> Self {
        Self {
            facility_type: FacilityType::Overdraft,
            financial_terms: FinancialTerms {
                commitment_amount: overdraft_limit,
                interest_rate: rate,
                term_months: None, // no term for overdraft
                origination_date: Utc::now(),
                first_payment_date: None,
                maturity_date: None,
                amortization_method: AmortizationMethod::InterestOnly,
                balloon_payment: None,
            },
            interest_config: InterestConfig {
                day_count_convention: DayCountConvention::Actual365,
                compounding_frequency: CompoundingFrequency::Continuous,
                penalty_config: None, // no penalties for overdraft
                grace_period_days: 0, // no grace period
                default_rate: None,
                variable_rate: false,
                rate_index: None,
                margin: None,
            },
            payment_config: PaymentConfig {
                payment_schedule: PaymentSchedule::None, // no scheduled payments
                minimum_payment: None,
                minimum_payment_percentage: None,
                overpayment_allowed: true,
                overpayment_strategy: OverpaymentStrategy::ReducePrincipal,
                partial_payment_strategy: PartialPaymentStrategy::ApplyImmediately,
                autopay_enabled: false,
            },
            fee_config: FeeConfig {
                origination_fee: None,
                origination_fee_percentage: None,
                late_fee: None, // no late fees for overdraft
                late_fee_percentage: None,
                overlimit_fee: Some(Money::from_major(35)),
                annual_fee: None,
                prepayment_penalty: None,
                commitment_fee_rate: None,
            },
            collateral_config: None,
            limits: FacilityLimits {
                minimum_drawdown: None,
                maximum_drawdown: None,
                minimum_payment: None,
                maximum_payment: None,
                credit_limit: None,
                overdraft_limit: Some(overdraft_limit),
                daily_transaction_limit: None,
                monthly_transaction_limit: None,
            },
        }
    }
}