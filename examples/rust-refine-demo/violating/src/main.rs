//! Demonstrates a genuinely self-contradictory refinement obligation:
//! this is valid, compiling Rust (the attribute is a no-op pass-through),
//! but no integer `x` can ever satisfy `x >= 10 && x < 5` -- `rust-refine`
//! flags it as violated rather than silently accepting it. Run
//! `cargo mvl-refine src/main.rs` (with the binary on `PATH`) to see the
//! diagnostic.

#[mvl::requires(x >= 10 && x < 5)]
fn impossible(x: i32) -> i32 {
    x
}

fn main() {
    println!("{}", impossible(7));
}
