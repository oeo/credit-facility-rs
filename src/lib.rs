pub mod collateral;
pub mod config;
pub mod decimal;
pub mod errors;
pub mod events;
pub mod facilities;
pub mod facility;
pub mod interest;
pub mod payments;
pub mod state;
pub mod types;

// re-export key types
pub use decimal::{Money, Rate};
pub use errors::{FacilityError, Result};
pub use events::{Event, EventStore};
pub use interest::{
    AccrualEngine, CompoundingEngine, DayCountConvention, InterestCalculation, PenaltyConfig,
    PenaltyEngine,
};
pub use collateral::{
    BitcoinCollateral, LiquidationEngine, LiquidationMethod, LiquidationResult,
    LtvCalculator, LtvMonitor, PriceFeed,
};
pub use types::{
    AmortizationMethod, CollateralPosition, DeficiencyBalance,
    FacilityId, FacilityStatus, LtvStatus, LtvThresholds, OpenTermType, OverpaymentStrategy,
    PaymentApplication, PaymentSchedule, RecoveryStatus, RevolvingType, TermLoanType,
};

// re-export external dependencies that users will need
pub use chrono;
pub use hourglass_rs::{SafeTimeProvider, TimeSource};
pub use rust_decimal::Decimal;
pub use uuid::Uuid;
