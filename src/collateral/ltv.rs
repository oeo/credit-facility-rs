use chrono::{DateTime, Duration, Utc};
use hourglass_rs::SafeTimeProvider;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use uuid::Uuid;

use crate::decimal::{Money, Rate};
use crate::errors::{FacilityError, Result};
use crate::events::Event;
use crate::types::{LtvStatus, LtvThresholds};

/// ltv calculator for facilities
pub struct LtvCalculator {
    facility_id: Uuid,
    thresholds: LtvThresholds,
}

impl LtvCalculator {
    /// create new ltv calculator
    pub fn new(facility_id: Uuid, thresholds: LtvThresholds) -> Self {
        Self {
            facility_id,
            thresholds,
        }
    }
    
    /// calculate current ltv ratio
    pub fn calculate_ltv(
        &self,
        cvl: Money,
        collateral_value: Money,
    ) -> Result<Rate> {
        if collateral_value.is_zero() {
            return Err(FacilityError::InvalidCollateral {
                message: "Collateral value cannot be zero".to_string(),
            });
        }
        
        Ok(Rate::from_decimal(cvl.as_decimal() / collateral_value.as_decimal()))
    }
    
    /// calculate current value of liability
    pub fn calculate_cvl(
        outstanding_principal: Money,
        accrued_interest: Money,
        accrued_fees: Money,
        accrued_penalties: Money,
    ) -> Money {
        outstanding_principal + accrued_interest + accrued_fees + accrued_penalties
    }
    
    /// determine ltv status based on thresholds
    pub fn get_ltv_status(&self, ltv: Rate) -> LtvStatus {
        let ltv_decimal = ltv.as_decimal();
        
        if ltv_decimal > self.thresholds.liquidation_ltv.as_decimal() {
            LtvStatus::Liquidation
        } else if ltv_decimal > self.thresholds.margin_call_ltv.as_decimal() {
            LtvStatus::MarginCall
        } else if ltv_decimal > self.thresholds.warning_ltv.as_decimal() {
            LtvStatus::Warning
        } else {
            LtvStatus::Healthy
        }
    }
    
    /// calculate required collateral to reach target ltv
    pub fn calculate_required_collateral(
        &self,
        cvl: Money,
        current_collateral: Money,
        target_ltv: Rate,
    ) -> Money {
        if target_ltv.as_decimal().is_zero() {
            return Money::from_decimal(Decimal::MAX);
        }
        
        let required_collateral_value = Money::from_decimal(
            cvl.as_decimal() / target_ltv.as_decimal()
        );
        
        if required_collateral_value > current_collateral {
            required_collateral_value - current_collateral
        } else {
            Money::ZERO
        }
    }
    
    /// calculate payment required to reach target ltv
    pub fn calculate_required_payment(
        &self,
        cvl: Money,
        collateral_value: Money,
        target_ltv: Rate,
    ) -> Money {
        let target_cvl = Money::from_decimal(
            collateral_value.as_decimal() * target_ltv.as_decimal()
        );
        
        if cvl > target_cvl {
            cvl - target_cvl
        } else {
            Money::ZERO
        }
    }
}

/// ltv monitor for continuous tracking
pub struct LtvMonitor {
    calculator: LtvCalculator,
    last_check: DateTime<Utc>,
    check_frequency: Duration,
    margin_call_active: bool,
    margin_call_deadline: Option<DateTime<Utc>>,
    events: Vec<Event>,
}

impl LtvMonitor {
    /// create new ltv monitor
    pub fn new(
        facility_id: Uuid,
        thresholds: LtvThresholds,
        check_frequency: Duration,
    ) -> Self {
        Self {
            calculator: LtvCalculator::new(facility_id, thresholds),
            last_check: DateTime::<Utc>::MIN_UTC,  // start with minimum date so first check always runs
            check_frequency,
            margin_call_active: false,
            margin_call_deadline: None,
            events: Vec::new(),
        }
    }
    
