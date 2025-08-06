use chrono::{DateTime, Utc};
use hourglass_rs::SafeTimeProvider;
use rust_decimal::Decimal;
use uuid::Uuid;

use crate::config::{FacilityConfig, FacilityType};
use crate::types::OpenTermType;
use crate::decimal::{Money, Rate};
use crate::errors::{FacilityError, Result};
use crate::events::Event;
use crate::facility::Facility;
use crate::types::{FacilityStatus, LtvStatus};

/// open-term loan facility (perpetual, collateral-backed)
pub struct OpenTermLoan {
    facility: Facility,
    time: Option<*const SafeTimeProvider>,
    btc_amount: Decimal,
    btc_price: Money,
    last_ltv_check: DateTime<Utc>,
    margin_call_active: bool,
    margin_call_deadline: Option<DateTime<Utc>>,
}

impl OpenTermLoan {
    /// create new open-term loan
    pub fn new(facility: Facility, btc_amount: Decimal, btc_price: Money) -> Result<Self> {
        // validate it's an open-term loan
        match &facility.config.facility_type {
            FacilityType::OpenTermLoan(_) => {}
            _ => {
                return Err(FacilityError::InvalidConfiguration {
                    message: "Not an open-term loan configuration".to_string(),
                });
            }
        }

        Ok(Self {
            facility,
            time: None,
            btc_amount,
            btc_price,
            last_ltv_check: Utc::now(),
            margin_call_active: false,
            margin_call_deadline: None,
        })
    }

    /// builder for creating open-term loans
    pub fn builder() -> OpenTermLoanBuilder {
        OpenTermLoanBuilder::new()
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
        
        // update collateral in state
        let collateral_value = Money::from_decimal(self.btc_amount * self.btc_price.as_decimal());
        self.facility.state.update_collateral_value(collateral_value, time.now());
        
        if let crate::state::FacilitySpecificState::OpenTerm {
            collateral_amount,
            ..
        } = &mut self.facility.state.facility_specific {
            *collateral_amount = format!("{} BTC", self.btc_amount);
        }
        
        // set status to active
        self.facility.state.update_status(FacilityStatus::Active, time.now());
        self.facility.state.activation_date = Some(time.now());
        
        // emit origination event
        self.facility.events.emit(Event::FacilityOriginated {
            facility_id: self.facility.id,
            amount: self.facility.config.financial_terms.commitment_amount,
            collateral_type: "BTC".to_string(),
            collateral_amount: self.btc_amount,
        });
        
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
        
        // disburse the requested amount
        let disbursed = self.facility.disburse(amount, time)?;
        
        // no payment schedule for open-term loans
        self.facility.state.next_payment_due = None;
        self.facility.state.next_payment_amount = None;
        self.facility.state.minimum_payment_due = None;
        
        Ok(disbursed)
    }
    
    /// originate and disburse using stored time (convenience method)
    pub fn originate_and_disburse(&mut self) -> Result<Money> {
        self.originate()?;
        let amount = self.facility.config.financial_terms.commitment_amount;
        self.disburse(amount)
    }
    
    /// originate and disburse open-term loan with explicit time
    pub fn originate_and_disburse_with_time(
        &mut self,
        time_provider: &SafeTimeProvider,
    ) -> Result<Money> {
        let amount = self.facility.config.financial_terms.commitment_amount;

        // update collateral in state
        let collateral_value = Money::from_decimal(self.btc_amount * self.btc_price.as_decimal());
        self.facility.state.update_collateral_value(collateral_value, time_provider.now());

        if let crate::state::FacilitySpecificState::OpenTerm {
            collateral_amount,
            ..
        } = &mut self.facility.state.facility_specific {
            *collateral_amount = format!("{} BTC", self.btc_amount);
        }

        // disburse full amount
        let disbursed = self.facility.disburse(amount, time_provider)?;

        // no payment schedule for open-term loans
        self.facility.state.next_payment_due = None;
        self.facility.state.next_payment_amount = None;
        self.facility.state.minimum_payment_due = None;

        // emit origination event
        self.facility.events.emit(Event::FacilityOriginated {
            facility_id: self.facility.id,
            amount,
            collateral_type: "BTC".to_string(),
            collateral_amount: self.btc_amount,
        });

        Ok(disbursed)
    }

