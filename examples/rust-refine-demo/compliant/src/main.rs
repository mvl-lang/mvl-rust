//! Demonstrates `rust-refine`'s native `L1`+`L2` obligation dispatch
//! (ADR-0001) with satisfiable interval bounds -- the `mask_low_nibble`
//! example from #22's real-`mvl`-example comparison.

#[mvl::total]
#[mvl::requires(0 <= b && b <= 255)]
#[mvl::ensures(0 <= result && result <= 15)]
fn mask_low_nibble(b: i32) -> i32 {
    b & 15
}

fn main() {
    println!("mask_low_nibble(200) = {}", mask_low_nibble(200));
}
