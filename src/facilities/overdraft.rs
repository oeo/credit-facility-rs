use hourglass_rs::SafeTimeProvider;
use rust_decimal_macros::dec;
use uuid::Uuid;

use crate::config::{FacilityConfig, FacilityType};
use crate::decimal::{Money, Rate};
use crate::errors::{FacilityError, Result};
use crate::events::Event;
use crate::facility::Facility;
use crate::types::FacilityStatus;

/// overdraft states
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverdraftState {
    Available,   // not in use
    Active,      // currently overdrawn
    BufferZone,  // within fee-free amount
    Exceeded,    // over arranged limit
    Suspended,   // facility withdrawn
}

/// overdraft facility linked to account
pub struct OverdraftFacility {
    facility: Facility,
    time: Option<*const SafeTimeProvider>,
    overdraft_limit: Money,
    buffer_zone: Money,
    _linked_account_id: String,
    linked_account_balance: Money,
    is_active: bool,
    daily_fee: Option<Money>,
    state: OverdraftState,
}

impl OverdraftFacility {
    /// create new overdraft facility
    pub fn new(
        facility: Facility,
        buffer_zone: Money,
        linked_account_id: String,
    ) -> Result<Self> {
        // validate it's an overdraft
        match &facility.config.facility_type {
            FacilityType::Overdraft => {}
            _ => {
                return Err(FacilityError::InvalidConfiguration {
                    message: "Not an overdraft configuration".to_string(),
                });
            }
        }
        
        let overdraft_limit = facility.config.limits.overdraft_limit
            .unwrap_or(facility.config.financial_terms.commitment_amount);
        
        Ok(Self {
            facility,
            time: None,
            overdraft_limit,
            buffer_zone,
            _linked_account_id: linked_account_id,
            linked_account_balance: Money::ZERO,
            is_active: false,
            daily_fee: None,
            state: OverdraftState::Available,
        })
    }
    
    /// set the time provider for this facility
    pub fn set_time(&mut self, time: &SafeTimeProvider) {
        self.time = Some(time as *const SafeTimeProvider);
    }
    