    /// check ltv and emit appropriate events
    pub fn check_ltv(
        &mut self,
        cvl: Money,
        collateral_value: Money,
        time_provider: &SafeTimeProvider,
    ) -> Result<LtvStatus> {
        let now = time_provider.now();
        
        // check if it's time to check
        if now - self.last_check < self.check_frequency {
            return Ok(self.get_current_status(cvl, collateral_value)?);
        }
        
        self.last_check = now;
        
        let ltv = self.calculator.calculate_ltv(cvl, collateral_value)?;
        let status = self.calculator.get_ltv_status(ltv);
        
        match status {
            LtvStatus::Healthy => {
                if self.margin_call_active {
                    self.resolve_margin_call(time_provider);
                }
            }
            LtvStatus::Warning => {
                self.emit_warning(ltv, time_provider);
            }
            LtvStatus::MarginCall => {
                if !self.margin_call_active {
                    self.issue_margin_call(ltv, cvl, collateral_value, time_provider)?;
                } else {
                    self.check_margin_call_deadline(time_provider)?;
                }
            }
            LtvStatus::Liquidation => {
                self.trigger_liquidation(ltv, cvl, collateral_value, time_provider);
            }
        }
        
        Ok(status)
    }
    
    /// get current status without checking
    fn get_current_status(&self, cvl: Money, collateral_value: Money) -> Result<LtvStatus> {
        let ltv = self.calculator.calculate_ltv(cvl, collateral_value)?;
        Ok(self.calculator.get_ltv_status(ltv))
    }
    
    /// emit warning event
    fn emit_warning(&mut self, ltv: Rate, time_provider: &SafeTimeProvider) {
        self.events.push(Event::LtvWarningBreached {
            facility_id: self.calculator.facility_id,
            ltv_ratio: ltv,
            threshold: self.calculator.thresholds.warning_ltv,
            timestamp: time_provider.now(),
        });
    }
    
    /// issue margin call
    fn issue_margin_call(
        &mut self,
        ltv: Rate,
        cvl: Money,
        collateral_value: Money,
        time_provider: &SafeTimeProvider,
    ) -> Result<()> {
        self.margin_call_active = true;
        self.margin_call_deadline = Some(time_provider.now() + Duration::hours(24));
        
        let required_collateral = self.calculator.calculate_required_collateral(
            cvl,
            collateral_value,
            self.calculator.thresholds.margin_call_ltv,
        );
        
        let required_payment = self.calculator.calculate_required_payment(
            cvl,
            collateral_value,
            self.calculator.thresholds.margin_call_ltv,
        );
        
        self.events.push(Event::MarginCallRequired {
            facility_id: self.calculator.facility_id,
            current_ltv: ltv,
            required_ltv: self.calculator.thresholds.margin_call_ltv,
            deadline: self.margin_call_deadline.unwrap(),
            options: vec![
                format!("Add collateral: {}", required_collateral),
                format!("Make payment: {}", required_payment),
            ],
        });
        
        Ok(())
    }
    
    /// check margin call deadline
    fn check_margin_call_deadline(&mut self, time_provider: &SafeTimeProvider) -> Result<()> {
        if let Some(deadline) = self.margin_call_deadline {
            if time_provider.now() > deadline {
                return Err(FacilityError::MarginCallExpired {
                    deadline,
                    current_time: time_provider.now(),
                });
            }
        }
        Ok(())
    }
    
    /// resolve margin call
    fn resolve_margin_call(&mut self, time_provider: &SafeTimeProvider) {
        self.margin_call_active = false;
        self.margin_call_deadline = None;
        
        self.events.push(Event::MarginCallResolved {
            facility_id: self.calculator.facility_id,
            new_ltv: Rate::from_decimal(dec!(0)), // would need to pass actual LTV
            timestamp: time_provider.now(),
        });
    }
    
    /// trigger liquidation
    fn trigger_liquidation(
        &mut self,
        ltv: Rate,
        cvl: Money,
        collateral_value: Money,
        time_provider: &SafeTimeProvider,
    ) {
        self.events.push(Event::LiquidationTriggered {
            facility_id: self.calculator.facility_id,
            ltv_ratio: ltv,
            collateral_value,
            debt_amount: cvl,
            timestamp: time_provider.now(),
        });
    }
    
