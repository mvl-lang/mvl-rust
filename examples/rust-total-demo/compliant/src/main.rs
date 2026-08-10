//! Demonstrates code that stays within rust-total's checks: no panics,
//! every recursive `#[mvl::total]` function carries a
//! `#[mvl::decreases(measure)]` whose descent the native solver proves
//! (ADR-0009), and every `while`/`loop` carries an `mvl::loop_decreases!`
//! whose descent the same solver proves (ADR-0010).

#[mvl::total]
#[mvl::decreases(n)]
fn factorial(n: u64) -> u64 {
    if n == 0 {
        1
    } else {
        n * factorial(n - 1)
    }
}

// ADR-0009 §2: `fuel - k` is a *symbolic* decrement, not a literal -- only
// provable because `#[mvl::requires(k > 0)]` gives the solver a hypothesis
// to prove `(fuel - k) < fuel` from (Fourier-Motzkin, L4). This is the
// case a fixed shape list could never recognize: nothing about `k` is
// known syntactically, only through the declared precondition.
#[mvl::total]
#[mvl::decreases(fuel)]
#[mvl::requires(k > 0)]
fn countdown(fuel: u64, k: u64) -> u64 {
    if fuel == 0 {
        0
    } else {
        countdown(fuel - k, k)
    }
}

// ADR-0010: `#[mvl::decreases(...)]` can't attach to a `while`/`loop` at
// all (a real attribute macro can't legally attach to an expression on
// stable Rust) -- `mvl::loop_decreases!` is a function-like macro instead,
// placed as the loop body's first statement. `n -= 1` is a literal
// decrement, proved the same way `factorial`'s recursive call is.
#[mvl::total]
fn sum_to(n: u64) -> u64 {
    let mut total = 0;
    let mut i = n;
    while i > 0 {
        mvl::loop_decreases!(i);
        total += i;
        i -= 1;
    }
    total
}

// The loop analogue of `countdown` above: `fuel -= k` is a symbolic
// decrement, only provable because `#[mvl::requires(k > 0)]` supplies the
// hypothesis the solver needs.
#[mvl::total]
#[mvl::requires(k > 0)]
fn countdown_loop(mut fuel: u64, k: u64) -> u64 {
    while fuel > 0 {
        mvl::loop_decreases!(fuel);
        fuel -= k;
    }
    fuel
}

enum TrafficLight {
    Red,
    Yellow,
    Green,
}

#[mvl::total]
fn next(light: &TrafficLight) -> TrafficLight {
    match light {
        TrafficLight::Red => TrafficLight::Green,
        TrafficLight::Yellow => TrafficLight::Red,
        TrafficLight::Green => TrafficLight::Yellow,
    }
}

fn main() {
    println!("5! = {}", factorial(5));
    println!("countdown(9, 3) = {}", countdown(9, 3));
    println!("sum_to(5) = {}", sum_to(5));
    println!("countdown_loop(9, 3) = {}", countdown_loop(9, 3));

    let mut light = TrafficLight::Red;
    for _ in 0..3 {
        light = next(&light);
    }
    match light {
        TrafficLight::Red => println!("red"),
        TrafficLight::Yellow => println!("yellow"),
        TrafficLight::Green => println!("green"),
    }
}
