/// revolving credit - draw, repay, redraw pattern
use credit_facility_rs::{Money, Rate, SafeTimeProvider, TimeSource};
use credit_facility_rs::facilities::RevolvingFacilityBuilder;
use credit_facility_rs::types::RevolvingType;
use chrono::{Duration, TimeZone, Utc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== revolving credit example ===\n");
    
    let time = SafeTimeProvider::new(TimeSource::Test(
        Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
    ));
    let controller = time.test_control().unwrap();
    
    // create credit card
    let mut card = RevolvingFacilityBuilder::new()
        .facility_type(RevolvingType::CreditCard)
        .credit_limit(Money::from_major(10_000))
        .rate(Rate::from_percentage(18))
        .minimum_percentage(rust_decimal_macros::dec!(0.02))  // 2% minimum
        .set_time(&time)
        .build()?;
    
    card.approve()?;
    println!("credit card approved");
    println!("  limit: $10,000");
    println!("  apr: 18%");
    println!("  minimum payment: 2% of balance");
    
    // month 1: initial purchase
    println!("\nmonth 1: january");
    card.draw(Money::from_major(3_000))?;
    println!("  purchase: $3,000");
    println!("  balance: $3,000");
    println!("  available: ${}", card.available_credit().as_decimal());
    
    // month 2: more purchases
    controller.advance(Duration::days(30));
    card.accrue_interest()?;
    println!("\nmonth 2: february");
    println!("  interest charged: ${:.2}", 
        card.facility().state.accrued_interest.as_decimal());
    
    card.draw(Money::from_major(1_500))?;
    println!("  new purchase: $1,500");
    
    let minimum = card.calculate_minimum_payment();
    println!("  minimum payment due: ${:.2}", minimum.as_decimal());
    
    // pay more than minimum
    card.make_payment(Money::from_major(500))?;
    println!("  payment made: $500");
    println!("  new balance: ${:.2}", 
        card.facility().state.outstanding_principal.as_decimal());
    println!("  available: ${:.2}", card.available_credit().as_decimal());
    
    // month 3: pay down significantly
    controller.advance(Duration::days(30));
    card.accrue_interest()?;
    println!("\nmonth 3: march");
    
    card.make_payment(Money::from_major(2_000))?;
    println!("  large payment: $2,000");
    println!("  balance: ${:.2}", 
        card.facility().state.outstanding_principal.as_decimal());
    println!("  available: ${:.2}", card.available_credit().as_decimal());
    
    // month 4: redraw available credit
    controller.advance(Duration::days(30));
    card.accrue_interest()?;
    println!("\nmonth 4: april");
    
    card.draw(Money::from_major(4_000))?;
    println!("  new purchases: $4,000");
    println!("  balance: ${:.2}", 
        card.facility().state.outstanding_principal.as_decimal());
    
    let utilization = card.utilization_rate();
    let state = card.utilization_state();
    println!("  utilization: {:.1}% ({:?})", 
        utilization.as_decimal() * rust_decimal_macros::dec!(100), state);
    
    // month 5: try to exceed limit
    controller.advance(Duration::days(30));
    card.accrue_interest()?;
    println!("\nmonth 5: may - overlimit test");
    
    let current_balance = card.facility().state.outstanding_principal;
    let available = card.available_credit();
    println!("  current balance: ${:.2}", current_balance.as_decimal());
    println!("  available credit: ${:.2}", available.as_decimal());
    
    // try to draw more than available (may charge overlimit fee)
    let overlimit_amount = available + Money::from_major(100);
    match card.draw(overlimit_amount) {
        Ok(drawn) => {
            println!("  ✓ overlimit draw allowed: ${}", drawn.as_decimal());
            println!("  overlimit fee charged: ${}", 
                card.facility().state.accrued_fees.as_decimal());
        }
        Err(e) => {
            println!("  ✗ overlimit draw rejected: {}", e);
        }
    }
    
    // pay off completely
    println!("\nmonth 6: june - pay off");
    controller.advance(Duration::days(30));
    card.accrue_interest()?;
    
    let total_due = card.facility().state.total_outstanding();
    println!("  total payoff amount: ${:.2}", total_due.as_decimal());
    
    card.make_payment(total_due)?;
    println!("  ✓ paid in full");
    println!("  balance: ${}", card.facility().state.outstanding_principal.as_decimal());
    println!("  available: ${}", card.available_credit().as_decimal());
    
    // can still use after paying off
    println!("\nmonth 7: july - use again");
    controller.advance(Duration::days(30));
    
    card.draw(Money::from_major(500))?;
    println!("  new purchase: $500");
    println!("  revolving credit continues!");
    
    Ok(())
}