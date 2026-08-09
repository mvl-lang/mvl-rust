//! Demonstrates code that stays within rust-total's checks: no panics, and
//! every recursive `#[mvl::total]` function carries a
//! `#[mvl::decreases(measure)]` whose descent the native solver proves
//! (ADR-0009).

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
