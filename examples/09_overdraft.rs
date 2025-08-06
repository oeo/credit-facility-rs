/// overdraft - checking account overdraft protection
use credit_facility_rs::{Money, Rate, SafeTimeProvider, TimeSource};
use credit_facility_rs::facilities::OverdraftBuilder;
use chrono::{Duration, TimeZone, Utc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== overdraft protection example ===\n");
    
    let time = SafeTimeProvider::new(TimeSource::Test(
        Utc.with_ymd_and_hms(2024, 1, 15, 0, 0, 0).unwrap()
    ));
    let controller = time.test_control().unwrap();
    
    // create overdraft facility
    let mut overdraft = OverdraftBuilder::new()
        .overdraft_limit(Money::from_major(500))
        .rate(Rate::from_percentage(25))  // high rate typical for overdrafts
        .buffer_zone(Money::from_major(25))  // first $25 fee-free
        .linked_account_id("CHK-98765".to_string())
        .daily_fee(Money::from_major(5))  // $5/day when overdrawn
        .set_time(&time)
        .build()?;
    
    overdraft.approve()?;
    println!("overdraft protection activated");
    println!("  linked account: CHK-98765");
    println!("  overdraft limit: $500");
    println!("  buffer zone: $25 (no fees)");
    println!("  apr: 25%");
    println!("  daily fee: $5 when overdrawn");
    
    // simulate checking account activity
    println!("\nday 1: january 15");
    println!("-----------------");
    
    // deposit paycheck
    overdraft.make_payment(Money::from_major(2_000))?;
    println!("  deposit (paycheck): +$2,000");
    println!("  balance: $2,000");
    
    // pay bills
    overdraft.disburse(Money::from_major(1_200))?;
    println!("  rent payment: -$1,200");
    println!("  balance: $800");
    
    overdraft.disburse(Money::from_major(150))?;
    println!("  utilities: -$150");
    println!("  balance: $650");
    
    overdraft.disburse(Money::from_major(400))?;
    println!("  groceries: -$400");
    println!("  balance: $250");
    
    // day 2: unexpected expense
    controller.advance(Duration::days(1));
    println!("\nday 2: january 16");
    println!("-----------------");
    
    // car repair exceeds balance
    let balance = overdraft.disburse(Money::from_major(350))?;
    println!("  car repair: -$350");
    println!("  balance: ${} (overdraft active!)", balance.as_decimal());
    
    if balance < Money::ZERO {
        let overdraft_amount = Money::ZERO - balance;
        println!("  overdraft amount: ${}", overdraft_amount.as_decimal());
        
        if overdraft_amount <= Money::from_major(25) {
            println!("  status: in buffer zone (no fees)");
        } else {
            println!("  status: overdraft fees apply");
        }
    }
    
    // day 3: more transactions while overdrawn
    controller.advance(Duration::days(1));
    println!("\nday 3: january 17");
    println!("-----------------");
    
    // daily overdraft fee charged
    // daily fee would be charged here
    println!("  daily overdraft fee: -$5");
    
    // small purchase
    let balance = overdraft.disburse(Money::from_major(20))?;
    println!("  coffee shop: -$20");
    println!("  balance: ${}", balance.as_decimal());
    println!("  total overdrawn: ${}", (Money::ZERO - balance).as_decimal());
    
    // day 4: deposit to clear overdraft
    controller.advance(Duration::days(1));
    println!("\nday 4: january 18");
    println!("-----------------");
    
    // daily fee would be charged here
    println!("  daily overdraft fee: -$5");
    
    // deposit funds
    let balance = overdraft.make_payment(Money::from_major(500))?;
    println!("  deposit: +$500");
    println!("  balance: ${}", balance.as_decimal());
    
    if balance >= Money::ZERO {
        println!("  ✓ overdraft cleared!");
    }
    
    // summary
    println!("\noverdraft summary:");
    println!("-----------------");
    let fees = overdraft.facility().state.total_fees_charged;
    println!("  total fees charged: ${}", fees.as_decimal());
    println!("  current balance: ${}", balance.as_decimal());
    
    // show available funds
    let available = overdraft.available_funds();
    println!("  available funds: ${}", available.as_decimal());
    println!("    (includes $500 overdraft limit)");
    
    // demonstrate overdraft limit
    println!("\noverdraft limit test:");
    println!("--------------------");
    
    // try to exceed overdraft limit
    match overdraft.disburse(available + Money::from_major(100)) {
        Ok(_) => println!("  error: should not exceed limit!"),
        Err(e) => println!("  ✓ transaction declined: {}", e),
    }
    
    Ok(())
}