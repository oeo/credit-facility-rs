/// lifecycle - complete facility lifecycle from origination to settlement
use credit_facility_rs::{Money, Rate, SafeTimeProvider, TimeSource};
use credit_facility_rs::facilities::TermLoanBuilder;
use credit_facility_rs::types::FacilityStatus;
use chrono::{Duration, TimeZone, Utc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== facility lifecycle ===\n");
    
    let time = SafeTimeProvider::new(TimeSource::Test(
        Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
    ));
    let controller = time.test_control().unwrap();
    
    // create a 6-month loan for easier demonstration
    let mut loan = TermLoanBuilder::new()
        .amount(Money::from_major(10_000))
        .rate(Rate::from_percentage(12))
        .term_months(6)
        .set_time(&time)
        .build()?;
    
    // 1. origination
    println!("1. origination phase");
    println!("-------------------");
    println!("  date: {}", time.now().format("%Y-%m-%d"));
    println!("  status: {:?}", loan.facility().state.status);
    
    loan.approve()?;
    println!("  ✓ loan approved");
    println!("  status: {:?}", loan.facility().state.status);
    
    // 2. disbursement
    println!("\n2. disbursement phase");
    println!("--------------------");
    let amount = loan.disburse(Money::from_major(10_000))?;
    println!("  ✓ disbursed: ${}", amount.as_decimal());
    println!("  outstanding: ${}", loan.facility().state.outstanding_principal.as_decimal());
    
    // 3. normal servicing (3 on-time payments)
    println!("\n3. normal servicing phase");
    println!("-------------------------");
    
    for month in 1..=3 {
        // advance to payment date
        controller.advance(Duration::days(30));
        println!("\n  month {}: {}", month, time.now().format("%Y-%m-%d"));
        
        // accrue interest
        loan.accrue_interest()?;
        let interest = loan.facility().state.accrued_interest;
        println!("    interest accrued: ${:.2}", interest.as_decimal());
        
        // process payment
        loan.process_scheduled_payment()?;
        println!("    ✓ payment processed");
        println!("    remaining balance: ${:.2}", 
            loan.facility().state.outstanding_principal.as_decimal());
    }
    
    // 4. grace period demonstration
    println!("\n4. grace period phase");
    println!("--------------------");
    controller.advance(Duration::days(30));
    println!("  date: {}", time.now().format("%Y-%m-%d"));
    println!("  payment due but not made");
    
    // update status - enters grace period
    controller.advance(Duration::days(1));
    loan.update_daily_status()?;
    println!("  day 1 past due: status = {:?}", loan.facility().state.status);
    
    // stay in grace for a few days
    for day in 2..=5 {
        controller.advance(Duration::days(1));
        loan.update_daily_status()?;
        println!("  day {} past due: still in grace", day);
    }
    
    // make late payment within grace period
    println!("\n  making late payment on day 5...");
    loan.process_scheduled_payment()?;
    println!("  ✓ payment accepted");
    println!("  status: {:?}", loan.facility().state.status);
    
    // 5. early payoff
    println!("\n5. early payoff phase");
    println!("--------------------");
    controller.advance(Duration::days(25));
    println!("  date: {}", time.now().format("%Y-%m-%d"));
    println!("  2 payments remaining in schedule");
    
    let payoff_amount = loan.facility().state.total_outstanding();
    println!("  total payoff amount: ${:.2}", payoff_amount.as_decimal());
    
    loan.make_payment(payoff_amount)?;
    println!("  ✓ loan paid off early");
    println!("  final status: {:?}", loan.facility().state.status);
    assert_eq!(loan.facility().state.status, FacilityStatus::Settled);
    
    // 6. post-settlement
    println!("\n6. post-settlement");
    println!("-----------------");
    println!("  outstanding principal: ${}", 
        loan.facility().state.outstanding_principal.as_decimal());
    println!("  outstanding interest: ${}", 
        loan.facility().state.accrued_interest.as_decimal());
    println!("  total outstanding: ${}", 
        loan.facility().state.total_outstanding().as_decimal());
    
    // demonstrate denied loan
    println!("\n7. denied loan example");
    println!("---------------------");
    let mut denied_loan = TermLoanBuilder::new()
        .amount(Money::from_major(100_000))
        .rate(Rate::from_percentage(15))
        .term_months(12)
        .set_time(&time)
        .build()?;
    
    denied_loan.deny()?;
    println!("  ✗ loan denied");
    println!("  status: {:?}", denied_loan.facility().state.status);
    
    // try to disburse (should fail)
    match denied_loan.disburse(Money::from_major(100_000)) {
        Ok(_) => println!("  error: should not disburse denied loan!"),
        Err(e) => println!("  ✓ cannot disburse: {}", e),
    }
    
    Ok(())
}