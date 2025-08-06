# Financial terms glossary

This library provides comprehensive credit facility management. Below are key financial concepts and corresponding API usage.

## Core concepts

### Facility types
Different loan products supported by the system:

```rust
use credit_facility_rs::{Facility, FacilityConfig};
use hourglass_rs::SafeTimeProvider;

// term loan with fixed payments
let term_loan = Facility::originate(
    FacilityConfig::term_loan(
        principal: Money::from(100_000), // $100,000
        rate: Rate::from_decimal(0.05),  // 5% APR
        term_months: 360,                // 30 years
    ),
    "ACCT001".to_string(),
    "CUST001".to_string(),
    &SafeTimeProvider::system(),
)?;

// open-term bitcoin-backed loan
let btc_loan = Facility::originate(
    FacilityConfig::open_term(
        commitment: Money::from(50_000),  // $50k loan
        rate: Rate::from_decimal(0.08),   // 8% APR
        collateral_btc: Decimal::from(1), // 1 BTC collateral
    ),
    "ACCT002".to_string(),
    "CUST002".to_string(),
    &SafeTimeProvider::system(),
)?;
```

### Interest calculations
Interest accrual methods and timing:

```rust
// accrue daily interest
let accruals = facility.accrue_interest(&time)?;

// penalty interest for late payments
let penalty = facility.apply_penalty_interest(&time)?;

// check total interest accrued
let total_interest = facility.state.accrued_interest;
```

### Payment processing
Payment application through waterfall system:

```rust
// make a payment
let result = facility.process_payment(
    Money::from(1_500), // $1,500 payment
    &time,
)?;

// payment applied in order: fees -> penalties -> interest -> principal
println!("To interest: {}", result.application.to_interest);
println!("To principal: {}", result.application.to_principal);
```

## Key financial terms

### Amortization
Gradual repayment of debt through regular payments:

- **declining principal**: Each payment reduces principal, interest calculated on remaining balance
- **equal installments**: Fixed payment amounts throughout loan term

### Balloon payment
Large final payment at loan maturity, typically much larger than regular payments.

### Collateral
Assets securing a loan that can be seized upon default:

```rust
use credit_facility_rs::{CollateralPosition, Money};
use chrono::Utc;

let collateral = CollateralPosition {
    asset_type: "bitcoin".to_string(),
    asset_amount: Decimal::from(2),        // 2 BTC
    current_value: Money::from(120_000),   // $120,000 current value
    initial_value: Money::from(100_000),   // $100,000 initial value
    last_valuation: Utc::now(),
    valuation_source: "coinbase_pro".to_string(),
};

facility.update_collateral(collateral, &time)?;
```

### Current value of liability (CVL)
Total amount owed including all components:

```rust
let cvl = facility.state.outstanding_principal
    + facility.state.accrued_interest
    + facility.state.accrued_fees
    + facility.state.accrued_penalties;
```

### EMI (equated monthly installment)
Fixed payment amount for term loans:

```rust
use credit_facility_rs::payments::calculate_emi;

let monthly_payment = calculate_emi(
    principal: Money::from(250_000), // $250,000 loan
    rate: Rate::from_decimal(0.04),  // 4% APR
    term_months: 360,                // 30 years
); // Returns ~$1,194/month
```

### Grace period
Time after payment due date with no penalty:

```rust
// status changes based on days past due and grace period
facility.update_daily_status(&time)?;

match facility.state.status {
    FacilityStatus::Active => {}, // current on payments
    FacilityStatus::GracePeriod => {}, // late but within grace
    FacilityStatus::Delinquent => {}, // past grace period
    _ => {},
}
```

### Interest accrual
Accumulation of interest charges over time:

- **daily compounding**: Interest calculated daily on outstanding balance
- **penalty interest**: Additional rate applied to overdue amounts

```rust
// configure daily compounding
let config = FacilityConfig {
    interest_config: InterestConfig {
        day_count_convention: DayCountConvention::Actual365,
        compounding: CompoundingFrequency::Daily,
        penalty_config: Some(PenaltyConfig {
            rate_multiplier: Rate::from_decimal(1.5), // 1.5x base rate
            grace_period_days: 10,
        }),
    },
    // ... other config
};
```

