use hourglass_rs::SafeTimeProvider;
use uuid::Uuid;

use crate::config::FacilityConfig;
use crate::decimal::{Money, Rate};
use crate::errors::{FacilityError, Result};
use crate::events::{Event, EventStore};
use crate::interest::{AccrualEngine, DailyAccrual, PenaltyEngine};
use crate::payments::{
    PaymentContext, PaymentProcessor, PaymentRequest, PaymentResult, PaymentWaterfall,
};
use crate::state::{FacilityState, StateSnapshot};
use crate::types::{CollateralPosition, FacilityId, FacilityStatus};

/// core facility struct
pub struct Facility {
    pub id: FacilityId,
    pub config: FacilityConfig,
    pub state: FacilityState,
    pub events: EventStore,
    pub snapshots: Vec<StateSnapshot>,
}

impl Facility {
    /// create new facility
    pub fn new(config: FacilityConfig, state: FacilityState) -> Self {
        Self {
            id: state.facility_id,
            config,
            state,
            events: EventStore::new(),
            snapshots: Vec::new(),
        }
    }

    /// originate facility
    pub fn originate(
        config: FacilityConfig,
        account_number: String,
        customer_id: String,
        time_provider: &SafeTimeProvider,
    ) -> Result<Self> {
        let facility_id = Uuid::new_v4();
        let now = time_provider.now();

        let state_type = match &config.facility_type {
            crate::config::FacilityType::TermLoan(_) => {
                let payment = crate::payments::overpayment::calculate_emi(
                    config.financial_terms.commitment_amount,
                    config.financial_terms.interest_rate,
                    config.financial_terms.term_months.unwrap_or(0),
                );

                crate::state::FacilityStateType::TermLoan {
                    payment,
                    term: config.financial_terms.term_months.unwrap_or(0),
                    balloon: config.financial_terms.balloon_payment,
                }
            }
            crate::config::FacilityType::OpenTermLoan(_) => {
                crate::state::FacilityStateType::OpenTerm
            }
            crate::config::FacilityType::Revolving(_) => {
                crate::state::FacilityStateType::Revolving {
                    limit: config.limits.credit_limit.unwrap_or(Money::ZERO),
                }
            }
            crate::config::FacilityType::Overdraft => {
                crate::state::FacilityStateType::Overdraft {
                    limit: config.limits.overdraft_limit.unwrap_or(Money::ZERO),
                }
            }
        };

        let state = FacilityState::new(
            facility_id,
            account_number,
            customer_id,
            config.financial_terms.commitment_amount,
            now,
            state_type,
        );

        let mut facility = Self::new(config, state);

        // emit origination event
        facility.events.emit(Event::FacilityOriginated {
            facility_id,
            amount: facility.config.financial_terms.commitment_amount,
            collateral_type: String::new(),
            collateral_amount: rust_decimal::Decimal::ZERO,
        });

        // apply origination fee if configured
        if let Some(fee) = facility.config.fee_config.origination_fee {
            facility.state.accrued_fees += fee;
            facility.state.total_fees_charged += fee;
        }

        // capture initial snapshot
        facility.snapshots.push(StateSnapshot::capture(&facility.state, "origination".to_string()));

        Ok(facility)
    }

    /// disburse funds
    pub fn disburse(
        &mut self,
        amount: Money,
        time_provider: &SafeTimeProvider,
    ) -> Result<Money> {
        // validate
        if !self.state.can_disburse() {
            return Err(FacilityError::FacilityNotActive {
                status: self.state.status,
            });
        }

        if amount > self.state.available_commitment {
            return Err(FacilityError::InsufficientFunds {
                available: self.state.available_commitment,
                requested: amount,
            });
        }

        // record disbursement
        self.state.record_disbursement(amount);

        // emit event
        let now = time_provider.now();
        if self.state.total_disbursed == amount {
            // first disbursement
            self.events.emit(Event::FacilityActivated {
                facility_id: self.id,
                first_disbursement: amount,
                timestamp: now,
            });
        } else {
            self.events.emit(Event::FundsDrawn {
                facility_id: self.id,
                amount,
                new_outstanding: self.state.outstanding_principal,
                available_credit: self.state.available_commitment,
                timestamp: now,
            });
        }

        // snapshot state
        self.snapshots.push(StateSnapshot::capture(&self.state, format!("disbursement: {}", amount)));

        Ok(amount)
    }

    /// accrue interest with system time
    pub fn accrue_interest_now(&mut self) -> Result<Vec<DailyAccrual>> {
        let time = SafeTimeProvider::new(hourglass_rs::TimeSource::System);
        self.accrue_interest(&time)
    }