    /// process account transaction using stored time
    pub fn process_account_transaction(&mut self, transaction_amount: Money) -> Result<Money> {
        let time_ptr = self.time
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Time provider not set. Call set_time() first".to_string(),
            })?;
        let time = unsafe { time_ptr.as_ref() }
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Invalid time provider reference".to_string(),
            })?;
        self.process_account_transaction_with_time(transaction_amount, time)
    }
    
    /// process account transaction and activate overdraft if needed with explicit time
    pub fn process_account_transaction_with_time(
        &mut self,
        transaction_amount: Money,
        time_provider: &SafeTimeProvider,
    ) -> Result<Money> {
        let previous_balance = self.linked_account_balance;
        let new_balance = previous_balance + transaction_amount;
        
        // check if transaction would exceed overdraft
        if new_balance < Money::ZERO - self.overdraft_limit {
            return Err(FacilityError::OverdraftLimitExceeded {
                limit: self.overdraft_limit,
                requested: (Money::ZERO - new_balance) - self.overdraft_limit,
            });
        }
        
        self.linked_account_balance = new_balance;
        
        // handle overdraft activation/deactivation
        if previous_balance >= Money::ZERO && new_balance < Money::ZERO {
            // activate overdraft
            self.activate_overdraft(Money::ZERO - new_balance, time_provider)?;
        } else if previous_balance < Money::ZERO && new_balance >= Money::ZERO {
            // clear overdraft
            self.clear_overdraft(time_provider)?;
        } else if new_balance < Money::ZERO {
            // update overdraft amount
            self.update_overdraft_amount(Money::ZERO - new_balance, time_provider)?;
        }
        
        Ok(new_balance)
    }
    
    /// activate overdraft
    fn activate_overdraft(
        &mut self,
        amount: Money,
        time_provider: &SafeTimeProvider,
    ) -> Result<()> {
        self.is_active = true;
        self.facility.state.outstanding_principal = amount;
        self.facility.state.update_status(FacilityStatus::Active, time_provider.now());
        
        // determine state based on amount
        self.state = if amount <= self.buffer_zone {
            OverdraftState::BufferZone
        } else if amount <= self.overdraft_limit {
            OverdraftState::Active
        } else {
            OverdraftState::Exceeded
        };
        
        // emit activation event
        self.facility.events.emit(Event::OverdraftActivated {
            facility_id: self.facility.id,
            account_balance: self.linked_account_balance,
            overdraft_amount: amount,
            timestamp: time_provider.now(),
        });
        
        // check buffer zone
        if amount > self.buffer_zone {
            self.facility.events.emit(Event::BufferZoneBreached {
                facility_id: self.facility.id,
                amount_over_buffer: amount - self.buffer_zone,
                timestamp: time_provider.now(),
            });
        }
        
        Ok(())
    }
    
    /// clear overdraft
    fn clear_overdraft(&mut self, time_provider: &SafeTimeProvider) -> Result<()> {
        let previous_amount = self.facility.state.outstanding_principal;
        
        self.is_active = false;
        self.facility.state.outstanding_principal = Money::ZERO;
        self.state = OverdraftState::Available;
        
        // emit cleared event
        self.facility.events.emit(Event::OverdraftCleared {
            facility_id: self.facility.id,
            repayment_amount: previous_amount,
            timestamp: time_provider.now(),
        });
        
        Ok(())
    }
    
    /// update overdraft amount
    fn update_overdraft_amount(
        &mut self,
        new_amount: Money,
        time_provider: &SafeTimeProvider,
    ) -> Result<()> {
        let previous_amount = self.facility.state.outstanding_principal;
        self.facility.state.outstanding_principal = new_amount;
        
        // update state
        self.state = if new_amount <= self.buffer_zone {
            OverdraftState::BufferZone
        } else if new_amount <= self.overdraft_limit {
            OverdraftState::Active
        } else {
            OverdraftState::Exceeded
        };
        
        // emit appropriate event
        if new_amount > previous_amount {
            self.facility.events.emit(Event::OverdraftIncreased {
                facility_id: self.facility.id,
                additional_amount: new_amount - previous_amount,
                new_total: new_amount,
                timestamp: time_provider.now(),
            });
        } else {
            // this is a partial repayment
            let repayment = previous_amount - new_amount;
            self.facility.state.record_payment(repayment, time_provider.now());
        }
        
        // check buffer zone breach
        if previous_amount <= self.buffer_zone && new_amount > self.buffer_zone {
            self.facility.events.emit(Event::BufferZoneBreached {
                facility_id: self.facility.id,
                amount_over_buffer: new_amount - self.buffer_zone,
                timestamp: time_provider.now(),
            });
        }
        
        Ok(())
    }
    
    /// accrue interest using stored time
    pub fn accrue_interest(&mut self) -> Result<()> {
        let time_ptr = self.time
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Time provider not set. Call set_time() first".to_string(),
            })?;
        let time = unsafe { time_ptr.as_ref() }
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Invalid time provider reference".to_string(),
            })?;
        self.accrue_interest_with_time(time)
    }
    
    /// accrue interest with continuous compounding with explicit time
    pub fn accrue_interest_with_time(&mut self, time_provider: &SafeTimeProvider) -> Result<()> {
        if !self.is_active || self.facility.state.outstanding_principal.is_zero() {
            return Ok(());
        }
        
        let principal = self.facility.state.outstanding_principal;
        let rate = self.facility.config.financial_terms.interest_rate;
        
        // continuous compounding: A = P * e^(rt)
        // for daily: interest = P * (e^(r/365) - 1)
        let daily_rate = rate.as_decimal() / dec!(365);
        
        // approximate e^x for small x: e^x ≈ 1 + x + x²/2
        // for daily rates this is very accurate
        let e_power = dec!(1) + daily_rate + (daily_rate * daily_rate) / dec!(2);
        let interest = principal.as_decimal() * (e_power - dec!(1));
        
        self.facility.state.accrued_interest += Money::from_decimal(interest);
        
        // emit interest event
        self.facility.events.emit(Event::InterestAccrued {
            facility_id: self.facility.id,
            amount: Money::from_decimal(interest),
            timestamp: time_provider.now(),
        });
        
        Ok(())
    }
    
    /// apply daily fees using stored time
    pub fn apply_daily_fees(&mut self) -> Result<()> {
        let time_ptr = self.time
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Time provider not set. Call set_time() first".to_string(),
            })?;
        let time = unsafe { time_ptr.as_ref() }
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Invalid time provider reference".to_string(),
            })?;
        self.apply_daily_fees_with_time(time)
    }
    
    /// apply daily fees if applicable with explicit time
    pub fn apply_daily_fees_with_time(&mut self, _time_provider: &SafeTimeProvider) -> Result<()> {
        if !self.is_active {
            return Ok(());
        }
        
        let mut fee = Money::ZERO;
        
        // apply fee based on state
        match self.state {
            OverdraftState::Active => {
                // fee on amount over buffer
                if self.facility.state.outstanding_principal > self.buffer_zone {
                    fee = self.daily_fee.unwrap_or(Money::from_major(5));
                }
            }
            OverdraftState::Exceeded => {
                // higher fee for exceeded
                fee = Money::from_major(10);
                
                // also apply overlimit fee
                if let Some(overlimit_fee) = self.facility.config.fee_config.overlimit_fee {
                    fee += overlimit_fee;
                }
            }
            _ => {}
        }
        
        if fee > Money::ZERO {
            self.facility.state.accrued_fees += fee;
            self.facility.state.total_fees_charged += fee;
        }
        
        Ok(())
    }
    
    /// get effective balance (account + available overdraft)
    pub fn effective_balance(&self) -> Money {
        if self.linked_account_balance >= Money::ZERO {
            self.linked_account_balance + self.overdraft_limit
        } else {
            self.overdraft_limit - self.linked_account_balance.abs()
        }
    }
    
    /// get available funds
    pub fn available_funds(&self) -> Money {
        if self.linked_account_balance >= Money::ZERO {
            self.linked_account_balance + self.overdraft_limit
        } else {
            let used = self.linked_account_balance.abs();
            if used < self.overdraft_limit {
                self.overdraft_limit - used
            } else {
                Money::ZERO
            }
        }
    }
    
    /// check if can process transaction
    pub fn can_process_transaction(&self, amount: Money) -> bool {
        let new_balance = self.linked_account_balance - amount;
        new_balance >= Money::ZERO - self.overdraft_limit
    }
    
    /// get facility reference
    pub fn facility(&self) -> &Facility {
        &self.facility
    }
    
    /// get mutable facility reference
    pub fn facility_mut(&mut self) -> &mut Facility {
        &mut self.facility
    }
    
    /// get json representation of current state
    pub fn to_json_pretty(&self) -> String {
        use super::serialization::{FacilityView, OverdraftView};
        
        let view = OverdraftView {
            facility: FacilityView::from_facility(&self.facility),
            overdraft_limit: self.overdraft_limit,
            buffer_zone: self.buffer_zone,
            linked_account_balance: self.linked_account_balance,
            is_active: self.is_active,
            available_funds: self.available_funds(),
        };
        
        serde_json::to_string_pretty(&view).unwrap_or_else(|e| format!("JSON error: {}", e))
    }
    
    /// short alias for json output
    pub fn json(&self) -> String {
        self.to_json_pretty()
    }
    
    /// approve the facility (activate it)
    pub fn approve(&mut self) -> Result<()> {
        let time_ptr = self.time
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Time provider not set. Call set_time() first".to_string(),
            })?;
        let time = unsafe { time_ptr.as_ref() }
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Invalid time provider reference".to_string(),
            })?;
        
        self.facility.state.update_status(FacilityStatus::Active, time.now());
        self.facility.state.activation_date = Some(time.now());
        
        self.facility.events.emit(Event::FacilityActivated {
            facility_id: self.facility.id,
            first_disbursement: Money::ZERO,
            timestamp: time.now(),
        });
        
        Ok(())
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
        self.state = OverdraftState::Suspended;
        
        self.facility.events.emit(Event::StatusChanged {
            facility_id: self.facility.id,
            old_status,
            new_status: FacilityStatus::Settled,
            reason: "Denied by lender - no funds disbursed".to_string(),
            timestamp: time.now(),
        });
        
        Ok(())
    }
    
    /// make payment (process positive balance transaction)
    pub fn make_payment(&mut self, amount: Money) -> Result<Money> {
        self.process_account_transaction(amount)
    }
    
    /// disburse funds (process negative balance transaction)
    pub fn disburse(&mut self, amount: Money) -> Result<Money> {
        self.process_account_transaction(Money::ZERO - amount)
    }
}

