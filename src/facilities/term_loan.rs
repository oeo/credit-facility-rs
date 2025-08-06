use chrono::{DateTime, Utc};
use hourglass_rs::SafeTimeProvider;
use uuid::Uuid;

use crate::config::{FacilityConfig, FacilityType};
use crate::types::TermLoanType;
use crate::decimal::{Money, Rate};
use crate::errors::{FacilityError, Result};
use crate::events::Event;
use crate::facility::Facility;
use crate::payments::AmortizationSchedule;
use crate::types::FacilityStatus;

/// term loan facility
pub struct TermLoan {
    facility: Facility,
    time: Option<*const SafeTimeProvider>,
    amortization_schedule: Option<AmortizationSchedule>,
    current_payment_number: u32,
}

impl TermLoan {
    /// create new term loan
    pub fn new(facility: Facility) -> Result<Self> {
        // validate it's a term loan
        match &facility.config.facility_type {
            FacilityType::TermLoan(_) => {}
            _ => {
                return Err(FacilityError::InvalidConfiguration {
                    message: "Not a term loan configuration".to_string(),
                });
            }
        }

        Ok(Self {
            facility,
            time: None,
            amortization_schedule: None,
            current_payment_number: 0,
        })
    }

    /// builder for creating term loans
    pub fn builder() -> TermLoanBuilder {
        TermLoanBuilder::new()
    }

    /// set the time provider for this loan
    pub fn set_time(&mut self, time: &SafeTimeProvider) {
        self.time = Some(time as *const SafeTimeProvider);
    }

