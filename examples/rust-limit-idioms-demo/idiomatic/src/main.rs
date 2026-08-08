//! The common Rust fragment `rust-limit` rejects: polymorphism via
//! `dyn Trait`. This is idiomatic, valid, compiling Rust -- the textbook
//! way to model "a shape is one of several kinds" -- and it sits outside
//! the qualified subset (ADR-0002 rule 2). Run `cargo mvl-limit
//! src/main.rs` (with the binary on `PATH`) to see it rejected.
//!
//! See `../contracted/` for the same behavior rewritten inside the subset.

trait Shape {
    fn perimeter(&self) -> i32;
}

struct Rectangle {
    width: i32,
    height: i32,
}

impl Shape for Rectangle {
    fn perimeter(&self) -> i32 {
        2 * (self.width + self.height)
    }
}

struct Square {
    side: i32,
}

impl Shape for Square {
    fn perimeter(&self) -> i32 {
        4 * self.side
    }
}

fn total_perimeter(shapes: &[Box<dyn Shape>]) -> i32 {
    // `Box<dyn Shape>`: type erasure. `rust-refine` can't attach an
    // obligation to this call site -- it has no way to know which
    // `perimeter` impl runs, so there is no single concrete signature to
    // check a `requires`/`ensures` against.
    shapes.iter().map(|s| s.perimeter()).sum()
}

fn main() {
    let shapes: Vec<Box<dyn Shape>> = vec![
        Box::new(Rectangle {
            width: 3,
            height: 4,
        }),
        Box::new(Square { side: 5 }),
    ];
    println!("total perimeter = {}", total_perimeter(&shapes));
}
