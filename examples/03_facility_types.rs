/// facility types - showcase different facility types
use credit_facility_rs::{Money, Rate, SafeTimeProvider, TimeSource};
use credit_facility_rs::facilities::{
    TermLoanBuilder, RevolvingFacilityBuilder, OpenTermLoanBuilder, OverdraftBuilder
};
use credit_facility_rs::types::{TermLoanType, RevolvingType};
use chrono::{TimeZone, Utc};
use rust_decimal_macros::dec;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== facility types showcase ===\n");
    
    // use test time for consistent output
    let time = SafeTimeProvider::new(TimeSource::Test(
        Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
    ));
    
    // 1. term loan (mortgage)
    println!("1. mortgage loan");
    println!("-----------------");
    let mut mortgage = TermLoanBuilder::new()
        .loan_type(TermLoanType::Mortgage)
        .amount(Money::from_major(300_000))
        .rate(Rate::from_percentage(4))
        .term_months(360)
        .property_value(Money::from_major(400_000))
        .set_time(&time)
        .build()?;
    
    mortgage.approve()?;
    mortgage.disburse(Money::from_major(300_000))?;
    println!("  amount: $300,000");
    println!("  rate: 4%");
    println!("  term: 30 years");
    println!("  monthly payment: ${}", 
        mortgage.facility().state.next_payment_amount
            .map(|m| m.as_decimal().to_string())
            .unwrap_or("N/A".to_string()));
    
    // 2. revolving facility (credit card)
    println!("\n2. credit card");
    println!("---------------");
    let mut card = RevolvingFacilityBuilder::new()
        .facility_type(RevolvingType::CreditCard)
        .credit_limit(Money::from_major(10_000))
        .rate(Rate::from_percentage(18))
        .set_time(&time)
        .build()?;
    
    card.approve()?;
    card.draw(Money::from_major(2_500))?;
    println!("  credit limit: $10,000");
    println!("  apr: 18%");
    println!("  current balance: $2,500");
    println!("  available credit: ${}", card.available_credit().as_decimal());
    
    // 3. open-term loan (bitcoin-backed)
    println!("\n3. bitcoin-backed loan");
    println!("----------------------");
    let mut btc_loan = OpenTermLoanBuilder::new()
        .amount(Money::from_major(50_000))
        .rate(Rate::from_percentage(5))
        .btc_collateral(dec!(2))
        .btc_price(Money::from_major(45_000))
        .set_time(&time)
        .build()?;
    
    btc_loan.approve()?;
    btc_loan.disburse(Money::from_major(50_000))?;
    let ltv = btc_loan.calculate_ltv();
    println!("  loan amount: $50,000");
    println!("  collateral: 2 BTC @ $45,000");
    println!("  ltv ratio: {:.1}%", ltv.as_decimal() * dec!(100));
    println!("  interest rate: 5%");
    println!("  term: perpetual (no maturity)");
    
    // 4. overdraft facility
    println!("\n4. overdraft facility");
    println!("--------------------");
    let mut overdraft = OverdraftBuilder::new()
        .overdraft_limit(Money::from_major(1_000))
        .rate(Rate::from_percentage(20))
        .buffer_zone(Money::from_major(50))
        .linked_account_id("CHK-12345".to_string())
        .set_time(&time)
        .build()?;
    
    overdraft.approve()?;
    
    // simulate account balance going negative
    overdraft.make_payment(Money::from_major(200))?;  // deposit
    let balance = overdraft.disburse(Money::from_major(250))?;  // withdraw more than balance
    
    println!("  overdraft limit: $1,000");
    println!("  buffer zone: $50 (no fees)");
    println!("  apr: 20%");
    println!("  current balance: ${}", balance.as_decimal());
    println!("  overdraft active: {}", 
        if balance < Money::ZERO { "yes" } else { "no" });
    
    // 5. revolving facility (line of credit)
    println!("\n5. business line of credit");
    println!("--------------------------");
    let mut loc = RevolvingFacilityBuilder::new()
        .facility_type(RevolvingType::LineOfCredit)
        .credit_limit(Money::from_major(500_000))
        .rate(Rate::from_percentage(8))
        .commitment_fee_rate(Rate::from_decimal(dec!(0.005)))  // 0.5%
        .set_time(&time)
        .build()?;
    
    loc.approve()?;
    loc.draw(Money::from_major(150_000))?;
    println!("  credit limit: $500,000");
    println!("  rate: 8%");
    println!("  commitment fee: 0.5% on undrawn");
    println!("  drawn: $150,000");
    println!("  undrawn: $350,000");
    
    Ok(())
}