### Liquidation
Process of selling collateral to repay defaulted loan:

```rust
// ltv thresholds trigger liquidation
let ltv_thresholds = LtvThresholds {
    initial_ltv: Rate::from_decimal(0.50),     // 50% max at origination
    warning_ltv: Rate::from_decimal(0.65),     // 65% warning
    margin_call_ltv: Rate::from_decimal(0.70), // 70% margin call
    liquidation_ltv: Rate::from_decimal(0.75), // 75% liquidation
};

// system automatically monitors ltv breaches
match facility.check_ltv_status() {
    LtvStatus::Healthy => {},
    LtvStatus::Warning => {}, // early warning
    LtvStatus::MarginCall => {}, // add collateral or pay down
    LtvStatus::Liquidation => {}, // automatic liquidation triggered
}
```

### Loan-to-value (LTV)
Ratio of debt to collateral value:

```rust
let ltv = facility.state.outstanding_principal.as_decimal()
    / collateral.current_value.as_decimal();

// convert to percentage
let ltv_percent = ltv * Decimal::from(100);
```

### Overpayment
Payment amounts exceeding required payment:

```rust
// configure overpayment handling
facility.config.payment_config.overpayment_strategy = OverpaymentStrategy::ReduceTerm;

// excess payment automatically applied per strategy
let payment_result = facility.process_payment(Money::from(2_000), &time)?;
if payment_result.application.excess > Money::ZERO {
    println!("Overpayment of {} applied", payment_result.application.excess);
}
```

### Payment waterfall
Order of payment application to debt components:

1. Fees (origination, late fees)
2. Penalties (penalty interest)
3. Interest (accrued interest)
4. Principal (loan balance)

```rust
// standard waterfall automatically applied
let application = facility.process_payment(amount, &time)?;

println!("Applied to fees: {}", application.application.to_fees);
println!("Applied to penalties: {}", application.application.to_penalties);
println!("Applied to interest: {}", application.application.to_interest);
println!("Applied to principal: {}", application.application.to_principal);
```

### Principal
Original loan amount borrowed, excluding interest and fees:

```rust
let outstanding_principal = facility.state.outstanding_principal;
let original_principal = facility.config.financial_terms.commitment_amount;
let principal_paid = original_principal - outstanding_principal;
```

## Facility lifecycle

### Origination
Creating a new credit facility:

```rust
// originate facility
let mut facility = Facility::originate(config, account, customer, &time)?;

// disburse funds
let disbursed = facility.disburse(Money::from(50_000), &time)?;
```

### Daily operations
Regular facility maintenance:

```rust
// daily status update handles:
// - interest accrual
// - payment due checking
// - grace period tracking
// - penalty application
facility.update_daily_status(&time)?;
```

### Settlement
Full loan payoff:

```rust
let payoff_amount = facility.state.total_outstanding();
let result = facility.process_payment(payoff_amount, &time)?;

// facility status becomes Settled
assert_eq!(facility.state.status, FacilityStatus::Settled);
```

## Event monitoring

The system emits comprehensive events for all facility activities:

```rust
// process events after operations
let events = facility.take_events();

for event in events {
    match event {
        Event::InterestAccrued { amount, .. } => {
            println!("Interest accrued: {}", amount);
        },
        Event::PaymentReceived { amount, application, .. } => {
            println!("Payment: {} -> Principal: {}", amount, application.to_principal);
        },
        Event::LtvWarningBreached { ltv_ratio, .. } => {
            println!("LTV warning: {}%", ltv_ratio.as_decimal() * 100);
        },
        _ => {},
    }
}
```

## Testing with time manipulation

Use `hourglass-rs` for deterministic time-based testing:

```rust
use hourglass_rs::{SafeTimeProvider, TimeSource};

// create test time provider
let time = SafeTimeProvider::new(TimeSource::Test("2024-01-01"));
let control = time.test_control().unwrap();

// advance time for testing
control.advance(Duration::days(30));
facility.update_daily_status(&time)?;

// verify interest accrual
assert!(facility.state.accrued_interest > Money::ZERO);
```

This provides a foundation for understanding financial concepts and their implementation in the credit facility system.