use chrono::{DateTime, Utc};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::decimal::{Money, Rate};
use crate::errors::Result;
use crate::interest::{InterestCalculation, InterestCalculator};

/// penalty configuration
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct PenaltyConfig {
    /// base interest rate
    pub base_rate: Rate,
    /// penalty multiplier (e.g., 1.5x for 50% higher)
    pub penalty_multiplier: Decimal,
    /// fixed penalty rate (overrides multiplier if set)
    pub fixed_penalty_rate: Option<Rate>,
    /// grace period before penalties apply
    pub grace_period_days: u32,
    /// minimum penalty amount
    pub minimum_penalty: Money,
    /// maximum penalty rate cap
    pub maximum_penalty_rate: Option<Rate>,
}

impl PenaltyConfig {
    pub fn new(base_rate: Rate, multiplier: Decimal) -> Self {
        Self {
            base_rate,
            penalty_multiplier: multiplier,
            fixed_penalty_rate: None,
            grace_period_days: 0,
            minimum_penalty: Money::ZERO,
            maximum_penalty_rate: None,
        }
    }
    
    /// get the effective penalty rate
    pub fn effective_penalty_rate(&self) -> Rate {
        let penalty_rate = if let Some(fixed_rate) = self.fixed_penalty_rate {
            fixed_rate
        } else {
            Rate::from_decimal(self.base_rate.as_decimal() * self.penalty_multiplier)
        };
        
        if let Some(max_rate) = self.maximum_penalty_rate {
            Rate::from_decimal(penalty_rate.as_decimal().min(max_rate.as_decimal()))
        } else {
            penalty_rate
        }
    }
}

/// engine for calculating penalty interest
pub struct PenaltyEngine {
    pub config: PenaltyConfig,
}

impl PenaltyEngine {
    pub fn new(config: PenaltyConfig) -> Self {
        Self { config }
    }
    
    /// calculate penalty interest on overdue amount
    pub fn calculate_penalty(
        &self,
        overdue_amount: Money,
        days_overdue: u32,
    ) -> PenaltyCalculation {
        if days_overdue <= self.config.grace_period_days {
            return PenaltyCalculation {
                penalty_amount: Money::ZERO,
                effective_rate: Rate::ZERO,
                days_charged: 0,
                overdue_base: overdue_amount,
                grace_applied: true,
            };
        }
        
        let days_charged = days_overdue - self.config.grace_period_days;
        let penalty_rate = self.config.effective_penalty_rate();
        let daily_penalty_rate = penalty_rate.as_decimal() / dec!(365);
        
        let penalty = overdue_amount.as_decimal() * daily_penalty_rate * Decimal::from(days_charged);
        let penalty_amount = Money::from_decimal(penalty).max(self.config.minimum_penalty);
        
        PenaltyCalculation {
            penalty_amount,
            effective_rate: penalty_rate,
            days_charged,
            overdue_base: overdue_amount,
            grace_applied: false,
        }
    }
    
    /// calculate default interest (entire balance at penalty rate)
    pub fn calculate_default_interest(
        &self,
        outstanding_balance: Money,
        days_in_default: u32,
    ) -> DefaultInterest {
        let penalty_rate = self.config.effective_penalty_rate();
        let daily_rate = penalty_rate.as_decimal() / dec!(365);
        
        let interest = outstanding_balance.as_decimal() * daily_rate * Decimal::from(days_in_default);
        
        DefaultInterest {
            interest_amount: Money::from_decimal(interest),
            penalty_rate,
            days_in_default,
            balance_base: outstanding_balance,
        }
    }
    
    /// calculate tiered penalty based on days overdue
    pub fn calculate_tiered_penalty(
        &self,
        overdue_amount: Money,
        days_overdue: u32,
        tiers: &[PenaltyTier],
    ) -> TieredPenaltyResult {
        let mut total_penalty = Money::ZERO;
        let mut tier_results = Vec::new();
        
        for tier in tiers {
            if days_overdue >= tier.min_days {
                let days_in_tier = if let Some(max) = tier.max_days {
                    days_overdue.min(max) - tier.min_days + 1
                } else {
                    days_overdue - tier.min_days + 1
                };
                
                let tier_rate = Rate::from_decimal(
                    self.config.base_rate.as_decimal() * tier.rate_multiplier
                );
                let daily_rate = tier_rate.as_decimal() / dec!(365);
                let tier_penalty = overdue_amount.as_decimal() * daily_rate * Decimal::from(days_in_tier);
                
                let penalty_amount = Money::from_decimal(tier_penalty);
                total_penalty += penalty_amount;
                
                tier_results.push(TierCalculation {
                    tier_name: tier.name.clone(),
                    days_in_tier,
                    rate: tier_rate,
                    penalty: penalty_amount,
                });
            }
        }
        
        TieredPenaltyResult {
            total_penalty: total_penalty.max(self.config.minimum_penalty),
            tier_calculations: tier_results,
            days_overdue,
            overdue_amount,
        }
    }
}

impl InterestCalculator for PenaltyEngine {
    fn calculate_interest(
        &self,
        principal: Money,
        _rate: Rate,
        start_date: DateTime<Utc>,
        end_date: DateTime<Utc>,
    ) -> Result<InterestCalculation> {
        let days = (end_date - start_date).num_days() as u32;
        let penalty_rate = self.config.effective_penalty_rate();
        let daily_rate = self.get_daily_rate(penalty_rate);
        
        let interest = principal.as_decimal() * daily_rate.as_decimal() * Decimal::from(days);
        
        Ok(InterestCalculation {
            interest_amount: Money::from_decimal(interest),
            daily_rate,
            days,
            principal_base: principal,
            calculation_method: "Penalty interest".to_string(),
        })
    }
    
