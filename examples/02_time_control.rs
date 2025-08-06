/// time control - deterministic testing with controlled time
use credit_facility_rs::{Money, Rate, SafeTimeProvider, TimeSource};
use credit_facility_rs::facilities::TermLoanBuilder;
use chrono::{Duration, TimeZone, Utc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== time control example ===\n");
    
    // create controlled time for testing
    let time = SafeTimeProvider::new(TimeSource::Test(
        Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
    ));
    let controller = time.test_control().unwrap();
    
    println!("starting date: {}", time.now().format("%Y-%m-%d"));
    
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
    println!("loan originated on {}", time.now().format("%Y-%m-%d"));
    
    // advance 30 days
    controller.advance(Duration::days(30));
    println!("\nadvanced to: {}", time.now().format("%Y-%m-%d"));
    
    // accrue interest for 30 days
    loan.accrue_interest()?;
    let interest = loan.facility().state.accrued_interest;
    println!("interest accrued (30 days): ${}", interest.as_decimal());
    
    // make first payment
    loan.process_scheduled_payment()?;
    println!("first payment processed");
    
    // advance 6 months
    controller.advance(Duration::days(180));
    println!("\nadvanced to: {} (6 months later)", time.now().format("%Y-%m-%d"));
    
    // process 6 monthly payments
    for month in 2..=7 {
        loan.process_scheduled_payment()?;
        println!("payment {} processed", month);
    }
    
    // check balance
    let outstanding = loan.facility().state.outstanding_principal;
    println!("\noutstanding after 7 payments: ${}", outstanding.as_decimal());
    
    // advance to end of loan term
    controller.advance(Duration::days(29 * 30));  // remaining 29 months
    println!("\nadvanced to end of term: {}", time.now().format("%Y-%m-%d"));
    
    // process remaining scheduled payments
    for _month in 8..=36 {
        if !loan.facility().state.total_outstanding().is_zero() {
            loan.process_scheduled_payment()?;
        }
    }
    
    // check if fully paid
    let remaining = loan.facility().state.total_outstanding();
    if !remaining.is_zero() {
        println!("\nsmall balance remaining: ${:.2}", remaining.as_decimal());
        println!("making final payment to clear...");
        loan.make_payment(remaining)?;
    }
    
    println!("\nloan fully paid off!");
    println!("final status: {:?}", loan.facility().state.status);
    
    Ok(())
}