    /// get pending events
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }
    
    /// is margin call active
    pub fn is_margin_call_active(&self) -> bool {
        self.margin_call_active
    }
    
    /// get margin call deadline
    pub fn margin_call_deadline(&self) -> Option<DateTime<Utc>> {
        self.margin_call_deadline
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use hourglass_rs::TimeSource;
    
    fn create_test_thresholds() -> LtvThresholds {
        LtvThresholds {
            initial_ltv: Rate::from_percentage(50),
            warning_ltv: Rate::from_percentage(65),
            margin_call_ltv: Rate::from_percentage(70),
            liquidation_ltv: Rate::from_percentage(75),
        }
    }
    
    #[test]
    fn test_ltv_calculation() {
        let calc = LtvCalculator::new(Uuid::new_v4(), create_test_thresholds());
        
        let cvl = Money::from_major(50_000);
        let collateral = Money::from_major(100_000);
        
        let ltv = calc.calculate_ltv(cvl, collateral).unwrap();
        assert_eq!(ltv.as_decimal(), dec!(0.5));
    }
    
    #[test]
    fn test_ltv_status_determination() {
        let calc = LtvCalculator::new(Uuid::new_v4(), create_test_thresholds());
        
        assert_eq!(calc.get_ltv_status(Rate::from_percentage(50)), LtvStatus::Healthy);
        assert_eq!(calc.get_ltv_status(Rate::from_percentage(66)), LtvStatus::Warning);
        assert_eq!(calc.get_ltv_status(Rate::from_percentage(71)), LtvStatus::MarginCall);
        assert_eq!(calc.get_ltv_status(Rate::from_percentage(76)), LtvStatus::Liquidation);
    }
    
    #[test]
    fn test_required_collateral_calculation() {
        let calc = LtvCalculator::new(Uuid::new_v4(), create_test_thresholds());
        
        let cvl = Money::from_major(70_000);
        let current_collateral = Money::from_major(90_000);
        let target_ltv = Rate::from_percentage(70);
        
        let required = calc.calculate_required_collateral(cvl, current_collateral, target_ltv);
        assert_eq!(required, Money::from_major(10_000));
    }
    
    #[test]
    fn test_ltv_monitoring() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        let control = time.test_control().unwrap();
        
        let mut monitor = LtvMonitor::new(
            Uuid::new_v4(),
            create_test_thresholds(),
            Duration::hours(1),
        );
        
        // healthy ltv
        let status = monitor.check_ltv(
            Money::from_major(50_000),
            Money::from_major(100_000),
            &time,
        ).unwrap();
        assert_eq!(status, LtvStatus::Healthy);
        
        // advance time and breach warning
        control.advance(Duration::hours(2));
        let status = monitor.check_ltv(
            Money::from_major(66_000),
            Money::from_major(100_000),
            &time,
        ).unwrap();
        assert_eq!(status, LtvStatus::Warning);
        
        // check events
        let events = monitor.take_events();
        assert_eq!(events.len(), 1);
    }
    
    #[test]
    fn test_margin_call_lifecycle() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        let control = time.test_control().unwrap();
        
        let mut monitor = LtvMonitor::new(
            Uuid::new_v4(),
            create_test_thresholds(),
            Duration::hours(1),
        );
        
        // trigger margin call
        control.advance(Duration::hours(2));
        let status = monitor.check_ltv(
            Money::from_major(71_000),
            Money::from_major(100_000),
            &time,
        ).unwrap();
        assert_eq!(status, LtvStatus::MarginCall);
        assert!(monitor.is_margin_call_active());
        
        // resolve margin call
        control.advance(Duration::hours(2));
        let status = monitor.check_ltv(
            Money::from_major(50_000),
            Money::from_major(100_000),
            &time,
        ).unwrap();
        assert_eq!(status, LtvStatus::Healthy);
        assert!(!monitor.is_margin_call_active());
        
        let events = monitor.take_events();
        assert!(events.iter().any(|e| matches!(e, Event::MarginCallRequired { .. })));
        assert!(events.iter().any(|e| matches!(e, Event::MarginCallResolved { .. })));
    }
}