    fn get_daily_rate(&self, annual_rate: Rate) -> Rate {
        Rate::from_decimal(annual_rate.as_decimal() / dec!(365))
    }
}

/// penalty calculation result
#[derive(Debug, Clone, PartialEq)]
pub struct PenaltyCalculation {
    pub penalty_amount: Money,
    pub effective_rate: Rate,
    pub days_charged: u32,
    pub overdue_base: Money,
    pub grace_applied: bool,
}

/// default interest calculation
#[derive(Debug, Clone, PartialEq)]
pub struct DefaultInterest {
    pub interest_amount: Money,
    pub penalty_rate: Rate,
    pub days_in_default: u32,
    pub balance_base: Money,
}

/// penalty tier for progressive penalties
#[derive(Debug, Clone)]
pub struct PenaltyTier {
    pub name: String,
    pub min_days: u32,
    pub max_days: Option<u32>,
    pub rate_multiplier: Decimal,
}

/// tiered penalty calculation result
#[derive(Debug, Clone, PartialEq)]
pub struct TieredPenaltyResult {
    pub total_penalty: Money,
    pub tier_calculations: Vec<TierCalculation>,
    pub days_overdue: u32,
    pub overdue_amount: Money,
}

/// individual tier calculation
#[derive(Debug, Clone, PartialEq)]
pub struct TierCalculation {
    pub tier_name: String,
    pub days_in_tier: u32,
    pub rate: Rate,
    pub penalty: Money,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_basic_penalty() {
        let config = PenaltyConfig::new(Rate::from_percentage(5), dec!(1.5));
        let engine = PenaltyEngine::new(config);
        
        let overdue = Money::from_major(1_000);
        let result = engine.calculate_penalty(overdue, 30);
        
        assert_eq!(result.effective_rate.as_percentage().round_dp(1), dec!(7.5));
        assert_eq!(result.days_charged, 30);
        
        let expected_penalty = Money::from_str_exact("6.16").unwrap();
        assert_eq!(result.penalty_amount.round_dp(2), expected_penalty);
    }
    
    #[test]
    fn test_grace_period() {
        let mut config = PenaltyConfig::new(Rate::from_percentage(5), dec!(1.5));
        config.grace_period_days = 5;
        let engine = PenaltyEngine::new(config);
        
        let overdue = Money::from_major(1_000);
        
        let result_in_grace = engine.calculate_penalty(overdue, 3);
        assert_eq!(result_in_grace.penalty_amount, Money::ZERO);
        assert!(result_in_grace.grace_applied);
        
        let result_after_grace = engine.calculate_penalty(overdue, 10);
        assert_eq!(result_after_grace.days_charged, 5);
        assert!(!result_after_grace.grace_applied);
    }
    
    #[test]
    fn test_minimum_penalty() {
        let mut config = PenaltyConfig::new(Rate::from_percentage(5), dec!(1.5));
        config.minimum_penalty = Money::from_major(25);
        let engine = PenaltyEngine::new(config);
        
        let overdue = Money::from_major(100);
        let result = engine.calculate_penalty(overdue, 5);
        
        assert_eq!(result.penalty_amount, Money::from_major(25));
    }
    
    #[test]
    fn test_fixed_penalty_rate() {
        let mut config = PenaltyConfig::new(Rate::from_percentage(5), dec!(1.5));
        config.fixed_penalty_rate = Some(Rate::from_percentage(10));
        
        assert_eq!(config.effective_penalty_rate(), Rate::from_percentage(10));
    }
    
    #[test]
    fn test_maximum_penalty_cap() {
        let mut config = PenaltyConfig::new(Rate::from_percentage(20), dec!(2.0));
        config.maximum_penalty_rate = Some(Rate::from_percentage(30));
        
        assert_eq!(config.effective_penalty_rate(), Rate::from_percentage(30));
    }
    
    #[test]
    fn test_default_interest() {
        let config = PenaltyConfig::new(Rate::from_percentage(5), dec!(2.0));
        let engine = PenaltyEngine::new(config);
        
        let balance = Money::from_major(10_000);
        let result = engine.calculate_default_interest(balance, 30);
        
        assert_eq!(result.penalty_rate, Rate::from_percentage(10));
        
        let expected = Money::from_str_exact("82.19").unwrap();
        assert_eq!(result.interest_amount.round_dp(2), expected);
    }
    
    #[test]
    fn test_tiered_penalties() {
        let config = PenaltyConfig::new(Rate::from_percentage(5), dec!(1.0));
        let engine = PenaltyEngine::new(config);
        
        let tiers = vec![
            PenaltyTier {
                name: "Tier 1".to_string(),
                min_days: 1,
                max_days: Some(30),
                rate_multiplier: dec!(1.5),
            },
            PenaltyTier {
                name: "Tier 2".to_string(),
                min_days: 31,
                max_days: Some(60),
                rate_multiplier: dec!(2.0),
            },
            PenaltyTier {
                name: "Tier 3".to_string(),
                min_days: 61,
                max_days: None,
                rate_multiplier: dec!(3.0),
            },
        ];
        
        let overdue = Money::from_major(1_000);
        let result = engine.calculate_tiered_penalty(overdue, 45, &tiers);
        
        assert_eq!(result.tier_calculations.len(), 2);
        assert_eq!(result.tier_calculations[0].tier_name, "Tier 1");
        assert_eq!(result.tier_calculations[0].days_in_tier, 30);
        assert_eq!(result.tier_calculations[1].tier_name, "Tier 2");
        assert_eq!(result.tier_calculations[1].days_in_tier, 15);
        
        assert!(result.total_penalty > Money::ZERO);
    }
}