//! Demonstrates every construct `rust-total` rejects. This is valid,
//! compiling Rust — it just falls outside what `#[mvl::total]` permits.
//! Run `cargo mvl-total src/main.rs` (with the binary on `PATH`) to see
//! the diagnostics.
//!
//! Note: a genuinely non-exhaustive match (no wildcard, missing variants)
//! is already a hard `rustc` compile error on its own — `rust-total`
//! doesn't need to add anything there, so it isn't demonstrated here (it
//! would just fail `cargo build`, not `cargo mvl-total`).

#[mvl::total]
fn factorial(n: u64) -> u64 {
    // missing #[mvl::decreases(n)]: outside the qualified subset
    if n == 0 {
        1
    } else {
        n * factorial(n - 1)
    }
}

#[mvl::total]
fn first(v: Vec<i32>) -> i32 {
    v[0] // raw indexing: outside the qualified subset
}

#[mvl::total]
fn half(a: i32, b: i32) -> i32 {
    a / b // division: outside the qualified subset
}

#[mvl::total]
fn must_have(x: Option<i32>) -> i32 {
    x.unwrap() // unwrap: outside the qualified subset
}

fn main() {
    println!("{}", factorial(5));
    println!("{}", first(vec![1, 2, 3]));
    println!("{}", half(10, 2));
    println!("{}", must_have(Some(5)));
}
