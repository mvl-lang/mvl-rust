//! Demonstrates code that stays within rust-total's checks: no panics, and
//! the one recursive `#[mvl::total]` function carries a
//! `#[mvl::decreases(measure)]` that strictly decreases.

#[mvl::total]
#[mvl::decreases(n)]
fn factorial(n: u64) -> u64 {
    if n == 0 {
        1
    } else {
        n * factorial(n - 1)
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
