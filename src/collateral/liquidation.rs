use chrono::{DateTime, Duration, Utc};
use hourglass_rs::SafeTimeProvider;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use uuid::Uuid;

use crate::decimal::{Money, Rate};
use crate::errors::{FacilityError, Result};
use crate::events::Event;
use crate::types::{DeficiencyBalance, PaymentApplication, RecoveryStatus};

/// liquidation method
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiquidationMethod {
    MarketSale,
    Auction,
    PrivateSale,
    Partial,
}

/// liquidation result
#[derive(Debug, Clone)]
pub struct LiquidationResult {
    pub method: LiquidationMethod,
    pub gross_proceeds: Money,
    pub liquidation_costs: Money,
    pub net_proceeds: Money,
    pub cvl_before: Money,
    pub cvl_after: Money,
    pub deficiency: Option<Money>,
    pub surplus: Option<Money>,
    pub timestamp: DateTime<Utc>,
}

/// liquidation engine
pub struct LiquidationEngine {
    facility_id: Uuid,
    status: LiquidationStatus,
    grace_period_hours: u32,
    grace_period_end: Option<DateTime<Utc>>,
    liquidation_cost_rate: Decimal,
    events: Vec<Event>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LiquidationStatus {
    NotInitiated,
    GracePeriod,
    InProgress,
    Completed,
}

impl LiquidationEngine {
    /// create new liquidation engine
    pub fn new(facility_id: Uuid) -> Self {
        Self {
            facility_id,
            status: LiquidationStatus::NotInitiated,
            grace_period_hours: 24,
            grace_period_end: None,
            liquidation_cost_rate: dec!(0.05), // 5% default cost
            events: Vec::new(),
        }
    }
    
    /// set grace period
    pub fn set_grace_period(&mut self, hours: u32) {
        self.grace_period_hours = hours;
    }
    
    /// set liquidation cost rate
    pub fn set_liquidation_cost_rate(&mut self, rate: Decimal) {
        self.liquidation_cost_rate = rate;
    }
    
    /// trigger liquidation
    pub fn trigger_liquidation(
        &mut self,
        cvl: Money,
        collateral_value: Money,
        time_provider: &SafeTimeProvider,
    ) -> Result<()> {
        if self.status != LiquidationStatus::NotInitiated {
            return Err(FacilityError::InvalidState {
                current: format!("{:?}", self.status),
                expected: "NotInitiated".to_string(),
            });
        }
        
        let now = time_provider.now();
        
        if self.grace_period_hours > 0 {
            self.status = LiquidationStatus::GracePeriod;
            self.grace_period_end = Some(now + Duration::hours(self.grace_period_hours as i64));
            
            self.events.push(Event::LiquidationPending {
                facility_id: self.facility_id,
                notice_period_ends: self.grace_period_end.unwrap(),
                timestamp: now,
            });
        } else {
            self.status = LiquidationStatus::InProgress;
            
            self.events.push(Event::CollateralSaleInitiated {
                facility_id: self.facility_id,
                method: "Immediate".to_string(),
                expected_proceeds: collateral_value,
                timestamp: now,
            });
        }
        
        self.events.push(Event::LiquidationTriggered {
            facility_id: self.facility_id,
            ltv_ratio: Rate::from_decimal(cvl.as_decimal() / collateral_value.as_decimal()),
            collateral_value,
            debt_amount: cvl,
            timestamp: now,
        });
        
        Ok(())
    }
    
    /// check if grace period expired
    pub fn check_grace_period(&mut self, time_provider: &SafeTimeProvider) -> Result<bool> {
        if self.status != LiquidationStatus::GracePeriod {
            return Ok(false);
        }
        
        if let Some(end) = self.grace_period_end {
            if time_provider.now() > end {
                self.status = LiquidationStatus::InProgress;
                
                self.events.push(Event::CollateralSaleInitiated {
                    facility_id: self.facility_id,
                    method: "PostGracePeriod".to_string(),
                    expected_proceeds: Money::ZERO, // would need to pass actual value
                    timestamp: time_provider.now(),
                });
                
                return Ok(true);
            }
        }
        
        Ok(false)
    }
    
    /// execute market liquidation
    pub fn execute_market_liquidation(
        &mut self,
        collateral_value: Money,
        cvl: Money,
        time_provider: &SafeTimeProvider,
    ) -> Result<LiquidationResult> {
        self.check_can_liquidate()?;
        
        // estimate proceeds with market discount
        let market_discount = dec!(0.05); // 5% market discount
        let gross_proceeds = Money::from_decimal(
            collateral_value.as_decimal() * (dec!(1) - market_discount)
        );
        
        self.complete_liquidation(
            LiquidationMethod::MarketSale,
            gross_proceeds,
            cvl,
            time_provider,
        )
    }
    
