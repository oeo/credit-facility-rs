use chrono::{DateTime, Utc};
use hourglass_rs::SafeTimeProvider;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use uuid::Uuid;

use crate::config::{FacilityConfig, FacilityType};
use crate::types::RevolvingType;
use crate::decimal::{Money, Rate};
use crate::errors::{FacilityError, Result};
use crate::events::Event;
use crate::facility::Facility;
use crate::types::FacilityStatus;

/// utilization states for revolving facilities
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UtilizationState {
    Unused,      // 0%
    Low,         // < 30%
    Moderate,    // 30-70%
    High,        // 70-90%
    Maxed,       // 90-100%
    Overlimit,   // > 100%
}

/// revolving facility
pub struct RevolvingFacility {
    facility: Facility,
    time: Option<*const SafeTimeProvider>,
    credit_limit: Money,
    available_credit: Money,
    draw_period_ends: Option<DateTime<Utc>>,
    repayment_period_ends: Option<DateTime<Utc>>,
    is_in_draw_period: bool,
    minimum_payment_percentage: Decimal,
}

impl RevolvingFacility {
    /// create new revolving facility
    pub fn new(facility: Facility) -> Result<Self> {
        // validate it's a revolving facility
        match &facility.config.facility_type {
            FacilityType::Revolving(_) => {}
            _ => {
                return Err(FacilityError::InvalidConfiguration {
                    message: "Not a revolving facility configuration".to_string(),
                });
            }
        }
        
        let credit_limit = facility.config.limits.credit_limit
            .unwrap_or(facility.config.financial_terms.commitment_amount);
        
        let minimum_payment_percentage = facility.config.payment_config
            .minimum_payment_percentage
            .unwrap_or(dec!(0.02)); // default 2%
        
        Ok(Self {
            facility,
            time: None,
            credit_limit,
            available_credit: credit_limit,
            draw_period_ends: None,
            repayment_period_ends: None,
            is_in_draw_period: true,
            minimum_payment_percentage,
        })
    }
    
    /// builder for creating revolving facilities
    pub fn builder() -> RevolvingFacilityBuilder {
        RevolvingFacilityBuilder::new()
    }
    
    /// set the time provider for this facility
    pub fn set_time(&mut self, time: &SafeTimeProvider) {
        self.time = Some(time as *const SafeTimeProvider);
    }
    
