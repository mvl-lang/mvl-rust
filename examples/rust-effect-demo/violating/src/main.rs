//! Demonstrates the two propagation violations `rust-effect` rejects.
//! This is valid, compiling Rust (`#[mvl::effect(...)]` is a no-op
//! pass-through) -- it just violates the effect-propagation rule. Run
//! `cargo mvl-effect src/main.rs` (with the binary on `PATH`) to see the
//! diagnostics.

#[mvl::effect(Console)]
fn log(msg: &str) {
    println!("{msg}");
}

// Pure function calling an effectful one, with no declaration at all.
fn silent_caller() {
    log("this should have been declared");
}

// Effectful function that doesn't declare all of its callee's effects.
#[mvl::effect(Console)]
fn under_declared() {
    fetch_and_log();
}

#[mvl::effect(Console, Net)]
fn fetch_and_log() {
    log("fetched");
}

#[mvl::effect(Console)]
fn main() {
    silent_caller();
    under_declared();
}
