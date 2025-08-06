/// quick start - minimal example to get started
use credit_facility_rs::{Money, Rate};
use credit_facility_rs::facilities::TermLoanBuilder;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // create a $10,000 personal loan
    let mut loan = TermLoanBuilder::new()
        .amount(Money::from_major(10_000))
        .rate(Rate::from_percentage(8))
        .term_months(12)
        .build()?;
    
    // approve and disburse
    loan.approve()?;
    loan.disburse(Money::from_major(10_000))?;
    
    // make a payment
    loan.make_payment(Money::from_major(500))?;
    
    // print current state
    println!("{}", loan.json());
    
    Ok(())
}