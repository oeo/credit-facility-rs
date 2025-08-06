/// json state - serialization for debugging and monitoring
use credit_facility_rs::{Money, Rate, SafeTimeProvider, TimeSource};
use credit_facility_rs::facilities::{TermLoanBuilder, RevolvingFacilityBuilder};
use credit_facility_rs::types::RevolvingType;
use chrono::{Duration, TimeZone, Utc};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== json state serialization ===\n");
    
    let time = SafeTimeProvider::new(TimeSource::Test(
        Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap()
    ));
    let controller = time.test_control().unwrap();
    
    // create a loan
    let mut loan = TermLoanBuilder::new()
        .amount(Money::from_major(50_000))
        .rate(Rate::from_percentage(7))
        .term_months(12)
        .set_time(&time)
        .build()?;
    
    // stage 1: after creation
    println!("stage 1: created (not yet approved)");
    println!("------------------------------------");
    println!("{}\n", loan.json());
    
    // stage 2: after approval
    loan.approve()?;
    println!("stage 2: approved (not yet disbursed)");
    println!("--------------------------------------");
    println!("{}\n", loan.json());
    
    // stage 3: after disbursement
    loan.disburse(Money::from_major(50_000))?;
    println!("stage 3: disbursed");
    println!("------------------");
    println!("{}\n", loan.json());
    
    // stage 4: after time passes
    controller.advance(Duration::days(30));
    loan.accrue_interest()?;
    println!("stage 4: 30 days later (interest accrued)");
    println!("------------------------------------------");
    println!("{}\n", loan.json());
    
    // stage 5: after payment
    loan.make_payment(Money::from_major(5_000))?;
    println!("stage 5: after $5,000 payment");
    println!("------------------------------");
    println!("{}\n", loan.json());
    
    // demonstrate revolving facility json
    println!("\n=== revolving facility json ===\n");
    
    let mut card = RevolvingFacilityBuilder::new()
        .facility_type(RevolvingType::CreditCard)
        .credit_limit(Money::from_major(5_000))
        .rate(Rate::from_percentage(18))
        .set_time(&time)
        .build()?;
    
    card.approve()?;
    card.draw(Money::from_major(1_500))?;
    
    println!("credit card after $1,500 draw:");
    println!("------------------------------");
    println!("{}", card.json());
    
    Ok(())
}