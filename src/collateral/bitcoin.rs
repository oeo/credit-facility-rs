use chrono::{DateTime, Duration, Utc};
use hourglass_rs::SafeTimeProvider;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::VecDeque;

use crate::decimal::{Money, Rate};
use crate::errors::{FacilityError, Result};

/// price feed for bitcoin
pub trait PriceFeed {
    /// get current spot price
    fn get_spot_price(&self) -> Money;
    
    /// get time-weighted average price
    fn get_twap(&self, duration: Duration) -> Money;
    
    /// get 30-day volatility
    fn get_30d_volatility(&self) -> Decimal;
    
    /// get 24h high/low
    fn get_24h_range(&self) -> (Money, Money);
}

/// mock price feed for testing
pub struct MockPriceFeed {
    spot_price: Money,
    volatility: Decimal,
    price_history: VecDeque<(DateTime<Utc>, Money)>,
}

impl MockPriceFeed {
    pub fn new(spot_price: Money) -> Self {
        Self {
            spot_price,
            volatility: dec!(0.05), // 5% default volatility
            price_history: VecDeque::new(),
        }
    }
    
    pub fn set_price(&mut self, price: Money, timestamp: DateTime<Utc>) {
        self.spot_price = price;
        self.price_history.push_back((timestamp, price));
        
        // keep only last 100 prices
        while self.price_history.len() > 100 {
            self.price_history.pop_front();
        }
    }
    
    pub fn set_volatility(&mut self, volatility: Decimal) {
        self.volatility = volatility;
    }
}

impl PriceFeed for MockPriceFeed {
    fn get_spot_price(&self) -> Money {
        self.spot_price
    }
    
    fn get_twap(&self, duration: Duration) -> Money {
        if self.price_history.is_empty() {
            return self.spot_price;
        }
        
        let cutoff = self.price_history.back()
            .map(|(t, _)| *t - duration)
            .unwrap_or(Utc::now() - duration);
        
        let relevant_prices: Vec<Money> = self.price_history.iter()
            .filter(|(t, _)| *t >= cutoff)
            .map(|(_, p)| *p)
            .collect();
        
        if relevant_prices.is_empty() {
            return self.spot_price;
        }
        
        let sum: Decimal = relevant_prices.iter()
            .map(|p| p.as_decimal())
            .sum();
        
        Money::from_decimal(sum / Decimal::from(relevant_prices.len()))
    }
    
    fn get_30d_volatility(&self) -> Decimal {
        self.volatility
    }
    
    fn get_24h_range(&self) -> (Money, Money) {
        let cutoff = self.price_history.back()
            .map(|(t, _)| *t - Duration::hours(24))
            .unwrap_or(Utc::now() - Duration::hours(24));
        
        let relevant_prices: Vec<Money> = self.price_history.iter()
            .filter(|(t, _)| *t >= cutoff)
            .map(|(_, p)| *p)
            .collect();
        
        if relevant_prices.is_empty() {
            return (self.spot_price, self.spot_price);
        }
        
        let min = relevant_prices.iter().min().copied().unwrap_or(self.spot_price);
        let max = relevant_prices.iter().max().copied().unwrap_or(self.spot_price);
        
        (min, max)
    }
}

/// bitcoin collateral management
pub struct BitcoinCollateral {
    btc_amount: Decimal,
    price_feed: Box<dyn PriceFeed>,
    volatility_buffer: Decimal,
    last_valuation: DateTime<Utc>,
    flash_crash_protection: bool,
    twap_window: Duration,
}

impl BitcoinCollateral {
    /// create new bitcoin collateral
    pub fn new(
        btc_amount: Decimal,
        price_feed: Box<dyn PriceFeed>,
    ) -> Self {
        Self {
            btc_amount,
            price_feed,
            volatility_buffer: dec!(0.1), // 10% default buffer
            last_valuation: Utc::now(),
            flash_crash_protection: true,
            twap_window: Duration::minutes(15),
        }
    }
    
    /// set volatility buffer
    pub fn set_volatility_buffer(&mut self, buffer: Decimal) {
        self.volatility_buffer = buffer;
    }
    
