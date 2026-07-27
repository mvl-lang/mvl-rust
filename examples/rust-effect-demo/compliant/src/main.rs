//! Demonstrates `rust-effect`'s propagation check: a caller must declare
//! every effect its same-file callees declare, and a pure function must
//! not call an effectful one.

#[mvl::effect(Console)]
fn log(msg: &str) {
    println!("{msg}");
}

fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[mvl::effect(Console)]
fn report(a: i32, b: i32) {
    log(&format!("{a} + {b} = {}", add(a, b)));
}

#[mvl::effect(Console)]
fn main() {
    report(2, 3);
}
