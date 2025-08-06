/// basic usage - simple loan origination and payment
use credit_facility_rs::{Money, Rate};
use credit_facility_rs::facilities::TermLoanBuilder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== basic usage example ===\n");
    
    // production: use system time (default)
    let mut loan = TermLoanBuilder::new()
        .amount(Money::from_major(10_000))
        .rate(Rate::from_percentage(8))
        .term_months(12)
        .build()?;
    
    // approve and disburse
    loan.approve()?;
    println!("loan approved");
    
    let disbursed = loan.disburse(Money::from_major(10_000))?;
    println!("disbursed: ${}", disbursed.as_decimal());
    
    // make a payment
    loan.make_payment(Money::from_major(500))?;
    println!("payment processed: $500");
    
    // check state
    println!("\ncurrent state:");
    println!("{}", loan.json());
    
    Ok(())
}