    /// originate the loan (activate it) using stored time
    pub fn originate(&mut self) -> Result<()> {
        let time_ptr = self.time
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Time provider not set. Call set_time() first".to_string(),
            })?;
        let time = unsafe { time_ptr.as_ref() }
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Invalid time provider reference".to_string(),
            })?;
        
        // set status to active
        self.facility.state.update_status(FacilityStatus::Active, time.now());
        self.facility.state.activation_date = Some(time.now());
        
        // generate amortization schedule
        self.generate_schedule(time)?;
        
        // set first payment due
        if let Some(schedule) = &self.amortization_schedule {
            if let Some(first_payment) = schedule.get_payment(1) {
                self.facility.state.next_payment_due = Some(first_payment.payment_date);
                self.facility.state.next_payment_amount = Some(first_payment.payment_amount);
                self.facility.state.minimum_payment_due = Some(first_payment.payment_amount);
            }
        }
        
        Ok(())
    }
    
    /// disburse funds using stored time
    pub fn disburse(&mut self, amount: Money) -> Result<Money> {
        let time_ptr = self.time
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Time provider not set. Call set_time() first".to_string(),
            })?;
        let time = unsafe { time_ptr.as_ref() }
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Invalid time provider reference".to_string(),
            })?;
        
        self.facility.disburse(amount, time)
    }
    
    /// originate and disburse using stored time (convenience method)
    pub fn originate_and_disburse(&mut self) -> Result<Money> {
        self.originate()?;
        let amount = self.facility.config.financial_terms.commitment_amount;
        self.disburse(amount)
    }

    /// process payment using stored time
    pub fn process_payment(&mut self, amount: Money) -> Result<crate::payments::PaymentResult> {
        let time_ptr = self.time
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Time provider not set. Call set_time() first".to_string(),
            })?;
        let time = unsafe { time_ptr.as_ref() }
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Invalid time provider reference".to_string(),
            })?;
        self.facility.process_payment(amount, time)
    }

    /// accrue interest using stored time
    pub fn accrue_interest(&mut self) -> Result<Vec<crate::interest::DailyAccrual>> {
        let time_ptr = self.time
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Time provider not set. Call set_time() first".to_string(),
            })?;
        let time = unsafe { time_ptr.as_ref() }
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Invalid time provider reference".to_string(),
            })?;
        self.facility.accrue_interest(time)
    }

    /// update daily status using stored time
    pub fn update_daily_status(&mut self) -> Result<()> {
        let time_ptr = self.time
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Time provider not set. Call set_time() first".to_string(),
            })?;
        let time = unsafe { time_ptr.as_ref() }
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Invalid time provider reference".to_string(),
            })?;
        self.facility.update_daily_status(time)
    }

    /// process scheduled payment using stored time
    pub fn process_scheduled_payment(&mut self) -> Result<()> {
        let time_ptr = self.time
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Time provider not set. Call set_time() first".to_string(),
            })?;
        let time = unsafe { time_ptr.as_ref() }
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Invalid time provider reference".to_string(),
            })?;
        self.process_scheduled_payment_with_time(time)
    }


    /// originate and disburse term loan with explicit time (original method)
    pub fn originate_and_disburse_with_time(
        &mut self,
        time_provider: &SafeTimeProvider,
    ) -> Result<Money> {
        let amount = self.facility.config.financial_terms.commitment_amount;

        // disburse full amount for term loans
        let disbursed = self.facility.disburse(amount, time_provider)?;

        // generate amortization schedule
        self.generate_schedule(time_provider)?;

        // set first payment due
        if let Some(schedule) = &self.amortization_schedule {
            if let Some(first_payment) = schedule.get_payment(1) {
                self.facility.state.next_payment_due = Some(first_payment.payment_date);
                self.facility.state.next_payment_amount = Some(first_payment.payment_amount);
                self.facility.state.minimum_payment_due = Some(first_payment.payment_amount);
            }
        }

        Ok(disbursed)
    }

    /// generate amortization schedule
    fn generate_schedule(&mut self, time_provider: &SafeTimeProvider) -> Result<()> {
        let schedule = AmortizationSchedule::generate(
            self.facility.id,
            self.facility.config.financial_terms.commitment_amount,
            self.facility.config.financial_terms.interest_rate,
            self.facility.config.financial_terms.term_months.unwrap_or(0),
            self.facility.config.financial_terms.origination_date,
            self.facility.config.financial_terms.amortization_method,
            time_provider,
        )?;

        self.amortization_schedule = Some(schedule);
        Ok(())
    }

    /// process scheduled payment with explicit time
    pub fn process_scheduled_payment_with_time(
        &mut self,
        time_provider: &SafeTimeProvider,
    ) -> Result<()> {
        let schedule = self.amortization_schedule.as_ref()
            .ok_or(FacilityError::InvalidConfiguration {
                message: "No amortization schedule".to_string(),
            })?;

        self.current_payment_number += 1;

        let scheduled = schedule.get_payment(self.current_payment_number)
            .ok_or(FacilityError::InvalidConfiguration {
                message: format!("No payment {} in schedule", self.current_payment_number),
            })?;

        // accrue interest up to payment date
        self.facility.accrue_interest(time_provider)?;

        // process the payment
        let _result = self.facility.process_payment(scheduled.payment_amount, time_provider)?;

        // update next payment due
        if let Some(next) = schedule.get_payment(self.current_payment_number + 1) {
            self.facility.state.next_payment_due = Some(next.payment_date);
            self.facility.state.next_payment_amount = Some(next.payment_amount);
            self.facility.state.minimum_payment_due = Some(next.payment_amount);
        } else {
            // last payment made
            self.facility.state.next_payment_due = None;
            self.facility.state.next_payment_amount = None;
            self.facility.state.minimum_payment_due = None;
        }

        // check if this was the final payment
        if self.current_payment_number >= schedule.term_months {
            self.handle_maturity(time_provider)?;
        }

        Ok(())
    }

    /// handle loan maturity
    fn handle_maturity(&mut self, time_provider: &SafeTimeProvider) -> Result<()> {
        let now = time_provider.now();

        // check for balloon payment
        if let Some(balloon) = self.facility.config.financial_terms.balloon_payment {
            if self.facility.state.outstanding_principal >= balloon {
                // balloon payment due
                self.facility.state.minimum_payment_due = Some(balloon);

                self.facility.events.emit(Event::PaymentDue {
                    facility_id: self.facility.id,
                    amount: balloon,
                    due_date: now.date_naive(),
                    principal_portion: balloon,
                    interest_portion: Money::ZERO,
                });

                return Ok(());
            }
        }

        // check if fully paid
        if self.facility.state.total_outstanding().is_zero() {
            self.facility.state.update_status(FacilityStatus::Settled, now);

            self.facility.events.emit(Event::FacilityMatured {
                facility_id: self.facility.id,
                final_payment: self.facility.state.last_payment_amount.unwrap_or(Money::ZERO),
                timestamp: now,
            });
        }

        Ok(())
    }

    /// handle missed payment (called when payment date is detected as missed)
    pub fn handle_missed_payment(&mut self, time_provider: &SafeTimeProvider) -> Result<()> {
        self.facility.state.missed_payment_count += 1;

        // emit missed payment event
        self.facility.events.emit(Event::PaymentMissed {
            facility_id: self.facility.id,
            expected_amount: self.facility.state.minimum_payment_due.unwrap_or(Money::ZERO),
            due_date: self.facility.state.next_payment_due
                .unwrap_or(time_provider.now())
                .date_naive(),
        });

        // let daily status update handle the state transitions
        self.facility.update_daily_status(time_provider)?;

        Ok(())
    }

    /// get facility reference
    pub fn facility(&self) -> &Facility {
        &self.facility
    }

    /// get mutable facility reference
    pub fn facility_mut(&mut self) -> &mut Facility {
        &mut self.facility
    }

    /// get amortization schedule
    pub fn schedule(&self) -> Option<&AmortizationSchedule> {
        self.amortization_schedule.as_ref()
    }
    
    /// get json representation of current state
    pub fn to_json_pretty(&self) -> String {
        use super::serialization::{FacilityView, TermLoanView};
        
        let view = TermLoanView {
            facility: FacilityView::from_facility(&self.facility),
            current_payment_number: self.current_payment_number,
            has_amortization_schedule: self.amortization_schedule.is_some(),
        };
        
        serde_json::to_string_pretty(&view).unwrap_or_else(|e| format!("JSON error: {}", e))
    }
    
    /// short alias for json output
    pub fn json(&self) -> String {
        self.to_json_pretty()
    }
    
    /// approve the loan (alias for originate)
    pub fn approve(&mut self) -> Result<()> {
        self.originate()
    }
    
    /// deny/cancel the loan
    /// note: currently uses settled status due to lack of cancelled status
    pub fn deny(&mut self) -> Result<()> {
        let time_ptr = self.time
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Time provider not set. Call set_time() first".to_string(),
            })?;
        let time = unsafe { time_ptr.as_ref() }
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Invalid time provider reference".to_string(),
            })?;
        
        let old_status = self.facility.state.status;
        self.facility.state.update_status(FacilityStatus::Settled, time.now());
        
        self.facility.events.emit(Event::StatusChanged {
            facility_id: self.facility.id,
            old_status,
            new_status: FacilityStatus::Settled,
            reason: "Denied by lender - no funds disbursed".to_string(),
            timestamp: time.now(),
        });
        
        Ok(())
    }
    
    /// make payment (alias for process_payment)
    pub fn make_payment(&mut self, amount: Money) -> Result<crate::payments::PaymentResult> {
        self.process_payment(amount)
    }
}