    /// enable/disable flash crash protection
    pub fn set_flash_crash_protection(&mut self, enabled: bool) {
        self.flash_crash_protection = enabled;
    }
    
    /// set twap window for flash crash protection
    pub fn set_twap_window(&mut self, window: Duration) {
        self.twap_window = window;
    }
    
    /// get btc amount
    pub fn btc_amount(&self) -> Decimal {
        self.btc_amount
    }
    
    /// add btc collateral
    pub fn add_collateral(&mut self, btc: Decimal) {
        self.btc_amount += btc;
    }
    
    /// remove btc collateral (for partial liquidation)
    pub fn remove_collateral(&mut self, btc: Decimal) -> Result<()> {
        if btc > self.btc_amount {
            return Err(FacilityError::InsufficientCollateral {
                available: self.btc_amount,
                required: btc,
            });
        }
        self.btc_amount -= btc;
        Ok(())
    }
    
    /// calculate current value
    pub fn calculate_value(&self) -> Money {
        let spot_price = self.price_feed.get_spot_price();
        Money::from_decimal(self.btc_amount * spot_price.as_decimal())
    }
    
    /// calculate risk-adjusted value
    pub fn calculate_risk_adjusted_value(&self) -> Money {
        let spot_price = self.price_feed.get_spot_price();
        let volatility = self.price_feed.get_30d_volatility();
        
        // apply larger haircut for higher volatility
        let haircut = self.volatility_buffer + volatility * dec!(0.5);
        let haircut = haircut.min(dec!(0.5)); // cap at 50% haircut
        
        let adjusted_value = self.btc_amount * spot_price.as_decimal() * (dec!(1) - haircut);
        Money::from_decimal(adjusted_value)
    }
    
    /// check if liquidation should trigger with flash crash protection
    pub fn should_liquidate(
        &mut self,
        cvl: Money,
        liquidation_threshold: Rate,
        time_provider: &SafeTimeProvider,
    ) -> bool {
        self.last_valuation = time_provider.now();
        
        if !self.flash_crash_protection {
            // simple spot price check
            let spot_value = self.calculate_value();
            let ltv = cvl.as_decimal() / spot_value.as_decimal();
            return ltv > liquidation_threshold.as_decimal();
        }
        
        // use both spot and twap for flash crash protection
        let spot_price = self.price_feed.get_spot_price();
        let twap = self.price_feed.get_twap(self.twap_window);
        
        let spot_value = Money::from_decimal(self.btc_amount * spot_price.as_decimal());
        let twap_value = Money::from_decimal(self.btc_amount * twap.as_decimal());
        
        let ltv_spot = cvl.as_decimal() / spot_value.as_decimal();
        let ltv_twap = cvl.as_decimal() / twap_value.as_decimal();
        
        // only liquidate if both spot and twap breach
        ltv_spot > liquidation_threshold.as_decimal() && 
        ltv_twap > liquidation_threshold.as_decimal()
    }
    
    /// calculate amount to liquidate for partial liquidation
    pub fn calculate_liquidation_amount(
        &self,
        cvl: Money,
        target_ltv: Rate,
    ) -> Result<Decimal> {
        let spot_price = self.price_feed.get_spot_price();
        
        // calculate required collateral value
        let required_value = Money::from_decimal(
            cvl.as_decimal() / target_ltv.as_decimal()
        );
        
        // calculate current value
        let current_value = self.calculate_value();
        
        if current_value <= required_value {
            // need to liquidate everything
            return Ok(self.btc_amount);
        }
        
        // calculate excess value to liquidate
        let excess_value = current_value - required_value;
        let btc_to_liquidate = excess_value.as_decimal() / spot_price.as_decimal();
        
        Ok(btc_to_liquidate.min(self.btc_amount))
    }
    
