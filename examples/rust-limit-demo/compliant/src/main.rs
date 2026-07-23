//! Demonstrates code that stays within `rust-limit`'s qualified subset:
//! no `unsafe`, no `dyn Trait`, no explicit lifetimes beyond `'static`/`'_`,
//! and only allowlisted macros.

fn checked_div(numerator: i32, denominator: &i32) -> Option<i32> {
    if *denominator == 0 {
        return None;
    }
    Some(numerator / *denominator)
}

fn describe(x: &i32) -> Result<i32, &'static str> {
    match checked_div(10, x) {
        Some(v) => Ok(v),
        None => Err("division by zero"),
    }
}

fn main() {
    let inputs = vec![5, 0, 2];
    for x in &inputs {
        match describe(x) {
            Ok(v) => println!("10 / {x} = {v}"),
            Err(e) => println!("10 / {x}: {e}"),
        }
    }
    assert_eq!(describe(&5), Ok(2));
}