/// builder for term loans
pub struct TermLoanBuilder {
    loan_type: Option<TermLoanType>,
    amount: Option<Money>,
    rate: Option<Rate>,
    term_months: Option<u32>,
    origination_date: Option<DateTime<Utc>>,
    property_value: Option<Money>,
    vehicle_value: Option<Money>,
    balloon_percentage: Option<rust_decimal::Decimal>,
    account_number: Option<String>,
    customer_id: Option<String>,
    time_provider: Option<*const SafeTimeProvider>,
}

impl TermLoanBuilder {
    pub fn new() -> Self {
        Self {
            loan_type: None,
            amount: None,
            rate: None,
            term_months: None,
            origination_date: None,
            property_value: None,
            vehicle_value: None,
            balloon_percentage: None,
            account_number: None,
            customer_id: None,
            time_provider: None,
        }
    }

    pub fn set_time(mut self, time: &SafeTimeProvider) -> Self {
        self.time_provider = Some(time as *const SafeTimeProvider);
        self
    }

    pub fn loan_type(mut self, loan_type: TermLoanType) -> Self {
        self.loan_type = Some(loan_type);
        self
    }

    pub fn amount(mut self, amount: Money) -> Self {
        self.amount = Some(amount);
        self
    }

    pub fn rate(mut self, rate: Rate) -> Self {
        self.rate = Some(rate);
        self
    }

