//! Demonstrates every construct `rust-total` rejects. This is valid,
//! compiling Rust — it just falls outside what `#[mvl::total]` permits.
//! Run `cargo mvl-total src/main.rs` (with the binary on `PATH`) to see
//! the diagnostics.
//!
//! Note: a genuinely non-exhaustive match (no wildcard, missing variants)
//! is already a hard `rustc` compile error on its own — `rust-total`
//! doesn't need to add anything there, so it isn't demonstrated here (it
//! would just fail `cargo build`, not `cargo mvl-total`).
//!
//! `cargo-mvl-total`'s diagnostics come from a static scan of the source
//! text, not execution, so `main` doesn't need to call every function
//! below to demonstrate its rejection. Several genuinely never terminate
//! at runtime (that's the point being demonstrated) and are marked
//! `#[allow(dead_code)]` instead of called, so this crate still builds and
//! runs cleanly rather than hanging or stack-overflowing if someone
//! actually executes it.

#[mvl::total]
fn factorial(n: u64) -> u64 {
    // missing #[mvl::decreases(n)]: outside the qualified subset
    if n == 0 {
        1
    } else {
        n * factorial(n - 1)
    }
}

// ADR-0009: presence is no longer enough. `n` is passed unchanged on the
// recursive call, so the measure never decreases -- ADR-0009 rejects this
// rather than accepting it on presence alone. Genuinely never terminates
// if called (n >= 10 never becomes true), so it isn't -- see module doc.
#[allow(dead_code)]
#[mvl::total]
#[mvl::decreases(n)]
fn count_up(n: u64) -> u64 {
    if n >= 10 {
        n
    } else {
        count_up(n)
    }
}

// ADR-0009 §5: `n` is rebound before the recursive call, so the `n` in
// `shadowed_measure(n - 1)` means the *shadowed local* (original n + 100),
// not the parameter -- the function never terminates (each call's argument
// is strictly larger than the last), which is exactly why a shadowed
// measure is rejected outright regardless of how the recursive call reads.
#[allow(dead_code)]
#[mvl::total]
#[mvl::decreases(n)]
fn shadowed_measure(n: u64) -> u64 {
    let n = n + 100;
    if n == 0 {
        0
    } else {
        shadowed_measure(n - 1)
    }
}

// ADR-0009 §2: `fuel - k` is a symbolic decrement -- provable given a
// `#[mvl::requires(k > 0)]` bound (see the compliant crate's `countdown`),
// but not without one: the solver cannot rule out `k <= 0`, under which
// `fuel - k` would not decrease `fuel` (or would underflow). Never
// terminates for e.g. k == 0, so it isn't called -- see module doc.
#[allow(dead_code)]
#[mvl::total]
#[mvl::decreases(fuel)]
fn unbounded_countdown(fuel: u64, k: u64) -> u64 {
    if fuel == 0 {
        0
    } else {
        unbounded_countdown(fuel - k, k)
    }
}

// ADR-0009 §2: division is outside the native solver's linear-arithmetic
// system entirely -- `(n / 2) < n` is `Runtime` (unprovable) regardless of
// hypotheses, confirmed empirically even against `n > 0`. Unlike the other
// rejected-but-uncallable functions above, this one actually terminates at
// runtime (integer division reaches 0), so it's safe to call -- the point
// is that `rust-total` can't *prove* it does.
#[mvl::total]
#[mvl::decreases(n)]
fn halve(n: u64) -> u64 {
    if n == 0 {
        0
    } else {
        halve(n / 2)
    }
}

// ADR-0010: the headline gap this section closes. `termination.rs` only
// ever looked at recursive calls -- this genuinely never terminates
// (`n += 1` forever) and was accepted with zero diagnostics before the
// loop-termination check existed. Uncallable -- see module doc.
#[allow(dead_code, unused_assignments, unused_variables)]
#[mvl::total]
fn spins_forever() -> u64 {
    let mut n = 0;
    loop {
        n += 1;
    }
}

// ADR-0010 §4: same class of bug as `shadowed_measure` above, for a loop.
// `n -= 1` inside the shadowing block mutates the fresh local, never the
// outer loop variable, so the outer `n > 0` condition never changes --
// which is also exactly why rustc's own `unused_mut` fires on the outer
// parameter below: it genuinely is never mutated, silent evidence of the
// same bug this function exists to demonstrate.
#[allow(dead_code, unused_mut, unused_assignments, unused_variables)]
#[mvl::total]
fn shadowed_loop_measure(mut n: u64) -> u64 {
    while n > 0 {
        mvl::loop_decreases!(n);
        let mut n = n + 100;
        n -= 1;
    }
    n
}

// The loop analogue of `unbounded_countdown` above: `fuel -= k` without a
// `#[mvl::requires(k > 0)]` bound (see the compliant crate's
// `countdown_loop`) -- the solver can't rule out `k == 0`. Never
// terminates for e.g. k == 0, so it isn't called -- see module doc.
#[allow(dead_code)]
#[mvl::total]
fn unbounded_countdown_loop(mut fuel: u64, k: u64) -> u64 {
    while fuel > 0 {
        mvl::loop_decreases!(fuel);
        fuel -= k;
    }
    fuel
}

// The loop analogue of `halve` above: genuinely terminates at runtime
// (integer division reaches 0), but division is outside the native
// solver's linear-arithmetic system entirely, so it's never provable --
// safe to call, since it does actually terminate.
#[mvl::total]
fn halve_loop(mut n: u64) -> u64 {
    while n > 0 {
        mvl::loop_decreases!(n);
        n /= 2;
    }
    n
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

// ADR-0012: orchestration, not itself a totality claim this demo is making.
#[mvl::partial]
fn main() {
    println!("{}", factorial(5));
    println!("{}", halve(100));
    println!("{}", halve_loop(100));
    println!("{}", first(vec![1, 2, 3]));
    println!("{}", half(10, 2));
    println!("{}", must_have(Some(5)));
}
