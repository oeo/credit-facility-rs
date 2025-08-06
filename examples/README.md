# credit facility examples

clean, focused examples demonstrating the new api patterns.

## examples

### 01_basic_usage.rs
simple loan origination and payment using default system time.

```bash
cargo run --example 01_basic_usage
```

### 02_time_control.rs  
deterministic testing with controlled time advancement.

```bash
cargo run --example 02_time_control
```

### 03_facility_types.rs
showcase of all facility types: term loans, revolving credit, open-term loans, overdrafts.

```bash
cargo run --example 03_facility_types
```

### 04_shared_time.rs
multiple facilities sharing the same time reference for synchronized operations.

```bash
cargo run --example 04_shared_time
```

### 05_json_state.rs
json serialization at different stages of facility lifecycle.

```bash
cargo run --example 05_json_state
```

### 06_lifecycle.rs
complete facility lifecycle from origination through settlement.

```bash
cargo run --example 06_lifecycle
```

### 07_bitcoin_loan.rs
bitcoin-backed loan with ltv monitoring and margin calls.

```bash
cargo run --example 07_bitcoin_loan
```

### 08_revolving_credit.rs
credit card with draw, repay, and redraw cycles.

```bash
cargo run --example 08_revolving_credit
```

### 09_overdraft.rs
checking account overdraft protection with daily fees.

```bash
cargo run --example 09_overdraft
```

## key patterns

### time control
```rust
// test time with controller
let time = SafeTimeProvider::new(TimeSource::Test(
    Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
));
let controller = time.test_control().unwrap();

// advance time
controller.advance(Duration::days(30));
```

### clean api
```rust
// no time parameters needed
let mut loan = TermLoanBuilder::new()
    .amount(Money::from_major(10_000))
    .rate(Rate::from_percentage(8))
    .set_time(&time)  // optional
    .build()?;

loan.approve()?;
loan.disburse(amount)?;
loan.make_payment(payment)?;
```

### shared time
```rust
// multiple facilities, same time
let mut loan1 = Builder::new().set_time(&time).build()?;
let mut loan2 = Builder::new().set_time(&time).build()?;

controller.advance(Duration::days(30));
// both see the same date
```

### json state
```rust
// serialize for debugging
println!("{}", facility.json());
```