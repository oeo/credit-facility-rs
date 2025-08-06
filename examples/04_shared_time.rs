/// shared time - multiple facilities sharing the same time reference
use credit_facility_rs::{Money, Rate, SafeTimeProvider, TimeSource};
use credit_facility_rs::facilities::{TermLoanBuilder, OpenTermLoanBuilder};
use chrono::{Duration, TimeZone, Utc};
use rust_decimal_macros::dec;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== shared time reference ===\n");
    
    // single time source shared by all facilities
    let time = SafeTimeProvider::new(TimeSource::Test(
        Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
    ));
    let controller = time.test_control().unwrap();
    
    println!("initial date: {}\n", time.now().format("%Y-%m-%d"));
    
    // create first loan
    let mut personal_loan = TermLoanBuilder::new()
        .amount(Money::from_major(20_000))
        .rate(Rate::from_percentage(10))
        .term_months(24)
        .set_time(&time)  // share time reference
        .build()?;
    
    // create second loan  
    let mut btc_loan = OpenTermLoanBuilder::new()
        .amount(Money::from_major(50_000))
        .rate(Rate::from_percentage(5))
        .btc_collateral(dec!(2))
        .btc_price(Money::from_major(50_000))
        .set_time(&time)  // same time reference
        .build()?;
    
    // originate both on the same day
    personal_loan.approve()?;
    personal_loan.disburse(Money::from_major(20_000))?;
    println!("personal loan originated: {}", time.now().format("%Y-%m-%d"));
    
    btc_loan.approve()?;
    btc_loan.disburse(Money::from_major(50_000))?;
    println!("bitcoin loan originated: {}", time.now().format("%Y-%m-%d"));
    
    // advance time - affects both loans
    controller.advance(Duration::days(30));
    println!("\nadvanced to: {}", time.now().format("%Y-%m-%d"));
    
    // accrue interest on both
    personal_loan.accrue_interest()?;
    btc_loan.accrue_interest()?;
    
    println!("\nafter 30 days:");
    println!("  personal loan interest: ${}", 
        personal_loan.facility().state.accrued_interest.as_decimal());
    println!("  bitcoin loan interest: ${}", 
        btc_loan.facility().state.accrued_interest.as_decimal());
    
    // advance 6 months
    controller.advance(Duration::days(180));
    println!("\nadvanced to: {} (6 months later)", time.now().format("%Y-%m-%d"));
    
    // simulate bitcoin price change
    let new_btc_price = Money::from_major(35_000);
    let ltv_status = btc_loan.update_btc_price(new_btc_price)?;
    println!("\nbitcoin price dropped to $35,000");
    println!("  new ltv: {:.1}%", btc_loan.calculate_ltv().as_decimal() * dec!(100));
    println!("  status: {:?}", ltv_status);
    
    // make payments on the same day
    personal_loan.make_payment(Money::from_major(1_000))?;
    println!("\npersonal loan payment: $1,000");
    
    btc_loan.make_payment(Money::from_major(5_000))?;
    println!("bitcoin loan payment: $5,000");
    
    // advance 1 year
    controller.advance(Duration::days(365));
    println!("\nadvanced to: {} (1 year later)", time.now().format("%Y-%m-%d"));
    
    // accrue interest for the year
    for _ in 0..365 {
        personal_loan.accrue_interest()?;
        btc_loan.accrue_interest()?;
    }
    
    println!("\nafter 1.5 years total:");
    println!("  personal loan outstanding: ${}", 
        personal_loan.facility().state.total_outstanding().as_decimal());
    println!("  bitcoin loan outstanding: ${}", 
        btc_loan.facility().state.total_outstanding().as_decimal());
    
    // demonstrate synchronized operations
    println!("\nsynchronized operations:");
    println!("  both loans see: {}", time.now().format("%Y-%m-%d"));
    println!("  days since origination: {}", 
        (time.now() - Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()).num_days());
    
    Ok(())
}