    /// get collateral health metrics
    pub fn get_health_metrics(&self) -> CollateralHealth {
        let spot_price = self.price_feed.get_spot_price();
        let (low_24h, high_24h) = self.price_feed.get_24h_range();
        let volatility = self.price_feed.get_30d_volatility();
        
        let price_change_24h = if low_24h > Money::ZERO {
            (spot_price.as_decimal() - low_24h.as_decimal()) / low_24h.as_decimal()
        } else {
            dec!(0)
        };
        
        CollateralHealth {
            btc_amount: self.btc_amount,
            spot_price,
            value: self.calculate_value(),
            risk_adjusted_value: self.calculate_risk_adjusted_value(),
            volatility_30d: volatility,
            price_change_24h,
            low_24h,
            high_24h,
        }
    }
}

/// collateral health metrics
#[derive(Debug, Clone)]
pub struct CollateralHealth {
    pub btc_amount: Decimal,
    pub spot_price: Money,
    pub value: Money,
    pub risk_adjusted_value: Money,
    pub volatility_30d: Decimal,
    pub price_change_24h: Decimal,
    pub low_24h: Money,
    pub high_24h: Money,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use hourglass_rs::TimeSource;
    
    #[test]
    fn test_bitcoin_valuation() {
        let price_feed = Box::new(MockPriceFeed::new(Money::from_major(50_000)));
        let collateral = BitcoinCollateral::new(dec!(1.5), price_feed);
        
        let value = collateral.calculate_value();
        assert_eq!(value, Money::from_major(75_000));
    }
    
    #[test]
    fn test_risk_adjusted_value() {
        let mut price_feed = MockPriceFeed::new(Money::from_major(50_000));
        price_feed.set_volatility(dec!(0.1)); // 10% volatility
        
        let collateral = BitcoinCollateral::new(dec!(1.0), Box::new(price_feed));
        
        let risk_adjusted = collateral.calculate_risk_adjusted_value();
        assert!(risk_adjusted < Money::from_major(50_000));
        assert!(risk_adjusted > Money::from_major(35_000)); // with max 50% haircut
    }
    
    #[test]
    fn test_flash_crash_protection() {
        let time = SafeTimeProvider::new(TimeSource::Test(
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
        ));
        
        let mut price_feed = MockPriceFeed::new(Money::from_major(50_000));
        
        // add more price history to ensure TWAP stays high
        // 15 minute window, so add 20 prices over 20 minutes
        for i in (0..20).rev() {
            price_feed.set_price(
                Money::from_major(50_000),
                time.now() - Duration::minutes(i as i64),
            );
        }
        
        // sudden price drop (flash crash) - just the latest price
        price_feed.set_price(Money::from_major(30_000), time.now());
        
        let mut collateral = BitcoinCollateral::new(dec!(1.0), Box::new(price_feed));
        
        let cvl = Money::from_major(35_000); // lower CVL to ensure TWAP-based LTV < 75%
        let liquidation_threshold = Rate::from_percentage(75);
        
        // with flash crash protection, should not liquidate
        // because twap is still close to 50k (only 1 out of ~15 prices is 30k)
        let should_liquidate = collateral.should_liquidate(
            cvl,
            liquidation_threshold,
            &time,
        );
        
        assert!(!should_liquidate);
    }
    
    #[test]
    fn test_partial_liquidation_calculation() {
        let price_feed = Box::new(MockPriceFeed::new(Money::from_major(50_000)));
        let collateral = BitcoinCollateral::new(dec!(2.0), price_feed);
        
        let cvl = Money::from_major(70_000);
        let target_ltv = Rate::from_percentage(65);
        
        let btc_to_liquidate = collateral.calculate_liquidation_amount(cvl, target_ltv).unwrap();
        
        // need collateral value of 70k/0.65 = 107.7k
        // current value is 100k (2 BTC * 50k)
        // so need to keep 107.7k/50k = 2.154 BTC worth
        // but we only have 2 BTC, so liquidate all
        assert_eq!(btc_to_liquidate, dec!(2.0));
    }
    
    #[test]
    fn test_collateral_addition() {
        let price_feed = Box::new(MockPriceFeed::new(Money::from_major(50_000)));
        let mut collateral = BitcoinCollateral::new(dec!(1.0), price_feed);
        
        collateral.add_collateral(dec!(0.5));
        assert_eq!(collateral.btc_amount(), dec!(1.5));
        
        let value = collateral.calculate_value();
        assert_eq!(value, Money::from_major(75_000));
    }
}