/// builder for overdraft facilities
pub struct OverdraftBuilder {
    overdraft_limit: Option<Money>,
    rate: Option<Rate>,
    buffer_zone: Option<Money>,
    linked_account_id: Option<String>,
    daily_fee: Option<Money>,
    account_number: Option<String>,
    customer_id: Option<String>,
    time_provider: Option<*const SafeTimeProvider>,
}

impl OverdraftBuilder {
    pub fn new() -> Self {
        Self {
            overdraft_limit: None,
            rate: None,
            buffer_zone: None,
            linked_account_id: None,
            daily_fee: None,
            account_number: None,
            customer_id: None,
            time_provider: None,
        }
    }
    
    pub fn set_time(mut self, time: &SafeTimeProvider) -> Self {
        self.time_provider = Some(time as *const SafeTimeProvider);
        self
    }
    
    pub fn overdraft_limit(mut self, limit: Money) -> Self {
        self.overdraft_limit = Some(limit);
        self
    }
    
    pub fn rate(mut self, rate: Rate) -> Self {
        self.rate = Some(rate);
        self
    }
    
    pub fn buffer_zone(mut self, buffer: Money) -> Self {
        self.buffer_zone = Some(buffer);
        self
    }
    
    pub fn linked_account_id(mut self, account_id: String) -> Self {
        self.linked_account_id = Some(account_id);
        self
    }
    