    /// execute auction liquidation
    pub fn execute_auction_liquidation(
        &mut self,
        winning_bid: Money,
        cvl: Money,
        time_provider: &SafeTimeProvider,
    ) -> Result<LiquidationResult> {
        self.check_can_liquidate()?;
        
        self.complete_liquidation(
            LiquidationMethod::Auction,
            winning_bid,
            cvl,
            time_provider,
        )
    }
    
    /// execute partial liquidation
    pub fn execute_partial_liquidation(
        &mut self,
        proceeds: Money,
        cvl: Money,
        time_provider: &SafeTimeProvider,
    ) -> Result<LiquidationResult> {
        self.check_can_liquidate()?;
        
        self.complete_liquidation(
            LiquidationMethod::Partial,
            proceeds,
            cvl,
            time_provider,
        )
    }
    
    /// complete liquidation
    fn complete_liquidation(
        &mut self,
        method: LiquidationMethod,
        gross_proceeds: Money,
        cvl: Money,
        time_provider: &SafeTimeProvider,
    ) -> Result<LiquidationResult> {
        // calculate costs
        let liquidation_costs = Money::from_decimal(
            gross_proceeds.as_decimal() * self.liquidation_cost_rate
        );
        let net_proceeds = gross_proceeds - liquidation_costs;
        
        // apply proceeds to debt
        let cvl_after = if net_proceeds >= cvl {
            Money::ZERO
        } else {
            cvl - net_proceeds
        };
        
        let deficiency = if cvl_after > Money::ZERO {
            Some(cvl_after)
        } else {
            None
        };
        
        let surplus = if net_proceeds > cvl {
            Some(net_proceeds - cvl)
        } else {
            None
        };
        
        self.status = LiquidationStatus::Completed;
        
        let result = LiquidationResult {
            method,
            gross_proceeds,
            liquidation_costs,
            net_proceeds,
            cvl_before: cvl,
            cvl_after,
            deficiency,
            surplus,
            timestamp: time_provider.now(),
        };
        
        self.events.push(Event::LiquidationCompleted {
            facility_id: self.facility_id,
            proceeds: net_proceeds,
            remaining_debt: cvl_after,
            timestamp: time_provider.now(),
        });
        
        if let Some(def) = deficiency {
            self.events.push(Event::DeficiencyBalance {
                facility_id: self.facility_id,
                amount: def,
                recovery_action: "Pursuing".to_string(),
                timestamp: time_provider.now(),
            });
        }
        
        Ok(result)
    }
    
    /// check if liquidation can proceed
    fn check_can_liquidate(&self) -> Result<()> {
        match self.status {
            LiquidationStatus::InProgress => Ok(()),
            _ => Err(FacilityError::InvalidState {
                current: format!("{:?}", self.status),
                expected: "InProgress".to_string(),
            }),
        }
    }
    
    /// apply liquidation proceeds using waterfall
    pub fn apply_proceeds_waterfall(
        &self,
        proceeds: Money,
        liquidation_costs: Money,
        penalties: Money,
        fees: Money,
        interest: Money,
        principal: Money,
    ) -> PaymentApplication {
        let mut remaining = proceeds;
        let mut application = PaymentApplication::default();
        
        // first: liquidation costs (not tracked in application)
        remaining = if remaining >= liquidation_costs {
            remaining - liquidation_costs
        } else {
            Money::ZERO
        };
        
        // then: penalties
        if remaining > Money::ZERO && penalties > Money::ZERO {
            let to_penalties = remaining.min(penalties);
            application.to_penalties = to_penalties;
            remaining -= to_penalties;
        }
        
        // then: fees
        if remaining > Money::ZERO && fees > Money::ZERO {
            let to_fees = remaining.min(fees);
            application.to_fees = to_fees;
            remaining -= to_fees;
        }
        
        // then: interest
        if remaining > Money::ZERO && interest > Money::ZERO {
            let to_interest = remaining.min(interest);
            application.to_interest = to_interest;
            remaining -= to_interest;
        }
        
        // finally: principal
        if remaining > Money::ZERO && principal > Money::ZERO {
            let to_principal = remaining.min(principal);
            application.to_principal = to_principal;
            remaining -= to_principal;
        }
        
        application.excess = remaining;
        application
    }
    
    /// create deficiency balance
    pub fn create_deficiency_balance(
        &self,
        remaining_debt: Money,
        collateral_proceeds: Money,
        liquidation_date: DateTime<Utc>,
    ) -> DeficiencyBalance {
        DeficiencyBalance {
            original_loan_id: self.facility_id,
            liquidation_date,
            collateral_proceeds,
            remaining_debt,
            recovery_status: RecoveryStatus::Pursuing,
        }
    }
    
