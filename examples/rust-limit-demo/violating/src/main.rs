//! Demonstrates every construct `rust-limit` rejects. This is valid,
//! compiling Rust — it just falls outside the qualified subset `rust-limit`
//! enforces. Run `cargo mvl-limit src/main.rs` (from this crate's
//! directory, with the binary on `PATH`) to see the diagnostics.

use std::any::Any;
use std::fmt::Debug;

macro_rules! my_custom_logging_macro {
    ($msg:expr) => {
        println!("[demo] {}", $msg)
    };
}

fn describe<'a>(x: &'a i32) -> &'a i32 {
    // explicit lifetime `'a`: outside the qualified subset
    x
}

fn print_debug(x: &dyn Debug) {
    // `dyn Trait`: outside the qualified subset
    println!("{x:?}");
}

fn boxed_any(x: i32) -> Box<dyn Any> {
    // `Box<dyn Any>`: outside the qualified subset (type erasure)
    Box::new(x)
}

fn bit_pattern(x: u32) -> f32 {
    unsafe {
        // `unsafe` block, plus `transmute`: both outside the qualified subset
        std::mem::transmute(x)
    }
}

fn raw_pointer(x: &i32) -> *const i32 {
    &raw const *x // raw address-of: outside the qualified subset
}

fn main() {
    my_custom_logging_macro!("this macro isn't on the allowlist");

    let n = 5;
    println!("{}", describe(&n));
    print_debug(&n);
    let _erased = boxed_any(n);
    println!("{}", bit_pattern(0x3f800000));
    let _ptr = raw_pointer(&n);
}
