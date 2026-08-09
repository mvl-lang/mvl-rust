//! The same behavior as `../idiomatic/`, contracted into `rust-limit`'s
//! qualified subset: `dyn Trait` becomes a closed `enum` + `match`. Every
//! call site now has a single concrete signature -- which is exactly what
//! buys back verification. `rectangle_perimeter`/`square_perimeter` below
//! carry a `#[mvl::requires]`/`#[mvl::ensures]` pair that `rust-refine`
//! discharges natively (linear arithmetic, no external solver) -- something
//! no `dyn Shape` call site could ever offer a `requires` to check against,
//! per ADR-0002 rule 2.

enum Shape {
    Rectangle { width: i32, height: i32 },
    Square { side: i32 },
}

#[mvl::requires(width > 0 && height > 0)]
#[mvl::ensures(result > 0)]
fn rectangle_perimeter(width: i32, height: i32) -> i32 {
    2 * (width + height)
}

#[mvl::requires(side > 0)]
#[mvl::ensures(result > 0)]
fn square_perimeter(side: i32) -> i32 {
    4 * side
}

fn perimeter(shape: &Shape) -> i32 {
    match shape {
        Shape::Rectangle { width, height } => rectangle_perimeter(*width, *height),
        Shape::Square { side } => square_perimeter(*side),
    }
}

fn total_perimeter(shapes: &[Shape]) -> i32 {
    shapes.iter().map(perimeter).sum()
}

fn main() {
    let shapes = vec![
        Shape::Rectangle {
            width: 3,
            height: 4,
        },
        Shape::Square { side: 5 },
    ];
    let total = total_perimeter(&shapes);
    println!("total perimeter = {total}");

    // Bound through `let` rather than passed straight to `println!` -- a
    // call inside a macro invocation is invisible to `syn`, so it would
    // produce no call-site obligation at all (see rust-refine-demo). With
    // literal arguments, `rust-refine` closes both the `requires` (3 > 0 &&
    // 4 > 0) and the `ensures` (2 * (3 + 4) > 0) at L1 -- a proof `dyn
    // Shape` dispatch has no signature to attach to.
    let direct = rectangle_perimeter(3, 4);
    println!("rectangle_perimeter(3, 4) = {direct}");
}