    pub fn daily_fee(mut self, fee: Money) -> Self {
        self.daily_fee = Some(fee);
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
    pub fn build(self) -> Result<OverdraftFacility> {
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
    pub fn build_now(self) -> Result<OverdraftFacility> {
        let time = SafeTimeProvider::new(hourglass_rs::TimeSource::System);
        self.build_with_time(&time)
    }
    
    /// Build with explicit time provider (for backward compatibility)
    pub fn build_with_time(self, time_provider: &SafeTimeProvider) -> Result<OverdraftFacility> {
        let overdraft_limit = self.overdraft_limit.ok_or(FacilityError::InvalidConfiguration {
            message: "Overdraft limit required".to_string(),
        })?;
        
        let rate = self.rate.ok_or(FacilityError::InvalidConfiguration {
            message: "Rate required".to_string(),
        })?;
        
        let buffer_zone = self.buffer_zone.unwrap_or(Money::from_major(50));
        
        let linked_account_id = self.linked_account_id.ok_or(FacilityError::InvalidConfiguration {
            message: "Linked account ID required".to_string(),
        })?;
        
        let config = FacilityConfig::overdraft(
            overdraft_limit,
            rate,
            buffer_zone,
            linked_account_id.clone(),
        );
        
        let account_number = self.account_number.unwrap_or_else(|| {
            format!("OD-{}", Uuid::new_v4().to_string()[..8].to_uppercase())
        });
        
        let customer_id = self.customer_id.unwrap_or_else(|| {
            format!("CUST-{}", Uuid::new_v4().to_string()[..8].to_uppercase())
        });
        
        let facility = Facility::originate(config, account_number, customer_id, time_provider)?;
        
        let mut overdraft = OverdraftFacility::new(facility, buffer_zone, linked_account_id)?;
        overdraft.daily_fee = self.daily_fee;
        
        // If time was set in builder, pass it to the facility
        if let Some(time_ptr) = self.time_provider {
            overdraft.time = Some(time_ptr);
        }
        
        Ok(overdraft)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hourglass_rs::TimeSource;
    use chrono::{TimeZone, Utc};
    
    #[test]
    fn test_overdraft_activation() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        
        let mut overdraft = OverdraftBuilder::new()
            .overdraft_limit(Money::from_major(1000))
            .rate(Rate::from_percentage(20))
            .buffer_zone(Money::from_major(50))
            .linked_account_id("ACC-123".to_string())
            .set_time(&time)
            .build()
            .unwrap();
        
        // start with positive balance
        overdraft.process_account_transaction_with_time(Money::from_major(500), &time).unwrap();
        assert_eq!(overdraft.linked_account_balance, Money::from_major(500));
        assert!(!overdraft.is_active);
        
        // transaction that goes negative
        let balance = overdraft.process_account_transaction_with_time(Money::ZERO - Money::from_major(600), &time).unwrap();
        assert_eq!(balance, Money::ZERO - Money::from_major(100));
        assert!(overdraft.is_active);
        assert_eq!(overdraft.facility.state.outstanding_principal, Money::from_major(100));
    }
    
    #[test]
    fn test_buffer_zone() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        
        let mut overdraft = OverdraftBuilder::new()
            .overdraft_limit(Money::from_major(1000))
            .rate(Rate::from_percentage(20))
            .buffer_zone(Money::from_major(50))
            .linked_account_id("ACC-123".to_string())
            .set_time(&time)
            .build()
            .unwrap();
        
        // within buffer zone
        overdraft.process_account_transaction_with_time(Money::ZERO - Money::from_major(30), &time).unwrap();
        assert_eq!(overdraft.state, OverdraftState::BufferZone);
        
        // exceed buffer zone
        overdraft.process_account_transaction_with_time(Money::ZERO - Money::from_major(40), &time).unwrap();
        assert_eq!(overdraft.state, OverdraftState::Active);
        assert_eq!(overdraft.facility.state.outstanding_principal, Money::from_major(70));
    }
    
    #[test]
    fn test_continuous_interest() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        let control = time.test_control().unwrap();
        
        let mut overdraft = OverdraftBuilder::new()
            .overdraft_limit(Money::from_major(1000))
            .rate(Rate::from_percentage(20))
            .buffer_zone(Money::from_major(50))
            .linked_account_id("ACC-123".to_string())
            .set_time(&time)
            .build()
            .unwrap();
        
        // activate overdraft
        overdraft.process_account_transaction_with_time(Money::ZERO - Money::from_major(500), &time).unwrap();
        
        // accrue interest for 5 days
        for _ in 0..5 {
            control.advance(chrono::Duration::days(1));
            overdraft.accrue_interest_with_time(&time).unwrap();
        }
        
        // should have accrued interest
        assert!(overdraft.facility.state.accrued_interest > Money::ZERO);
        
        // continuous compounding should be slightly higher than simple interest
        let simple_daily = Money::from_major(500) * Rate::from_percentage(20).as_decimal() / dec!(365);
        let total_simple = simple_daily * dec!(5);
        assert!(overdraft.facility.state.accrued_interest > total_simple);
    }
    