    /// update bitcoin price and check ltv using stored time
    pub fn update_btc_price(&mut self, new_price: Money) -> Result<LtvStatus> {
        let time_ptr = self.time
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Time provider not set. Call set_time() first".to_string(),
            })?;
        let time = unsafe { time_ptr.as_ref() }
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Invalid time provider reference".to_string(),
            })?;
        self.update_btc_price_with_time(new_price, time)
    }
    
    /// update bitcoin price and check ltv with explicit time
    pub fn update_btc_price_with_time(
        &mut self,
        new_price: Money,
        time_provider: &SafeTimeProvider,
    ) -> Result<LtvStatus> {
        self.btc_price = new_price;
        let collateral_value = Money::from_decimal(self.btc_amount * new_price.as_decimal());

        self.facility.state.update_collateral_value(collateral_value, time_provider.now());
        self.last_ltv_check = time_provider.now();

        // calculate and check ltv
        let ltv = self.calculate_ltv();
        let status = self.check_ltv_status(ltv, time_provider)?;

        // emit ltv update event
        self.facility.events.emit(Event::LtvCalculated {
            facility_id: self.facility.id,
            ltv_ratio: ltv,
            collateral_value,
            total_debt: self.facility.state.total_outstanding(),
            timestamp: time_provider.now(),
        });

        Ok(status)
    }

    /// calculate current ltv
    pub fn calculate_ltv(&self) -> Rate {
        let collateral_value = Money::from_decimal(self.btc_amount * self.btc_price.as_decimal());
        if collateral_value.is_zero() {
            return Rate::from_percentage(100); // max ltv if no collateral
        }

        let total_debt = self.facility.state.total_outstanding();
        Rate::from_decimal(total_debt.as_decimal() / collateral_value.as_decimal())
    }

    /// check ltv status and trigger events
    fn check_ltv_status(
        &mut self,
        ltv: Rate,
        time_provider: &SafeTimeProvider,
    ) -> Result<LtvStatus> {
        let config = &self.facility.config.collateral_config
            .as_ref()
            .ok_or(FacilityError::InvalidConfiguration {
                message: "No collateral configuration".to_string(),
            })?;

        let thresholds = &config.ltv_thresholds;
        let now = time_provider.now();

        // determine status
        let status = if ltv <= thresholds.warning_ltv {
            LtvStatus::Healthy
        } else if ltv <= thresholds.margin_call_ltv {
            LtvStatus::Warning
        } else if ltv <= thresholds.liquidation_ltv {
            LtvStatus::MarginCall
        } else {
            LtvStatus::Liquidation
        };

        // handle status transitions
        match status {
            LtvStatus::Healthy => {
                if self.margin_call_active {
                    self.margin_call_active = false;
                    self.margin_call_deadline = None;

                    self.facility.events.emit(Event::MarginCallResolved {
                        facility_id: self.facility.id,
                        new_ltv: ltv,
                        timestamp: now,
                    });
                }
            }
            LtvStatus::Warning => {
                self.facility.events.emit(Event::LtvWarningBreached {
                    facility_id: self.facility.id,
                    ltv_ratio: ltv,
                    threshold: thresholds.warning_ltv,
                    timestamp: now,
                });
            }
            LtvStatus::MarginCall => {
                if !self.margin_call_active {
                    self.margin_call_active = true;
                    self.margin_call_deadline = Some(
                        now + chrono::Duration::days(config.margin_call_days as i64)
                    );

                    self.facility.events.emit(Event::MarginCallRequired {
                        facility_id: self.facility.id,
                        current_ltv: ltv,
                        required_ltv: thresholds.margin_call_ltv,
                        deadline: self.margin_call_deadline.unwrap(),
                        options: vec![
                            "Add more BTC collateral".to_string(),
                            "Pay down principal".to_string(),
                            "Pay off loan to retrieve BTC".to_string(),
                        ],
                    });
                }
            }
            LtvStatus::Liquidation => {
                self.facility.state.update_status(FacilityStatus::Liquidating, now);

                self.facility.events.emit(Event::LiquidationTriggered {
                    facility_id: self.facility.id,
                    ltv_ratio: ltv,
                    collateral_value: Money::from_decimal(self.btc_amount * self.btc_price.as_decimal()),
                    debt_amount: self.facility.state.total_outstanding(),
                    timestamp: now,
                });
            }
        }

        Ok(status)
    }

    /// add bitcoin collateral using stored time
    pub fn add_collateral(&mut self, additional_btc: Decimal) -> Result<()> {
        let time_ptr = self.time
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Time provider not set. Call set_time() first".to_string(),
            })?;
        let time = unsafe { time_ptr.as_ref() }
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Invalid time provider reference".to_string(),
            })?;
        self.add_collateral_with_time(additional_btc, time)
    }
    
    /// add bitcoin collateral with explicit time
    pub fn add_collateral_with_time(
        &mut self,
        additional_btc: Decimal,
        time_provider: &SafeTimeProvider,
    ) -> Result<()> {
        self.btc_amount += additional_btc;

        // update collateral value
        let new_value = Money::from_decimal(self.btc_amount * self.btc_price.as_decimal());
        self.facility.state.update_collateral_value(new_value, time_provider.now());

        if let crate::state::FacilitySpecificState::OpenTerm {
            collateral_amount,
            ..
        } = &mut self.facility.state.facility_specific {
            *collateral_amount = format!("{} BTC", self.btc_amount);
        }

        // emit event
        self.facility.events.emit(Event::CollateralAdded {
            facility_id: self.facility.id,
            amount: additional_btc,
            new_total: self.btc_amount,
            new_ltv: self.calculate_ltv(),
            timestamp: time_provider.now(),
        });

        // recheck ltv status
        self.check_ltv_status(self.calculate_ltv(), time_provider)?;

        Ok(())
    }

    /// process payment using stored time (optional for open-term loans)
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
    
    /// process payment with explicit time (optional for open-term loans)
    pub fn process_payment_with_time(
        &mut self,
        amount: Money,
        time_provider: &SafeTimeProvider,
    ) -> Result<()> {
        // apply payment through standard waterfall
        let _result = self.facility.process_payment(amount, time_provider)?;

        // recheck ltv after payment
        self.check_ltv_status(self.calculate_ltv(), time_provider)?;

        Ok(())
    }

    /// accrue interest using stored time (no payment requirement)
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
    
    /// accrue interest with explicit time (no payment requirement)
    pub fn accrue_interest_with_time(&mut self, time_provider: &SafeTimeProvider) -> Result<()> {
        self.facility.accrue_interest(time_provider)?;

        // emit specific event for open-term
        self.facility.events.emit(Event::InterestAccrued {
            facility_id: self.facility.id,
            amount: self.facility.state.accrued_interest,
            timestamp: time_provider.now(),
        });

        // no payment due event for open-term
        // no delinquency triggered

        Ok(())
    }

    /// check if loan can be paid off
    pub fn total_payoff_amount(&self) -> Money {
        self.facility.state.total_outstanding()
    }

    /// pay off loan and release collateral using stored time
    pub fn payoff_and_release(&mut self) -> Result<Decimal> {
        let time_ptr = self.time
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Time provider not set. Call set_time() first".to_string(),
            })?;
        let time = unsafe { time_ptr.as_ref() }
            .ok_or(FacilityError::InvalidConfiguration {
                message: "Invalid time provider reference".to_string(),
            })?;
        self.payoff_and_release_with_time(time)
    }
    
    /// pay off loan and release collateral with explicit time
    pub fn payoff_and_release_with_time(
        &mut self,
        time_provider: &SafeTimeProvider,
    ) -> Result<Decimal> {
        let payoff = self.total_payoff_amount();

        // process full payment
        self.facility.process_payment(payoff, time_provider)?;

        // update status
        self.facility.state.update_status(FacilityStatus::Settled, time_provider.now());

        // emit collateral release event
        self.facility.events.emit(Event::CollateralReleased {
            facility_id: self.facility.id,
            collateral_type: "BTC".to_string(),
            amount: self.btc_amount,
            timestamp: time_provider.now(),
        });

        Ok(self.btc_amount)
    }

    /// get facility reference
    pub fn facility(&self) -> &Facility {
        &self.facility
    }

    /// get mutable facility reference
    pub fn facility_mut(&mut self) -> &mut Facility {
        &mut self.facility
    }

    /// get current btc collateral amount
    pub fn btc_collateral(&self) -> Decimal {
        self.btc_amount
    }

    /// get current btc price
    pub fn btc_price(&self) -> Money {
        self.btc_price
    }

    /// check if margin call is active
    pub fn is_margin_call_active(&self) -> bool {
        self.margin_call_active
    }
    
    /// get json representation of current state
    pub fn to_json_pretty(&self) -> String {
        use super::serialization::{FacilityView, OpenTermView};
        
        let view = OpenTermView {
            facility: FacilityView::from_facility(&self.facility),
            btc_collateral: self.btc_amount,
            btc_price: self.btc_price,
            collateral_value: Money::from_decimal(self.btc_amount * self.btc_price.as_decimal()),
            ltv_ratio: self.calculate_ltv(),
            margin_call_active: self.margin_call_active,
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
}

/// builder for open-term loans
pub struct OpenTermLoanBuilder {
    loan_type: Option<OpenTermType>,
    amount: Option<Money>,
    rate: Option<Rate>,
    btc_amount: Option<Decimal>,
    btc_price: Option<Money>,
    account_number: Option<String>,
    customer_id: Option<String>,
    time_provider: Option<*const SafeTimeProvider>,
}

impl OpenTermLoanBuilder {
    pub fn new() -> Self {
        Self {
            loan_type: None,
            amount: None,
            rate: None,
            btc_amount: None,
            btc_price: None,
            account_number: None,
            customer_id: None,
            time_provider: None,
        }
    }
    
    pub fn set_time(mut self, time: &SafeTimeProvider) -> Self {
        self.time_provider = Some(time as *const SafeTimeProvider);
        self
    }
    
    pub fn loan_type(mut self, loan_type: OpenTermType) -> Self {
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

    pub fn btc_collateral(mut self, btc: Decimal) -> Self {
        self.btc_amount = Some(btc);
        self
    }

    pub fn btc_price(mut self, price: Money) -> Self {
        self.btc_price = Some(price);
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
    pub fn build(self) -> Result<OpenTermLoan> {
        if let Some(time_ptr) = self.time_provider {
            let time = unsafe { time_ptr.as_ref() }
                .ok_or(FacilityError::InvalidConfiguration {
                    message: "Invalid time provider reference".to_string(),
                })?;
            self.build_with_time(time)
        } else {
            let time = SafeTimeProvider::new(hourglass_rs::TimeSource::System);
            self.set_time(&time)
            .build()
        }
    }
    
    /// Build with system time
    pub fn build_now(self) -> Result<OpenTermLoan> {
        let time = SafeTimeProvider::new(hourglass_rs::TimeSource::System);
        self.set_time(&time)
            .build()
    }
    
    /// Build with explicit time provider (for backward compatibility)
    pub fn build_with_time(self, time_provider: &SafeTimeProvider) -> Result<OpenTermLoan> {
        let amount = self.amount.ok_or(FacilityError::InvalidConfiguration {
            message: "Amount required".to_string(),
        })?;

        let rate = self.rate.ok_or(FacilityError::InvalidConfiguration {
            message: "Rate required".to_string(),
        })?;

        // Infer loan type based on collateral, default to BitcoinBacked if BTC provided
        let loan_type = self.loan_type.unwrap_or_else(|| {
            if self.btc_amount.is_some() {
                OpenTermType::BitcoinBacked
            } else {
                OpenTermType::AssetBacked
            }
        });

        // For now, we only support Bitcoin-backed loans
        // Future: Add support for other asset types
        if loan_type != OpenTermType::BitcoinBacked {
            return Err(FacilityError::InvalidConfiguration {
                message: "Currently only Bitcoin-backed loans are supported".to_string(),
            });
        }

        let btc_amount = self.btc_amount.ok_or(FacilityError::InvalidConfiguration {
            message: "BTC collateral required".to_string(),
        })?;

        let btc_price = self.btc_price.ok_or(FacilityError::InvalidConfiguration {
            message: "BTC price required".to_string(),
        })?;

        let config = FacilityConfig::bitcoin_backed_loan(
            amount,
            rate,
            btc_amount,
            btc_price,
        );

        let account_number = self.account_number.unwrap_or_else(|| {
            format!("BTC-{}", Uuid::new_v4().to_string()[..8].to_uppercase())
        });

        let customer_id = self.customer_id.unwrap_or_else(|| {
            format!("CUST-{}", Uuid::new_v4().to_string()[..8].to_uppercase())
        });

        let facility = Facility::originate(config, account_number, customer_id, time_provider)?;

        let mut open_term = OpenTermLoan::new(facility, btc_amount, btc_price)?;
        
        // If time was set in builder, pass it to the loan
        if let Some(time_ptr) = self.time_provider {
            open_term.time = Some(time_ptr);
        }
        
        Ok(open_term)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hourglass_rs::TimeSource;
    use chrono::TimeZone;
    use rust_decimal_macros::dec;

    #[test]
    fn test_open_term_creation() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));

        let mut loan = OpenTermLoan::builder()
            .amount(Money::from_major(50_000))
            .rate(Rate::from_percentage(5))
            .btc_collateral(dec!(2))
            .btc_price(Money::from_major(50_000))
            .account_number("BTC-001".to_string())
            .customer_id("CUST-123".to_string())
            .set_time(&time)
            .build()
            .unwrap();

        assert_eq!(loan.facility.state.original_commitment, Money::from_major(50_000));
        assert_eq!(loan.btc_amount, dec!(2));

        // ltv is low before disbursement (only origination fee)
        let initial_ltv = loan.calculate_ltv();
        assert!(initial_ltv < Rate::from_percentage(1), "Initial LTV is {:?}", initial_ltv);

        // disburse the loan
        loan.originate_and_disburse().unwrap();

        // now ltv should be ~50.5% (50k principal + 500 fee) / 100k collateral
        let final_ltv = loan.calculate_ltv();
        assert!(final_ltv >= Rate::from_percentage(50) && final_ltv <= Rate::from_percentage(51), "Final LTV is {:?}", final_ltv);
    }

    #[test]
    fn test_no_payment_schedule() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));

        let mut loan = OpenTermLoan::builder()
            .amount(Money::from_major(50_000))
            .rate(Rate::from_percentage(5))
            .btc_collateral(dec!(2))
            .btc_price(Money::from_major(50_000))
            .set_time(&time)
            .build()
            .unwrap();

        loan.set_time(&time);
        loan.originate_and_disburse().unwrap();

        // no payment schedule
        assert!(loan.facility.state.next_payment_due.is_none());
        assert!(loan.facility.state.next_payment_amount.is_none());
        assert!(loan.facility.state.minimum_payment_due.is_none());
    }

    #[test]
    fn test_interest_accrual_without_payment_requirement() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        let control = time.test_control().unwrap();

        let mut loan = OpenTermLoan::builder()
            .amount(Money::from_major(50_000))
            .rate(Rate::from_percentage(5))
            .btc_collateral(dec!(2))
            .btc_price(Money::from_major(50_000))
            .set_time(&time)
            .build()
            .unwrap();

        loan.originate_and_disburse().unwrap();

        // advance 1 year without payments
        for _ in 0..365 {
            control.advance(chrono::Duration::days(1));
            loan.accrue_interest().unwrap();
        }

        // loan still active despite no payments
        assert_eq!(loan.facility.state.status, FacilityStatus::Active);

        // interest has accrued
        assert!(loan.facility.state.accrued_interest > Money::ZERO);

        // no delinquency
        assert_eq!(loan.facility.state.days_past_due, 0);
    }

    #[test]
    fn test_ltv_monitoring() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));

        let mut loan = OpenTermLoan::builder()
            .amount(Money::from_major(50_000))
            .rate(Rate::from_percentage(5))
            .btc_collateral(dec!(2))
            .btc_price(Money::from_major(50_000))
            .set_time(&time)
            .build()
            .unwrap();

        loan.originate_and_disburse().unwrap();

        // healthy at 50% ltv (allow for small rounding differences due to origination fee)
        let ltv = loan.calculate_ltv();
        assert!(ltv >= Rate::from_percentage(50) && ltv <= Rate::from_percentage(51), "LTV is {:?}", ltv);

        // btc price drops to $35k
        let status = loan.update_btc_price(Money::from_major(35_000)).unwrap();

        // ltv increases to ~71%
        assert!(loan.calculate_ltv() > Rate::from_percentage(70));
        assert_eq!(status, LtvStatus::MarginCall);
        assert!(loan.is_margin_call_active());
    }

    #[test]
    fn test_margin_call_resolution() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));

        let mut loan = OpenTermLoan::builder()
            .amount(Money::from_major(50_000))
            .rate(Rate::from_percentage(5))
            .btc_collateral(dec!(2))
            .btc_price(Money::from_major(35_000))
            .set_time(&time)
            .build()
            .unwrap();

        loan.originate_and_disburse().unwrap();

        // trigger margin call by updating price (which internally checks ltv)
        loan.update_btc_price(loan.btc_price()).unwrap();
        assert!(loan.is_margin_call_active());

        // add more collateral
        loan.add_collateral(dec!(0.5)).unwrap();

        // margin call resolved
        assert!(!loan.is_margin_call_active());
        assert!(loan.calculate_ltv() < Rate::from_percentage(65));
    }

    #[test]
    fn test_optional_payments() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        let control = time.test_control().unwrap();

        let mut loan = OpenTermLoan::builder()
            .amount(Money::from_major(50_000))
            .rate(Rate::from_percentage(5))
            .btc_collateral(dec!(2))
            .btc_price(Money::from_major(50_000))
            .set_time(&time)
            .build()
            .unwrap();

        loan.originate_and_disburse().unwrap();

        // accrue interest for 30 days
        for _ in 0..30 {
            control.advance(chrono::Duration::days(1));
            loan.accrue_interest().unwrap();
        }

        let interest_before = loan.facility.state.accrued_interest;

        // make optional payment
        loan.process_payment(Money::from_major(1_000)).unwrap();

        // payment applied to interest
        assert!(loan.facility.state.accrued_interest < interest_before);
    }

    #[test]
    fn test_payoff_and_release() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        let control = time.test_control().unwrap();

        let mut loan = OpenTermLoan::builder()
            .amount(Money::from_major(50_000))
            .rate(Rate::from_percentage(5))
            .btc_collateral(dec!(2))
            .btc_price(Money::from_major(50_000))
            .set_time(&time)
            .build()
            .unwrap();

        loan.originate_and_disburse().unwrap();

        // accrue some interest
        control.advance(chrono::Duration::days(90));
        loan.accrue_interest().unwrap();

        // pay off loan
        let btc_released = loan.payoff_and_release().unwrap();

        assert_eq!(btc_released, dec!(2));
        assert_eq!(loan.facility.state.status, FacilityStatus::Settled);
        assert_eq!(loan.facility.state.total_outstanding(), Money::ZERO);
    }

    #[test]
    fn test_perpetual_loan_5_years() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        let control = time.test_control().unwrap();

        let mut loan = OpenTermLoan::builder()
            .amount(Money::from_major(50_000))
            .rate(Rate::from_percentage(5))
            .btc_collateral(dec!(2))
            .btc_price(Money::from_major(50_000))
            .set_time(&time)
            .build()
            .unwrap();

        loan.originate_and_disburse().unwrap();

        // run for 5 years with no payments
        for _ in 0..1825 {
            control.advance(chrono::Duration::days(1));
            loan.accrue_interest().unwrap();
        }

        // loan still active
        assert_eq!(loan.facility.state.status, FacilityStatus::Active);

        // significant interest accrued
        assert!(loan.facility.state.accrued_interest > Money::from_major(12_500));

        // can still pay off
        let payoff = loan.total_payoff_amount();
        assert!(payoff > Money::from_major(62_500)); // principal + 5 years interest
    }
}
