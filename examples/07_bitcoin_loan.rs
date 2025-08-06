/// bitcoin loan - open-term loan with ltv monitoring
use credit_facility_rs::{Money, Rate, SafeTimeProvider, TimeSource};
use credit_facility_rs::facilities::OpenTermLoanBuilder;
use credit_facility_rs::types::LtvStatus;
use chrono::{Duration, TimeZone, Utc};
use rust_decimal_macros::dec;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== bitcoin-backed loan example ===\n");
    
    let time = SafeTimeProvider::new(TimeSource::Test(
        Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
    ));
    let controller = time.test_control().unwrap();
    
    // create bitcoin-backed loan
    let mut loan = OpenTermLoanBuilder::new()
        .amount(Money::from_major(100_000))
        .rate(Rate::from_percentage(5))
        .btc_collateral(dec!(3))  // 3 btc
        .btc_price(Money::from_major(50_000))  // $50k per btc
        .set_time(&time)
        .build()?;
    
    println!("loan setup:");
    println!("  amount: $100,000");
    println!("  collateral: 3 BTC @ $50,000 = $150,000");
    println!("  initial ltv: 66.7%");
    println!("  rate: 5% (perpetual)");
    
    // approve and disburse
    loan.approve()?;
    loan.disburse(Money::from_major(100_000))?;
    println!("\n✓ loan approved and disbursed");
    
    // simulate 6 months
    controller.advance(Duration::days(180));
    println!("\nafter 6 months:");
    
    // accrue interest (no payment required)
    for _ in 0..180 {
        loan.accrue_interest()?;
    }
    
    let interest = loan.facility().state.accrued_interest;
    println!("  accrued interest: ${:.2}", interest.as_decimal());
    println!("  total debt: ${:.2}", loan.facility().state.total_outstanding().as_decimal());
    
    // scenario 1: btc price increases
    println!("\nscenario 1: btc rallies to $70,000");
    let status = loan.update_btc_price(Money::from_major(70_000))?;
    let ltv = loan.calculate_ltv();
    println!("  new collateral value: $210,000");
    println!("  new ltv: {:.1}%", ltv.as_decimal() * dec!(100));
    println!("  status: {:?}", status);
    
    // optional payment
    loan.make_payment(Money::from_major(10_000))?;
    println!("  made optional payment: $10,000");
    println!("  new ltv: {:.1}%", loan.calculate_ltv().as_decimal() * dec!(100));
    
    // scenario 2: btc price crashes
    println!("\nscenario 2: btc crashes to $30,000");
    let status = loan.update_btc_price(Money::from_major(30_000))?;
    let ltv = loan.calculate_ltv();
    println!("  new collateral value: $90,000");
    println!("  new ltv: {:.1}%", ltv.as_decimal() * dec!(100));
    println!("  status: {:?}", status);
    
    match status {
        LtvStatus::MarginCall => {
            println!("\n⚠️  margin call triggered!");
            println!("  options:");
            println!("    1. add more btc collateral");
            println!("    2. pay down principal");
            println!("    3. pay off loan to retrieve btc");
            
            // option 1: add collateral
            println!("\n  choosing option 1: add 1 btc");
            loan.add_collateral(dec!(1))?;
            println!("  new collateral: 4 btc");
            println!("  new ltv: {:.1}%", loan.calculate_ltv().as_decimal() * dec!(100));
        }
        _ => {}
    }
    
    // advance 1 year
    controller.advance(Duration::days(365));
    println!("\nafter 1 more year:");
    
    for _ in 0..365 {
        loan.accrue_interest()?;
    }
    
    println!("  total interest accrued: ${:.2}", 
        loan.facility().state.accrued_interest.as_decimal());
    println!("  total debt: ${:.2}", 
        loan.facility().state.total_outstanding().as_decimal());
    
    // payoff and release collateral
    println!("\npaying off loan:");
    let payoff = loan.total_payoff_amount();
    println!("  payoff amount: ${:.2}", payoff.as_decimal());
    
    let btc_released = loan.payoff_and_release()?;
    println!("  ✓ loan paid off");
    println!("  btc released: {}", btc_released);
    println!("  final status: {:?}", loan.facility().state.status);
    
    Ok(())
}