    #[test]
    fn test_overdraft_clearing() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        
        let mut overdraft = OverdraftBuilder::new()
            .overdraft_limit(Money::from_major(1000))
            .rate(Rate::from_percentage(20))
            .buffer_zone(Money::from_major(50))
            .linked_account_id("ACC-123".to_string())
            .set_time(&time)
            .build()
            .unwrap();
        
        // activate overdraft
        overdraft.process_account_transaction_with_time(Money::ZERO - Money::from_major(200), &time).unwrap();
        assert!(overdraft.is_active);
        
        // deposit clears overdraft
        overdraft.process_account_transaction_with_time(Money::from_major(250), &time).unwrap();
        assert!(!overdraft.is_active);
        assert_eq!(overdraft.linked_account_balance, Money::from_major(50));
        assert_eq!(overdraft.facility.state.outstanding_principal, Money::ZERO);
    }
    
    #[test]
    fn test_overdraft_limit_exceeded() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        
        let mut overdraft = OverdraftBuilder::new()
            .overdraft_limit(Money::from_major(1000))
            .rate(Rate::from_percentage(20))
            .buffer_zone(Money::from_major(50))
            .linked_account_id("ACC-123".to_string())
            .set_time(&time)
            .build()
            .unwrap();
        
        // try to exceed limit
        let result = overdraft.process_account_transaction_with_time(Money::ZERO - Money::from_major(1200), &time);
        assert!(result.is_err());
        
        // balance should be unchanged
        assert_eq!(overdraft.linked_account_balance, Money::ZERO);
    }
    
    #[test]
    fn test_effective_balance() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        
        let mut overdraft = OverdraftBuilder::new()
            .overdraft_limit(Money::from_major(1000))
            .rate(Rate::from_percentage(20))
            .buffer_zone(Money::from_major(50))
            .linked_account_id("ACC-123".to_string())
            .set_time(&time)
            .build()
            .unwrap();
        
        // positive balance
        overdraft.process_account_transaction_with_time(Money::from_major(500), &time).unwrap();
        assert_eq!(overdraft.effective_balance(), Money::from_major(1500));
        assert_eq!(overdraft.available_funds(), Money::from_major(1500));
        
        // negative balance
        overdraft.process_account_transaction_with_time(Money::ZERO - Money::from_major(700), &time).unwrap();
        assert_eq!(overdraft.effective_balance(), Money::from_major(800));
        assert_eq!(overdraft.available_funds(), Money::from_major(800));
    }
}