    /// draw funds using stored time
    pub fn draw(&mut self, amount: Money) -> Result<Money> {
        let time_ptr = self.time
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Time provider not set. Call set_time() first".to_string(),
            })?;
        let time = unsafe { time_ptr.as_ref() }
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Invalid time provider reference".to_string(),
            })?;
        self.draw_with_time(amount, time)
    }
    
    /// process payment using stored time
    pub fn process_payment(&mut self, amount: Money) -> Result<()> {
        let time_ptr = self.time
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Time provider not set. Call set_time() first".to_string(),
            })?;
        let time = unsafe { time_ptr.as_ref() }
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Invalid time provider reference".to_string(),
            })?;
        self.process_payment_with_time(amount, time)
    }
    
    /// activate using stored time
    pub fn activate(&mut self) -> Result<()> {
        let time_ptr = self.time
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Time provider not set. Call set_time() first".to_string(),
            })?;
        let time = unsafe { time_ptr.as_ref() }
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Invalid time provider reference".to_string(),
            })?;
        self.activate_with_time(time)
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
    
    /// activate the facility with explicit time
    pub fn activate_with_time(&mut self, time_provider: &SafeTimeProvider) -> Result<()> {
        self.facility.state.activation_date = Some(time_provider.now());
        self.facility.state.update_status(FacilityStatus::Active, time_provider.now());
        
        // set up draw period for heloc
        if let FacilityType::Revolving(RevolvingType::HELOC) = &self.facility.config.facility_type {
            if let Some(term_months) = self.facility.config.financial_terms.term_months {
                // assume first 60% is draw period
                let draw_months = (term_months as f64 * 0.6) as i64;
                self.draw_period_ends = Some(
                    time_provider.now() + chrono::Duration::days(draw_months * 30)
                );
                self.repayment_period_ends = Some(
                    time_provider.now() + chrono::Duration::days(term_months as i64 * 30)
                );
            }
        }
        
        // update state
        if let crate::state::FacilitySpecificState::Revolving {
            credit_limit,
            available_credit,
            ..
        } = &mut self.facility.state.facility_specific {
            *credit_limit = self.credit_limit;
            *available_credit = self.available_credit;
        }
        
        self.facility.events.emit(Event::FacilityActivated {
            facility_id: self.facility.id,
            first_disbursement: Money::ZERO,
            timestamp: time_provider.now(),
        });
        
        Ok(())
    }
    
    /// draw funds from the facility with explicit time
    pub fn draw_with_time(
        &mut self,
        amount: Money,
        time_provider: &SafeTimeProvider,
    ) -> Result<Money> {
        // validate draw
        if !self.is_in_draw_period {
            return Err(FacilityError::DrawPeriodEnded);
        }
        
        if amount <= Money::ZERO {
            return Err(FacilityError::InvalidDrawAmount { amount });
        }
        
        // check minimum drawdown
        if let Some(min) = self.facility.config.limits.minimum_drawdown {
            if amount < min {
                return Err(FacilityError::BelowMinimumDrawdown { 
                    minimum: min, 
                    requested: amount 
                });
            }
        }
        
        // check if overlimit (may allow with fee)
        let is_overlimit = amount > self.available_credit;
        if is_overlimit {
            // check if we allow overlimit
            let total_after_draw = self.facility.state.outstanding_principal + amount;
            let overlimit_amount = total_after_draw - self.credit_limit;
            
            if overlimit_amount > self.credit_limit * dec!(0.1) {
                // don't allow more than 10% overlimit
                return Err(FacilityError::ExceedsCreditLimit {
                    available: self.available_credit,
                    requested: amount,
                });
            }
            
            // apply overlimit fee
            if let Some(fee) = self.facility.config.fee_config.overlimit_fee {
                self.facility.state.accrued_fees += fee;
                self.facility.state.total_fees_charged += fee;
                
                self.facility.events.emit(Event::OverlimitOccurred {
                    facility_id: self.facility.id,
                    amount_over: overlimit_amount,
                    fees_applied: fee,
                    timestamp: time_provider.now(),
                });
            }
        }
        
        // perform the draw
        self.facility.state.record_disbursement(amount);
        self.available_credit = (self.credit_limit - self.facility.state.outstanding_principal)
            .max(Money::ZERO);
        
        // update revolving state
        if let crate::state::FacilitySpecificState::Revolving {
            available_credit,
            ..
        } = &mut self.facility.state.facility_specific {
            *available_credit = self.available_credit;
        }
        
        // emit draw event
        self.facility.events.emit(Event::FundsDrawn {
            facility_id: self.facility.id,
            amount,
            new_outstanding: self.facility.state.outstanding_principal,
            available_credit: self.available_credit,
            timestamp: time_provider.now(),
        });
        
        Ok(amount)
    }
    
    /// process payment and restore available credit with explicit time
    pub fn process_payment_with_time(
        &mut self,
        amount: Money,
        time_provider: &SafeTimeProvider,
    ) -> Result<()> {
        let principal_before = self.facility.state.outstanding_principal;
        
        // process payment through standard waterfall
        self.facility.process_payment(amount, time_provider)?;
        
        let principal_after = self.facility.state.outstanding_principal;
        let principal_paid = principal_before - principal_after;
        
        // restore available credit
        self.available_credit = (self.available_credit + principal_paid)
            .min(self.credit_limit);
        
        // update state
        if let crate::state::FacilitySpecificState::Revolving {
            available_credit,
            ..
        } = &mut self.facility.state.facility_specific {
            *available_credit = self.available_credit;
        }
        
        Ok(())
    }
    
    /// calculate minimum payment
    pub fn calculate_minimum_payment(&self) -> Money {
        let outstanding = self.facility.state.outstanding_principal;
        let interest = self.facility.state.accrued_interest;
        let fees = self.facility.state.accrued_fees;
        
        // minimum is higher of:
        // 1. percentage of outstanding + all interest and fees
        // 2. absolute minimum from config
        let percentage_based = outstanding * self.minimum_payment_percentage + interest + fees;
        let absolute_minimum = self.facility.config.payment_config.minimum_payment
            .unwrap_or(Money::from_major(25));
        
        // if overlimit, must pay overlimit amount
        if outstanding > self.credit_limit {
            let overlimit = outstanding - self.credit_limit;
            percentage_based.max(absolute_minimum).max(overlimit + interest + fees)
        } else {
            percentage_based.max(absolute_minimum)
        }
    }
    
    /// calculate utilization rate
    pub fn utilization_rate(&self) -> Rate {
        if self.credit_limit.is_zero() {
            return Rate::from_percentage(0);
        }
        
        Rate::from_decimal(
            self.facility.state.outstanding_principal.as_decimal() / 
            self.credit_limit.as_decimal()
        )
    }
    
    /// get utilization state
    pub fn utilization_state(&self) -> UtilizationState {
        let rate = self.utilization_rate();
        
        if rate == Rate::from_percentage(0) {
            UtilizationState::Unused
        } else if rate < Rate::from_percentage(30) {
            UtilizationState::Low
        } else if rate < Rate::from_percentage(70) {
            UtilizationState::Moderate
        } else if rate < Rate::from_percentage(90) {
            UtilizationState::High
        } else if rate <= Rate::from_percentage(100) {
            UtilizationState::Maxed
        } else {
            UtilizationState::Overlimit
        }
    }
    
    /// check if facility is overlimit
    pub fn is_overlimit(&self) -> bool {
        self.facility.state.outstanding_principal > self.credit_limit
    }
    
    /// charge commitment fee on undrawn amounts
    pub fn charge_commitment_fee(&mut self) -> Result<()> {
        let time_ptr = self.time
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Time provider not set. Call set_time() first".to_string(),
            })?;
        let time_provider = unsafe { time_ptr.as_ref() }
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Invalid time provider reference".to_string(),
            })?;
        if let Some(commitment_rate) = self.facility.config.fee_config.commitment_fee_rate {
            let undrawn = self.available_credit;
            if undrawn > Money::ZERO {
                // monthly fee
                let monthly_rate = commitment_rate.as_decimal() / dec!(12);
                let fee = Money::from_decimal(undrawn.as_decimal() * monthly_rate);
                
                self.facility.state.accrued_fees += fee;
                self.facility.state.total_fees_charged += fee;
                
                self.facility.events.emit(Event::CommitmentFeeCharged {
                    facility_id: self.facility.id,
                    undrawn_amount: undrawn,
                    fee,
                    timestamp: time_provider.now(),
                });
            }
        }
        
        Ok(())
    }
    
    /// end draw period (for heloc)
    pub fn end_draw_period(&mut self) -> Result<()> {
        let time_ptr = self.time
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Time provider not set. Call set_time() first".to_string(),
            })?;
        let time_provider = unsafe { time_ptr.as_ref() }
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Invalid time provider reference".to_string(),
            })?;
        if !self.is_in_draw_period {
            return Ok(()); // already ended
        }
        
        self.is_in_draw_period = false;
        self.draw_period_ends = Some(time_provider.now());
        
        // change to amortizing payments
        if let FacilityType::Revolving(RevolvingType::HELOC) = &self.facility.config.facility_type {
            // calculate new minimum payment for amortization
            // this would normally generate an amortization schedule
            self.minimum_payment_percentage = dec!(0.015); // 1.5% for principal + interest
        }
        
        Ok(())
    }
    
    /// change credit limit
    pub fn change_credit_limit(&mut self, new_limit: Money) -> Result<()> {
        let time_ptr = self.time
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Time provider not set. Call set_time() first".to_string(),
            })?;
        let time_provider = unsafe { time_ptr.as_ref() }
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Invalid time provider reference".to_string(),
            })?;
        let old_limit = self.credit_limit;
        
        // update limit
        self.credit_limit = new_limit;
        
        // update available credit
        self.available_credit = (new_limit - self.facility.state.outstanding_principal)
            .max(Money::ZERO);
        
        // update state
        if let crate::state::FacilitySpecificState::Revolving {
            credit_limit,
            available_credit,
            ..
        } = &mut self.facility.state.facility_specific {
            *credit_limit = self.credit_limit;
            *available_credit = self.available_credit;
        }
        
        // emit event
        self.facility.events.emit(Event::CreditLimitChanged {
            facility_id: self.facility.id,
            old_limit,
            new_limit,
            timestamp: time_provider.now(),
        });
        
        // check if now overlimit
        if self.is_overlimit() {
            self.facility.events.emit(Event::OverlimitOccurred {
                facility_id: self.facility.id,
                amount_over: self.facility.state.outstanding_principal - new_limit,
                fees_applied: Money::ZERO,
                timestamp: time_provider.now(),
            });
        }
        
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
    
    /// get credit limit
    pub fn credit_limit(&self) -> Money {
        self.credit_limit
    }
    
    /// get available credit
    pub fn available_credit(&self) -> Money {
        self.available_credit
    }
    
    /// get json representation of current state
    pub fn to_json_pretty(&self) -> String {
        use super::serialization::{FacilityView, RevolvingView};
        
        let view = RevolvingView {
            facility: FacilityView::from_facility(&self.facility),
            credit_limit: self.credit_limit,
            available_credit: self.available_credit,
            utilization_rate: self.utilization_rate(),
            is_in_draw_period: self.is_in_draw_period,
        };
        
        serde_json::to_string_pretty(&view).unwrap_or_else(|e| format!("JSON error: {}", e))
    }
    
    /// short alias for json output
    pub fn json(&self) -> String {
        self.to_json_pretty()
    }
    
    /// approve the facility (alias for activate)
    pub fn approve(&mut self) -> Result<()> {
        self.activate()
    }
    
    /// deny/cancel the facility
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
    pub fn make_payment(&mut self, amount: Money) -> Result<()> {
        self.process_payment(amount)
    }
    
    /// disburse funds (alias for draw)
    pub fn disburse(&mut self, amount: Money) -> Result<Money> {
        self.draw(amount)
    }
}

