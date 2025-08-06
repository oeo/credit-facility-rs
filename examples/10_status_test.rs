/// status test - verify status transitions work correctly
use credit_facility_rs::{Money, Rate, SafeTimeProvider, TimeSource};
use credit_facility_rs::facilities::TermLoanBuilder;
use credit_facility_rs::types::FacilityStatus;
use chrono::{Duration, TimeZone, Utc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== status transition test ===\n");
    
    let time = SafeTimeProvider::new(TimeSource::Test(
        Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
    ));
    let controller = time.test_control().unwrap();
    
    // create a small 3-month loan for easy testing
    let mut loan = TermLoanBuilder::new()
        .amount(Money::from_major(3_000))
        .rate(Rate::from_percentage(12))
        .term_months(3)
        .set_time(&time)
        .build()?;
    
    // check initial status
    println!("1. initial status: {:?}", loan.facility().state.status);
    assert_eq!(loan.facility().state.status, FacilityStatus::Originated);
    
    // approve - should become Active
    loan.approve()?;
    println!("2. after approve: {:?}", loan.facility().state.status);
    assert_eq!(loan.facility().state.status, FacilityStatus::Active);
    
    // disburse - should stay Active
    loan.disburse(Money::from_major(3_000))?;
    println!("3. after disburse: {:?}", loan.facility().state.status);
    assert_eq!(loan.facility().state.status, FacilityStatus::Active);
    
    // make all 3 payments
    for month in 1..=3 {
        controller.advance(Duration::days(30));
        loan.accrue_interest()?;
        loan.process_scheduled_payment()?;
        println!("   payment {}: status = {:?}, outstanding = ${}", 
            month, 
            loan.facility().state.status,
            loan.facility().state.total_outstanding().as_decimal());
    }
    
    // after final payment, should be Settled
    println!("4. after all payments: {:?}", loan.facility().state.status);
    let outstanding = loan.facility().state.total_outstanding();
    println!("   total outstanding: ${}", outstanding.as_decimal());
    
    if outstanding.is_zero() {
        println!("   ✓ loan fully paid (outstanding = $0)");
        if loan.facility().state.status != FacilityStatus::Settled {
            println!("   ⚠️  WARNING: Status should be 'Settled' but is '{:?}'", 
                loan.facility().state.status);
        }
    }
    
    // test early payoff
    println!("\n5. testing early payoff:");
    let mut loan2 = TermLoanBuilder::new()
        .amount(Money::from_major(5_000))
        .rate(Rate::from_percentage(10))
        .term_months(12)
        .set_time(&time)
        .build()?;
    
    loan2.approve()?;
    loan2.disburse(Money::from_major(5_000))?;
    
    // advance one month
    controller.advance(Duration::days(30));
    loan2.accrue_interest()?;
    
    // pay off entire balance
    let payoff = loan2.facility().state.total_outstanding();
    println!("   payoff amount: ${}", payoff.as_decimal());
    loan2.make_payment(payoff)?;
    
    println!("   after payoff: status = {:?}, outstanding = ${}", 
        loan2.facility().state.status,
        loan2.facility().state.total_outstanding().as_decimal());
    
    if loan2.facility().state.total_outstanding().is_zero() {
        println!("   ✓ loan fully paid");
        if loan2.facility().state.status != FacilityStatus::Settled {
            println!("   ⚠️  WARNING: Status should be 'Settled' but is '{:?}'", 
                loan2.facility().state.status);
        }
    }
    
    // test denied loan
    println!("\n6. testing denied loan:");
    let mut loan3 = TermLoanBuilder::new()
        .amount(Money::from_major(10_000))
        .rate(Rate::from_percentage(15))
        .term_months(24)
        .set_time(&time)
        .build()?;
    
    loan3.deny()?;
    println!("   after deny: status = {:?}", loan3.facility().state.status);
    
    if loan3.facility().state.status == FacilityStatus::Settled {
        println!("   ⚠️  WARNING: Denied loans should not use 'Settled' status");
        println!("              Consider adding a 'Denied' or 'Cancelled' status");
    }
    
    Ok(())
}