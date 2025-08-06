# credit-facility-rs

A Rust library for modeling and managing various types of credit facilities including term loans, revolving credit, open-term loans, and overdraft facilities. Provides deterministic time control for testing and financial calculations.

## Features

- **Multiple facility types**
  - Term loans (personal, mortgage, auto)
  - Revolving credit (credit cards, HELOC)
  - Open-term loans with collateral
  - Overdraft facilities
  
- **Time control system**
  - Deterministic time manipulation for testing
  - Real-time and controlled time modes
  - Built on hourglass-rs SafeTimeProvider

- **Financial calculations**
  - Interest accrual with multiple conventions
  - Payment processing and amortization
  - Penalty calculations and grace periods
  - Collateral monitoring and liquidation

- **API design**
  - Unified primitives: `approve`, `deny`, `disburse`, `make_payment`, `json`
  - Builder pattern with `.set_time(&time).build()`
  - JSON serialization for state inspection

## Quick start

```rust
use credit_facility_rs::{Money, Rate};
use credit_facility_rs::facilities::TermLoanBuilder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // create a $10,000 personal loan
    let mut loan = TermLoanBuilder::new()
        .amount(Money::from_major(10_000))
        .rate(Rate::from_percentage(8))
        .term_months(12)
        .build()?;
    
    // approve and disburse
    loan.approve()?;
    loan.disburse(Money::from_major(10_000))?;
    
    // make a payment
    loan.make_payment(Money::from_major(500))?;
    
    // print current state
    println!("{}", loan.json());
    
    Ok(())
}
```

## Time control for testing

```rust
use credit_facility_rs::{Money, Rate, SafeTimeProvider, TimeSource};
use credit_facility_rs::facilities::TermLoanBuilder;
use chrono::{Duration, TimeZone, Utc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // create controlled time for testing
    let time = SafeTimeProvider::new(TimeSource::Test(
        Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
    ));
    let controller = time.test_control().unwrap();
    
    // create loan with controlled time
    let mut loan = TermLoanBuilder::new()
        .amount(Money::from_major(100_000))
        .rate(Rate::from_percentage(5))
        .term_months(36)
        .set_time(&time)  // set time controller
        .build()?;
    
    // originate at t=0
    loan.approve()?;
    loan.disburse(Money::from_major(100_000))?;
    
    // advance 30 days
    controller.advance(Duration::days(30));
    
    // accrue interest and make payment
    loan.accrue_interest()?;
    loan.process_scheduled_payment()?;
    
    Ok(())
}
```

## API patterns

All facility types follow consistent patterns:

### Core operations
- `approve()` - approve the facility for use
- `deny()` - deny/cancel the facility  
- `disburse(amount)` - disburse funds
- `make_payment(amount)` - process a payment
- `json()` - get JSON representation of current state

### Time management
- `.set_time(&time)` - set time provider during construction
- `.build()` - build facility with stored or system time
- `accrue_interest()` - accrue interest using stored time
- `update_daily_status()` - update status using stored time

### State inspection
- `.facility()` - access underlying facility data
- `.json()` - pretty-printed JSON state
- Status lifecycle: `Originated → Active → Settled/GracePeriod/Delinquent`

## Examples

The `examples/` directory contains 11 examples:

- **00_quick_start** - minimal usage example
- **01_basic_usage** - basic facility operations
- **02_time_control** - deterministic time manipulation
- **03_facility_types** - different facility types
- **04_shared_time** - shared time across facilities
- **05_json_state** - JSON serialization
- **06_lifecycle** - facility status lifecycle
- **07_bitcoin_loan** - collateralized bitcoin loan
- **08_revolving_credit** - revolving credit facility
- **09_overdraft** - overdraft facility
- **10_status_test** - status transition testing

Run examples with:
```bash
cargo run --example 00_quick_start
cargo run --example 02_time_control
```

## Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
credit-facility-rs = "0.1.0"
```

Dependencies:
- `rust_decimal` - precise decimal arithmetic
- `chrono` - date/time handling
- `hourglass-rs` - time control for testing
- `uuid` - unique identifiers
- `serde` - JSON serialization

## Testing

Run all tests:
```bash
cargo test
```

Runs 88 unit tests covering facility types, interest calculations, payment processing, collateral management, and time manipulation.

## Architecture

The library is organized into modules:

- **facilities/** - facility implementations (term_loan, revolving, open_term, overdraft)
- **interest/** - interest calculation engines
- **payments/** - payment processing and amortization
- **collateral/** - collateral management and liquidation
- **decimal/** - precise decimal types (Money, Rate)
- **config/** - facility configuration
- **state/** - facility state management
- **events/** - event system for auditing

## Known limitations

- Denied loans use `Settled` status (no `Cancelled` status yet)
- Scheduled payments may leave small residual balances due to rounding
- Some edge cases in payment timing and status transitions