/// builder for revolving facilities
pub struct RevolvingFacilityBuilder {
    facility_type: Option<RevolvingType>,
    credit_limit: Option<Money>,
    rate: Option<Rate>,
    minimum_percentage: Option<Decimal>,
    commitment_fee_rate: Option<Rate>,
    property_value: Option<Money>,
    draw_period: Option<u32>,
    repayment_period: Option<u32>,
    account_number: Option<String>,
    customer_id: Option<String>,
    time_provider: Option<*const SafeTimeProvider>,
}

impl RevolvingFacilityBuilder {
    pub fn new() -> Self {
        Self {
            facility_type: None,
            credit_limit: None,
            rate: None,
            minimum_percentage: None,
            commitment_fee_rate: None,
            property_value: None,
            draw_period: None,
            repayment_period: None,
            account_number: None,
            customer_id: None,
            time_provider: None,
        }
    }
    
    pub fn set_time(mut self, time: &SafeTimeProvider) -> Self {
        self.time_provider = Some(time as *const SafeTimeProvider);
        self
    }
    
    pub fn facility_type(mut self, facility_type: RevolvingType) -> Self {
        self.facility_type = Some(facility_type);
        self
    }
    
    pub fn credit_limit(mut self, limit: Money) -> Self {
        self.credit_limit = Some(limit);
        self
    }
    