    /// get pending events
    pub fn take_events(&mut self) -> Vec<Event> {
        std::mem::take(&mut self.events)
    }
    
    /// is liquidation in progress
    pub fn is_in_progress(&self) -> bool {
        self.status == LiquidationStatus::InProgress
    }
    
    /// is liquidation completed
    pub fn is_completed(&self) -> bool {
        self.status == LiquidationStatus::Completed
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use hourglass_rs::TimeSource;
    
    #[test]
    fn test_liquidation_trigger() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        
        let mut engine = LiquidationEngine::new(Uuid::new_v4());
        
        engine.trigger_liquidation(
            Money::from_major(100_000),
            Money::from_major(80_000),
            &time,
        ).unwrap();
        
        assert_eq!(engine.status, LiquidationStatus::GracePeriod);
        assert!(engine.grace_period_end.is_some());
        
        let events = engine.take_events();
        assert!(events.iter().any(|e| matches!(e, Event::LiquidationTriggered { .. })));
        assert!(events.iter().any(|e| matches!(e, Event::LiquidationPending { .. })));
    }
    
    #[test]
    fn test_grace_period_expiry() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        let control = time.test_control().unwrap();
        
        let mut engine = LiquidationEngine::new(Uuid::new_v4());
        engine.set_grace_period(24);
        
        engine.trigger_liquidation(
            Money::from_major(100_000),
            Money::from_major(80_000),
            &time,
        ).unwrap();
        
        // advance past grace period
        control.advance(Duration::hours(25));
        
        let expired = engine.check_grace_period(&time).unwrap();
        assert!(expired);
        assert_eq!(engine.status, LiquidationStatus::InProgress);
    }
    
    #[test]
    fn test_market_liquidation() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        
        let mut engine = LiquidationEngine::new(Uuid::new_v4());
        engine.set_grace_period(0); // no grace period
        
        engine.trigger_liquidation(
            Money::from_major(100_000),
            Money::from_major(80_000),
            &time,
        ).unwrap();
        
        let result = engine.execute_market_liquidation(
            Money::from_major(80_000),
            Money::from_major(100_000),
            &time,
        ).unwrap();
        
        assert_eq!(result.method, LiquidationMethod::MarketSale);
        assert_eq!(result.gross_proceeds, Money::from_major(76_000)); // 5% discount
        assert!(result.deficiency.is_some());
    }
    
    #[test]
    fn test_liquidation_with_surplus() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        
        let mut engine = LiquidationEngine::new(Uuid::new_v4());
        engine.set_grace_period(0);
        
        engine.trigger_liquidation(
            Money::from_major(50_000),
            Money::from_major(100_000),
            &time,
        ).unwrap();
        
        let result = engine.execute_market_liquidation(
            Money::from_major(100_000),
            Money::from_major(50_000),
            &time,
        ).unwrap();
        
        assert!(result.surplus.is_some());
        assert_eq!(result.surplus.unwrap(), Money::from_major(40_250)); // 95k - 4.75k costs - 50k debt
        assert_eq!(result.cvl_after, Money::ZERO);
    }
    
    #[test]
    fn test_proceeds_waterfall() {
        let engine = LiquidationEngine::new(Uuid::new_v4());
        
        let application = engine.apply_proceeds_waterfall(
            Money::from_major(50_000),
            Money::from_major(2_000),
            Money::from_major(500),
            Money::from_major(300),
            Money::from_major(1_200),
            Money::from_major(50_000),
        );
        
        // after costs: 48k
        assert_eq!(application.to_penalties, Money::from_major(500));
        assert_eq!(application.to_fees, Money::from_major(300));
        assert_eq!(application.to_interest, Money::from_major(1_200));
        assert_eq!(application.to_principal, Money::from_major(46_000));
        assert_eq!(application.excess, Money::ZERO);
    }
    
    #[test]
    fn test_partial_liquidation() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        
        let mut engine = LiquidationEngine::new(Uuid::new_v4());
        engine.set_grace_period(0);
        
        engine.trigger_liquidation(
            Money::from_major(100_000),
            Money::from_major(120_000),
            &time,
        ).unwrap();
        
        let result = engine.execute_partial_liquidation(
            Money::from_major(40_000),
            Money::from_major(100_000),
            &time,
        ).unwrap();
        
        assert_eq!(result.method, LiquidationMethod::Partial);
        assert_eq!(result.net_proceeds, Money::from_major(38_000)); // 40k - 2k costs
        assert_eq!(result.cvl_after, Money::from_major(62_000)); // 100k - 38k
    }
}