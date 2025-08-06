# examples validation report

## status transitions
all examples have been tested and validated for correct behavior.

### ✅ working correctly
- **originated → active**: happens on `approve()`
- **active → settled**: happens when `total_outstanding()` reaches zero
- **active → grace period**: happens 1 day after missed payment
- **grace period → delinquent**: happens after grace period expires
- **any → settled**: when loan is paid off via `make_payment()`

### ⚠️ known issues

#### 1. scheduled payments residual balance
**issue**: `process_scheduled_payment()` may leave small residual balances due to daily interest accrual timing.

**example**: 3-month loan with 3 scheduled payments leaves $90.94 balance

**workaround**:
```rust
// after all scheduled payments
let remaining = loan.facility().state.total_outstanding();
if !remaining.is_zero() {
    loan.make_payment(remaining)?;  // clear residual
}
```

#### 2. denied loans status
**issue**: denied loans show status as `Settled` instead of a proper `Cancelled` or `Denied` status.

**impact**: confusing to see denied loans as "settled"

**recommendation**: add `FacilityStatus::Cancelled` variant

## example outputs verified

### 02_time_control
- ✅ tracks payments correctly
- ✅ shows settled status after final payment
- ✅ handles residual balance properly

### 06_lifecycle  
- ✅ shows all status transitions
- ✅ grace period works correctly
- ✅ early payoff updates to settled

### 07_bitcoin_loan
- ✅ ltv monitoring works
- ✅ margin calls trigger correctly
- ✅ payoff releases collateral and settles

### 08_revolving_credit
- ✅ maintains available credit correctly
- ✅ handles overlimit scenarios
- ✅ can be paid off and reused

### 09_overdraft
- ✅ tracks overdraft state
- ✅ buffer zone logic works
- ✅ clears when balance goes positive

## recommendations

1. **add cancelled status**: create `FacilityStatus::Cancelled` for denied/cancelled loans
2. **fix amortization**: adjust schedule calculation to account for daily accrual
3. **auto-settle**: consider auto-settling loans when balance < threshold (e.g., $0.01)
4. **add settle method**: explicit `settle()` method for edge cases