    pub fn rate(mut self, rate: Rate) -> Self {
        self.rate = Some(rate);
        self
    }
    
    pub fn minimum_percentage(mut self, percentage: Decimal) -> Self {
        self.minimum_percentage = Some(percentage);
        self
    }
    
    pub fn commitment_fee_rate(mut self, rate: Rate) -> Self {
        self.commitment_fee_rate = Some(rate);
        self
    }
    
    pub fn property_value(mut self, value: Money) -> Self {
        self.property_value = Some(value);
        self
    }
    
    pub fn draw_period_months(mut self, months: u32) -> Self {
        self.draw_period = Some(months);
        self
    }
    
    pub fn repayment_period_months(mut self, months: u32) -> Self {
        self.repayment_period = Some(months);
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
    pub fn build(self) -> Result<RevolvingFacility> {
        if let Some(time_ptr) = self.time_provider {
            let time = unsafe { time_ptr.as_ref() }
                .ok_or(FacilityError::InvalidConfiguration {
                    message: "Invalid time provider reference".to_string(),
                })?;
            self.build_with_time(time)
        } else {
            let time = SafeTimeProvider::new(hourglass_rs::TimeSource::System);
            self.build_with_time(&time)
        }
    }
    
    /// Build with system time
    pub fn build_now(self) -> Result<RevolvingFacility> {
        let time = SafeTimeProvider::new(hourglass_rs::TimeSource::System);
        self.build_with_time(&time)
    }
    
    /// Build with explicit time provider (for backward compatibility)
    pub fn build_with_time(self, time_provider: &SafeTimeProvider) -> Result<RevolvingFacility> {
        // Infer facility type based on provided values, or default to LineOfCredit
        let facility_type = self.facility_type.unwrap_or_else(|| {
            if self.property_value.is_some() {
                RevolvingType::HELOC  // Home equity line of credit
            } else if self.minimum_percentage == Some(dec!(0.02)) || self.minimum_percentage == Some(dec!(0.025)) {
                RevolvingType::CreditCard  // Credit cards typically have 2-2.5% minimum
            } else {
                RevolvingType::LineOfCredit  // General purpose line of credit
            }
        });
        
        let credit_limit = self.credit_limit.ok_or(FacilityError::InvalidConfiguration {
            message: "Credit limit required".to_string(),
        })?;
        
        let rate = self.rate.ok_or(FacilityError::InvalidConfiguration {
            message: "Rate required".to_string(),
        })?;
        
        let config = match facility_type {
            RevolvingType::CreditCard => {
                FacilityConfig::credit_card(
                    credit_limit,
                    rate,
                    self.minimum_percentage.unwrap_or(dec!(0.02)),
                )
            }
            RevolvingType::LineOfCredit => {
                FacilityConfig::line_of_credit(
                    credit_limit,
                    rate,
                    self.commitment_fee_rate.unwrap_or(Rate::from_decimal(dec!(0.005))),
                )
            }
            RevolvingType::HELOC => {
                let property_value = self.property_value.ok_or(FacilityError::InvalidConfiguration {
                    message: "Property value required for HELOC".to_string(),
                })?;
                
                FacilityConfig::heloc(
                    credit_limit,
                    rate,
                    property_value,
                    self.draw_period.unwrap_or(120), // 10 years default
                    self.repayment_period.unwrap_or(240), // 20 years default
                )
            }
        };
        
        let account_number = self.account_number.unwrap_or_else(|| {
            format!("REV-{}", Uuid::new_v4().to_string()[..8].to_uppercase())
        });
        
        let customer_id = self.customer_id.unwrap_or_else(|| {
            format!("CUST-{}", Uuid::new_v4().to_string()[..8].to_uppercase())
        });
        
        let facility = Facility::originate(config, account_number, customer_id, time_provider)?;
        
        let mut revolving = RevolvingFacility::new(facility)?;
        
        // If time was set in builder, pass it to the facility
        if let Some(time_ptr) = self.time_provider {
            revolving.time = Some(time_ptr);
        }
        
        Ok(revolving)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hourglass_rs::TimeSource;
    use chrono::TimeZone;
    
    #[test]
    fn test_credit_card_creation() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        
        let card = RevolvingFacility::builder()
            .facility_type(RevolvingType::CreditCard)
            .credit_limit(Money::from_major(10_000))
            .rate(Rate::from_percentage(18))
            .minimum_percentage(dec!(0.02))
            .set_time(&time)
            .build()
            .unwrap();
        
        assert_eq!(card.credit_limit, Money::from_major(10_000));
        assert_eq!(card.available_credit, Money::from_major(10_000));
        assert_eq!(card.utilization_rate(), Rate::from_percentage(0));
    }
    
    #[test]
    fn test_draw_and_repay_cycle() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        let control = time.test_control().unwrap();
        
        let mut card = RevolvingFacility::builder()
            .facility_type(RevolvingType::CreditCard)
            .credit_limit(Money::from_major(10_000))
            .rate(Rate::from_percentage(18))
            .set_time(&time).build()
            .unwrap();
        
        card.activate().unwrap();
        
        // draw funds
        let drawn = card.draw(Money::from_major(3_000)).unwrap();
        assert_eq!(drawn, Money::from_major(3_000));
        assert_eq!(card.available_credit, Money::from_major(7_000));
        assert_eq!(card.utilization_rate(), Rate::from_percentage(30));
        
        // accrue interest for a month
        control.advance(chrono::Duration::days(30));
        card.facility_mut().accrue_interest(&time).unwrap();
        
        // make payment
        card.process_payment(Money::from_major(1_000)).unwrap();
        
        // available credit increased
        assert!(card.available_credit > Money::from_major(7_000));
        
        // can redraw
        card.draw(Money::from_major(500)).unwrap();
        assert!(card.available_credit >= Money::from_major(7_400)); // roughly 7500 minus some interest
    }
    