    pub fn term_months(mut self, months: u32) -> Self {
        self.term_months = Some(months);
        self
    }

    pub fn origination_date(mut self, date: DateTime<Utc>) -> Self {
        self.origination_date = Some(date);
        self
    }

    pub fn property_value(mut self, value: Money) -> Self {
        self.property_value = Some(value);
        self
    }

    pub fn vehicle_value(mut self, value: Money) -> Self {
        self.vehicle_value = Some(value);
        self
    }

    pub fn balloon_percentage(mut self, percentage: rust_decimal::Decimal) -> Self {
        self.balloon_percentage = Some(percentage);
        self
    }

    pub fn account_number(mut self, account: String) -> Self {
        self.account_number = Some(account);
        self
    }

    pub fn customer_id(mut self, customer: String) -> Self {
        self.customer_id = Some(customer);
        self
    }

    /// Build with stored time or system time if not set
    pub fn build(self) -> Result<TermLoan> {
        if let Some(time_ptr) = self.time_provider {
            // Use the stored time provider
            let time = unsafe { time_ptr.as_ref() }
                .ok_or(FacilityError::InvalidConfiguration {
                    message: "Invalid time provider reference".to_string(),
                })?;
            self.build_with_time(time)
        } else {
            // Default to system time
            let time = SafeTimeProvider::new(hourglass_rs::TimeSource::System);
            self.build_with_time(&time)
        }
    }

    /// Build with system time
    pub fn build_now(self) -> Result<TermLoan> {
        let time = SafeTimeProvider::new(hourglass_rs::TimeSource::System);
        self.build_with_time(&time)
    }