    /// accrue interest
    pub fn accrue_interest(&mut self, time_provider: &SafeTimeProvider) -> Result<Vec<DailyAccrual>> {
        let now = time_provider.now();

        // check if we should accrue
        if self.state.outstanding_principal.is_zero() {
            return Ok(Vec::new());
        }

        // create accrual engine
        let engine = AccrualEngine::new(self.config.interest_config.day_count_convention);

        // accrue daily interest
        let accruals = engine.accrue_daily(
            self.state.outstanding_principal,
            self.config.financial_terms.interest_rate,
            self.state.last_interest_accrual,
            time_provider,
        );

        // update state
        for accrual in &accruals {
            self.state.accrued_interest += accrual.interest_amount;

            // emit event
            self.events.emit(Event::InterestAccrued {
                facility_id: self.id,
                amount: accrual.interest_amount,
                timestamp: accrual.date,
            });
        }

        self.state.last_interest_accrual = now;

        Ok(accruals)
    }

    /// process payment with system time
    pub fn process_payment_now(&mut self, amount: Money) -> Result<PaymentResult> {
        let time = SafeTimeProvider::new(hourglass_rs::TimeSource::System);
        self.process_payment(amount, &time)
    }

    /// process payment
    pub fn process_payment(
        &mut self,
        amount: Money,
        time_provider: &SafeTimeProvider,
    ) -> Result<PaymentResult> {
        // validate
        if !self.state.can_accept_payment() {
            return Err(FacilityError::FacilityNotActive {
                status: self.state.status,
            });
        }

        // create payment context
        let mut context = PaymentContext {
            facility_id: self.id,
            accrued_fees: self.state.accrued_fees,
            accrued_penalties: self.state.accrued_penalties,
            accrued_interest: self.state.accrued_interest,
            outstanding_principal: self.state.outstanding_principal,
            minimum_payment: self.state.minimum_payment_due,
            payment_due_date: self.state.next_payment_due,
            days_overdue: self.state.days_past_due,
        };

        // create payment request
        let request = PaymentRequest {
            facility_id: self.id,
            amount,
            payment_date: time_provider.now(),
            reference: format!("payment-{}", Uuid::new_v4()),
            is_principal_only: false,
        };

        // process through waterfall
        let processor = PaymentProcessor::new(PaymentWaterfall::standard());
        let result = processor.process(request, &mut context, time_provider, &mut self.events)?;

        // update state from context
        self.state.accrued_fees = context.accrued_fees;
        self.state.accrued_penalties = context.accrued_penalties;
        self.state.accrued_interest = context.accrued_interest;
        self.state.outstanding_principal = context.outstanding_principal;

        // update payment tracking
        self.state.record_payment(amount, time_provider.now());
        self.state.total_interest_paid += result.application.to_interest;
        self.state.total_fees_paid += result.application.to_fees;

        // check if fully paid
        if self.state.total_outstanding().is_zero() {
            self.state.update_status(FacilityStatus::Settled, time_provider.now());

            self.events.emit(Event::FacilitySettled {
                facility_id: self.id,
                settlement_amount: amount,
                timestamp: time_provider.now(),
            });
        }

        // snapshot state
        self.snapshots.push(StateSnapshot::capture(&self.state, format!("payment: {}", amount)));

        Ok(result)
    }

    /// apply penalty interest
    pub fn apply_penalty_interest(
        &mut self,
        time_provider: &SafeTimeProvider,
    ) -> Result<Money> {
        if self.state.days_past_due == 0 {
            return Ok(Money::ZERO);
        }

        let penalty_config = self.config.interest_config.penalty_config
            .as_ref()
            .ok_or(FacilityError::InvalidConfiguration {
                message: "No penalty configuration".to_string(),
            })?;

        let engine = PenaltyEngine::new(penalty_config.clone());

        // calculate penalty on overdue amount
        let overdue_amount = self.state.minimum_payment_due.unwrap_or(Money::ZERO);
        let calculation = engine.calculate_penalty(overdue_amount, self.state.days_past_due);

        if calculation.penalty_amount > Money::ZERO {
            self.state.accrued_penalties += calculation.penalty_amount;

            self.events.emit(Event::PenaltyInterestApplied {
                facility_id: self.id,
                amount: calculation.penalty_amount,
                days_overdue: self.state.days_past_due,
                timestamp: time_provider.now(),
            });
        }

        Ok(calculation.penalty_amount)
    }

    /// update collateral
    pub fn update_collateral(
        &mut self,
        collateral: CollateralPosition,
        time_provider: &SafeTimeProvider,
    ) -> Result<()> {
        let old_value = self.state.collateral
            .as_ref()
            .map(|c| c.current_value)
            .unwrap_or(Money::ZERO);

        self.events.emit(Event::CollateralValueUpdated {
            facility_id: self.id,
            old_value,
            new_value: collateral.current_value,
            source: collateral.valuation_source.clone(),
            timestamp: time_provider.now(),
        });

        self.state.collateral = Some(collateral);

        // check LTV if applicable
        if let Some(config) = self.config.collateral_config.clone() {
            self.check_ltv_breach(&config.ltv_thresholds, time_provider)?;
        }

        Ok(())
    }