    #[test]
    fn test_utilization_states() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        
        let mut card = RevolvingFacility::builder()
            .facility_type(RevolvingType::CreditCard)
            .credit_limit(Money::from_major(10_000))
            .rate(Rate::from_percentage(18))
            .set_time(&time).build()
            .unwrap();
        
        card.activate().unwrap();
        
        // test different utilization levels
        assert_eq!(card.utilization_state(), UtilizationState::Unused);
        
        card.draw(Money::from_major(2_000)).unwrap();
        assert_eq!(card.utilization_state(), UtilizationState::Low);
        
        card.draw(Money::from_major(3_000)).unwrap();
        assert_eq!(card.utilization_state(), UtilizationState::Moderate);
        
        card.draw(Money::from_major(3_000)).unwrap();
        assert_eq!(card.utilization_state(), UtilizationState::High);
        
        card.draw(Money::from_major(1_500)).unwrap();
        assert_eq!(card.utilization_state(), UtilizationState::Maxed);
    }
    
    #[test]
    fn test_overlimit_handling() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        
        let mut card = RevolvingFacility::builder()
            .facility_type(RevolvingType::CreditCard)
            .credit_limit(Money::from_major(1_000))
            .rate(Rate::from_percentage(18))
            .set_time(&time).build()
            .unwrap();
        
        card.activate().unwrap();
        
        // max out the card
        card.draw(Money::from_major(1_000)).unwrap();
        assert_eq!(card.available_credit, Money::ZERO);
        
        // try to go overlimit (should work with fee)
        card.draw(Money::from_major(50)).unwrap();
        assert!(card.is_overlimit());
        assert_eq!(card.utilization_state(), UtilizationState::Overlimit);
        
        // check that overlimit fee was applied
        assert!(card.facility.state.accrued_fees > Money::ZERO);
        
        // try to exceed 10% overlimit (should fail)
        let result = card.draw(Money::from_major(100));
        assert!(result.is_err());
    }
    
    #[test]
    fn test_minimum_payment_calculation() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        let control = time.test_control().unwrap();
        
        let mut card = RevolvingFacility::builder()
            .facility_type(RevolvingType::CreditCard)
            .credit_limit(Money::from_major(10_000))
            .rate(Rate::from_percentage(18))
            .minimum_percentage(dec!(0.02))
            .set_time(&time).build()
            .unwrap();
        
        card.activate().unwrap();
        card.draw(Money::from_major(1_000)).unwrap();
        
        // accrue interest
        control.advance(chrono::Duration::days(30));
        card.facility_mut().accrue_interest(&time).unwrap();
        
        let minimum = card.calculate_minimum_payment();
        
        // should be at least 2% of balance + interest
        let expected = Money::from_major(20) + card.facility.state.accrued_interest;
        assert!(minimum >= expected);
        
        // should also be at least $25
        assert!(minimum >= Money::from_major(25));
    }
    
    #[test]
    fn test_commitment_fee() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        
        let mut loc = RevolvingFacility::builder()
            .facility_type(RevolvingType::LineOfCredit)
            .credit_limit(Money::from_major(500_000))
            .rate(Rate::from_percentage(8))
            .commitment_fee_rate(Rate::from_decimal(dec!(0.005)))
            .set_time(&time).build()
            .unwrap();
        
        loc.activate().unwrap();
        
        // draw partial amount
        loc.draw(Money::from_major(200_000)).unwrap();
        
        // get initial fees (origination fee)
        let initial_fees = loc.facility.state.accrued_fees;
        
        // charge commitment fee on undrawn
        loc.charge_commitment_fee().unwrap();
        
        // fee should be on $300k undrawn at 0.5% annual / 12
        let expected_commitment_fee = Money::from_decimal(dec!(300_000) * dec!(0.005) / dec!(12));
        assert_eq!(loc.facility.state.accrued_fees - initial_fees, expected_commitment_fee);
    }
    
    #[test]
    fn test_credit_limit_change() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        
        let mut card = RevolvingFacility::builder()
            .facility_type(RevolvingType::CreditCard)
            .credit_limit(Money::from_major(10_000))
            .rate(Rate::from_percentage(18))
            .set_time(&time).build()
            .unwrap();
        
        card.activate().unwrap();
        card.draw(Money::from_major(4_000)).unwrap();
        
        // increase limit
        card.change_credit_limit(Money::from_major(15_000)).unwrap();
        assert_eq!(card.credit_limit, Money::from_major(15_000));
        assert_eq!(card.available_credit, Money::from_major(11_000));
        
        // decrease limit below outstanding (triggers overlimit)
        card.change_credit_limit(Money::from_major(3_000)).unwrap();
        assert!(card.is_overlimit());
        assert_eq!(card.available_credit, Money::ZERO);
    }
    
    #[test]
    fn test_heloc_draw_period() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        
        let mut heloc = RevolvingFacility::builder()
            .facility_type(RevolvingType::HELOC)
            .credit_limit(Money::from_major(100_000))
            .rate(Rate::from_percentage(6))
            .property_value(Money::from_major(400_000))
            .draw_period_months(120)
            .repayment_period_months(240)
            .set_time(&time).build()
            .unwrap();
        
        heloc.activate().unwrap();
        
        // can draw during draw period
        assert!(heloc.is_in_draw_period);
        heloc.draw(Money::from_major(50_000)).unwrap();
        
        // end draw period
        heloc.end_draw_period().unwrap();
        assert!(!heloc.is_in_draw_period);
        
        // cannot draw after draw period ends
        let result = heloc.draw(Money::from_major(10_000));
        assert!(result.is_err());
        
        // minimum payment increases for amortization
        let minimum = heloc.calculate_minimum_payment();
        assert!(minimum > Money::from_major(750)); // 1.5% of balance
    }
}