    /// Build with explicit time provider (for backward compatibility)
    pub fn build_with_time(self, time_provider: &SafeTimeProvider) -> Result<TermLoan> {
        // Infer loan type based on provided values, or default to PersonalLoan
        let loan_type = self.loan_type.unwrap_or_else(|| {
            if self.property_value.is_some() {
                TermLoanType::Mortgage
            } else if self.vehicle_value.is_some() {
                TermLoanType::AutoLoan
            } else {
                TermLoanType::PersonalLoan
            }
        });

        let amount = self.amount.ok_or(FacilityError::InvalidConfiguration {
            message: "Amount required".to_string(),
        })?;

        let rate = self.rate.ok_or(FacilityError::InvalidConfiguration {
            message: "Rate required".to_string(),
        })?;

        let term = self.term_months.ok_or(FacilityError::InvalidConfiguration {
            message: "Term required".to_string(),
        })?;

        let origination_date = self.origination_date.unwrap_or_else(|| time_provider.now());

        let config = match loan_type {
            TermLoanType::Mortgage => {
                let property_value = self.property_value.ok_or(FacilityError::InvalidConfiguration {
                    message: "Property value required for mortgage".to_string(),
                })?;

                FacilityConfig::mortgage(amount, rate, term, origination_date, property_value)
            }
            TermLoanType::PersonalLoan => {
                FacilityConfig::personal_loan(amount, rate, term, origination_date)
            }
            TermLoanType::AutoLoan => {
                let vehicle_value = self.vehicle_value.ok_or(FacilityError::InvalidConfiguration {
                    message: "Vehicle value required for auto loan".to_string(),
                })?;

                FacilityConfig::auto_loan(
                    amount,
                    rate,
                    term,
                    origination_date,
                    vehicle_value,
                    self.balloon_percentage,
                )
            }
            _ => {
                return Err(FacilityError::InvalidConfiguration {
                    message: format!("Unsupported loan type: {:?}", loan_type),
                });
            }
        };

        let account_number = self.account_number.unwrap_or_else(|| {
            format!("ACC-{}", Uuid::new_v4().to_string()[..8].to_uppercase())
        });

        let customer_id = self.customer_id.unwrap_or_else(|| {
            format!("CUST-{}", Uuid::new_v4().to_string()[..8].to_uppercase())
        });

        let facility = Facility::originate(config, account_number, customer_id, time_provider)?;

        let mut term_loan = TermLoan::new(facility)?;

        // If time was set in builder, pass it to the loan
        if let Some(time_ptr) = self.time_provider {
            term_loan.time = Some(time_ptr);
        }

        Ok(term_loan)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hourglass_rs::TimeSource;
    use chrono::TimeZone;

    #[test]
    fn test_mortgage_creation() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));

        let loan = TermLoan::builder()
            .loan_type(TermLoanType::Mortgage)
            .amount(Money::from_major(300_000))
            .rate(Rate::from_percentage(4))
            .term_months(360)
            .property_value(Money::from_major(400_000))
            .account_number("MORT-001".to_string())
            .customer_id("CUST-123".to_string())
            .set_time(&time)
            .build()
            .unwrap();

        assert_eq!(loan.facility.state.original_commitment, Money::from_major(300_000));
        assert_eq!(loan.facility.config.financial_terms.term_months, Some(360));
    }

    #[test]
    fn test_personal_loan_disbursement() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));

        let mut loan = TermLoan::builder()
            .loan_type(TermLoanType::PersonalLoan)
            .amount(Money::from_major(10_000))
            .rate(Rate::from_percentage(12))
            .term_months(36)
            .set_time(&time)
            .build()
            .unwrap();

        let disbursed = loan.originate_and_disburse().unwrap();

        assert_eq!(disbursed, Money::from_major(10_000));
        assert_eq!(loan.facility.state.outstanding_principal, Money::from_major(10_000));
        assert_eq!(loan.facility.state.status, FacilityStatus::Active);
        assert!(loan.amortization_schedule.is_some());
    }

    #[test]
    fn test_auto_loan_with_balloon() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));

        let loan = TermLoan::builder()
            .loan_type(TermLoanType::AutoLoan)
            .amount(Money::from_major(30_000))
            .rate(Rate::from_percentage(6))
            .term_months(60)
            .vehicle_value(Money::from_major(35_000))
            .balloon_percentage(rust_decimal_macros::dec!(30))
            .set_time(&time)
            .build()
            .unwrap();

        assert_eq!(loan.facility.config.financial_terms.balloon_payment,
                   Some(Money::from_major(9_000)));
    }

    #[test]
    fn test_payment_processing() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        let control = time.test_control().unwrap();

        let mut loan = TermLoan::builder()
            .loan_type(TermLoanType::PersonalLoan)
            .amount(Money::from_major(10_000))
            .rate(Rate::from_percentage(12))
            .term_months(12)
            .set_time(&time)
            .build()
            .unwrap();

        loan.originate_and_disburse().unwrap();

        // advance to first payment date
        control.advance(chrono::Duration::days(30));

        // process first scheduled payment
        loan.process_scheduled_payment().unwrap();

        assert_eq!(loan.current_payment_number, 1);
        assert!(loan.facility.state.outstanding_principal < Money::from_major(10_000));
        assert_eq!(loan.facility.state.payment_count, 1);
    }

    #[test]
    fn test_missed_payment_handling() {
        let start_date = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let time = SafeTimeProvider::new(TimeSource::Test(start_date));
        let control = time.test_control().unwrap();

        let mut loan = TermLoan::builder()
            .loan_type(TermLoanType::PersonalLoan)
            .amount(Money::from_major(5_000))
            .rate(Rate::from_percentage(15))
            .term_months(24)
            .set_time(&time)
            .build()
            .unwrap();

        loan.originate_and_disburse().unwrap();

        // get the actual payment due date
        let payment_due = loan.facility.state.next_payment_due.unwrap();
        let days_until_due = (payment_due - time.now()).num_days();

        // advance to payment due date
        control.advance(chrono::Duration::days(days_until_due));

        // payment is now due, but not yet missed
        loan.facility.update_daily_status(&time).unwrap();
        assert_eq!(loan.facility.state.status, FacilityStatus::Active);

        // advance 1 day past due - should enter grace period
        control.advance(chrono::Duration::days(1));
        loan.facility.update_daily_status(&time).unwrap();
        assert_eq!(loan.facility.state.status, FacilityStatus::GracePeriod);
        assert_eq!(loan.facility.state.days_past_due, 1);

        // advance to day 10 (last day of grace for personal loan)
        control.advance(chrono::Duration::days(9));
        loan.facility.update_daily_status(&time).unwrap();
        assert_eq!(loan.facility.state.status, FacilityStatus::GracePeriod);
        assert_eq!(loan.facility.state.days_past_due, 10);

        // advance to day 11 - should become delinquent
        control.advance(chrono::Duration::days(1));
        loan.facility.update_daily_status(&time).unwrap();
        assert_eq!(loan.facility.state.status, FacilityStatus::Delinquent);
        assert_eq!(loan.facility.state.days_past_due, 11);

        // verify late fee was applied
        assert!(loan.facility.state.accrued_fees > Money::ZERO);
    }

    #[test]
    fn test_grace_period_transitions_with_time() {
        let start_date = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let time = SafeTimeProvider::new(TimeSource::Test(start_date));
        let control = time.test_control().unwrap();

        let mut loan = TermLoan::builder()
            .loan_type(TermLoanType::PersonalLoan)
            .amount(Money::from_major(10_000))
            .rate(Rate::from_percentage(12))
            .term_months(12)
            .set_time(&time)
            .build()
            .unwrap();

        loan.originate_and_disburse().unwrap();

        // get the actual payment due date and advance to it
        let payment_due = loan.facility.state.next_payment_due.unwrap();
        let days_until_due = (payment_due - time.now()).num_days();
        control.advance(chrono::Duration::days(days_until_due));

        // track daily through grace period
        let mut daily_interest_balances = Vec::new();

        for day in 1..=15 {
            control.advance(chrono::Duration::days(1));
            loan.facility.update_daily_status(&time).unwrap();

            daily_interest_balances.push((day, loan.facility.state.accrued_interest));

            // verify status transitions
            match day {
                1..=10 => {
                    assert_eq!(loan.facility.state.status, FacilityStatus::GracePeriod,
                              "Day {} should be in grace period", day);
                    assert_eq!(loan.facility.state.days_past_due, day);
                }
                _ => {
                    assert_eq!(loan.facility.state.status, FacilityStatus::Delinquent,
                              "Day {} should be delinquent", day);
                    assert_eq!(loan.facility.state.days_past_due, day);
                }
            }
        }

        // verify interest continued to accrue daily
        for i in 1..daily_interest_balances.len() {
            assert!(
                daily_interest_balances[i].1 > daily_interest_balances[i-1].1,
                "Interest should increase daily"
            );
        }

        // verify penalties applied after grace period
        assert!(loan.facility.state.accrued_penalties > Money::ZERO);
    }
}