    /// check for LTV breach
    fn check_ltv_breach(
        &mut self,
        thresholds: &crate::types::LtvThresholds,
        time_provider: &SafeTimeProvider,
    ) -> Result<()> {
        let collateral = self.state.collateral.as_ref()
            .ok_or(FacilityError::NoCollateral)?;

        let ltv = Rate::from_decimal(
            self.state.outstanding_principal.as_decimal() / collateral.current_value.as_decimal()
        );

        self.events.emit(Event::LtvCalculated {
            facility_id: self.id,
            ltv_ratio: ltv,
            collateral_value: collateral.current_value,
            total_debt: self.state.outstanding_principal,
            timestamp: time_provider.now(),
        });

        if ltv > thresholds.liquidation_ltv {
            self.state.update_status(FacilityStatus::Liquidating, time_provider.now());

            self.events.emit(Event::LtvLiquidationBreached {
                facility_id: self.id,
                ltv_ratio: ltv,
                threshold: thresholds.liquidation_ltv,
                timestamp: time_provider.now(),
            });

            return Err(FacilityError::LtvBreach {
                ltv,
                threshold: thresholds.liquidation_ltv,
            });
        }

        if ltv > thresholds.margin_call_ltv {
            self.events.emit(Event::MarginCallRequired {
                facility_id: self.id,
                current_ltv: ltv,
                required_ltv: thresholds.margin_call_ltv,
                deadline: time_provider.now() + chrono::Duration::days(7),
                options: vec!["Add collateral".to_string(), "Pay down principal".to_string()],
            });
        }

        if ltv > thresholds.warning_ltv {
            self.events.emit(Event::LtvWarningBreached {
                facility_id: self.id,
                ltv_ratio: ltv,
                threshold: thresholds.warning_ltv,
                timestamp: time_provider.now(),
            });
        }

        Ok(())
    }

    /// update daily status based on actual time
    pub fn update_daily_status(&mut self, time_provider: &SafeTimeProvider) -> Result<()> {
        let now = time_provider.now();

        // calculate actual days past due
        if let Some(due_date) = self.state.next_payment_due {
            if now > due_date {
                // check if payment was made after due date
                let payment_made = self.state.last_payment_date
                    .map(|pd| pd >= due_date)
                    .unwrap_or(false);

                if !payment_made {
                    // calculate days past due
                    let days_overdue = (now - due_date).num_days() as u32;
                    self.state.days_past_due = days_overdue;

                    // update status based on DPD and grace period
                    let grace_period = self.config.interest_config.grace_period_days;

                    let new_status = match days_overdue {
                        0 => FacilityStatus::Active,
                        d if d <= grace_period => FacilityStatus::GracePeriod,
                        _ => FacilityStatus::Delinquent,
                    };

                    // emit event if status changed
                    if new_status != self.state.status {
                        let old_status = self.state.status;
                        self.state.update_status(new_status, now);

                        self.events.emit(Event::StatusChanged {
                            facility_id: self.id,
                            old_status,
                            new_status,
                            reason: format!("{} days past due", days_overdue),
                            timestamp: now,
                        });

                        // emit grace period event if entering grace
                        if new_status == FacilityStatus::GracePeriod && old_status == FacilityStatus::Active {
                            self.events.emit(Event::GracePeriodStarted {
                                facility_id: self.id,
                                payment_due_date: due_date.date_naive(),
                                grace_ends_at: (due_date + chrono::Duration::days(grace_period as i64)).date_naive(),
                                timestamp: now,
                            });
                        }

                        // emit grace period expired if leaving grace
                        if old_status == FacilityStatus::GracePeriod && new_status == FacilityStatus::Delinquent {
                            self.events.emit(Event::GracePeriodExpired {
                                facility_id: self.id,
                                days_overdue,
                                timestamp: now,
                            });

                            // apply late fee
                            if let Some(fee) = self.config.fee_config.late_fee {
                                self.state.accrued_fees += fee;
                                self.state.total_fees_charged += fee;

                                self.events.emit(Event::LateFeeApplied {
                                    facility_id: self.id,
                                    fee_amount: fee,
                                    days_overdue,
                                    timestamp: now,
                                });
                            }
                        }
                    }

                    // apply penalty interest if past grace period
                    if days_overdue > grace_period {
                        self.apply_penalty_interest(time_provider)?;
                    }
                }
            }
        }

        // accrue daily interest regardless of status
        if self.state.outstanding_principal > Money::ZERO {
            self.accrue_interest(time_provider)?;
        }

        Ok(())
    }

    /// get events
    pub fn take_events(&mut self) -> Vec<Event> {
        self.events.take_events()
    }
}
