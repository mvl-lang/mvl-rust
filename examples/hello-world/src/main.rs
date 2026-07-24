// Simplest possible mvl-rust program.
// Exercises: rust-limit qualified subset (Req 1), totality (Req 2),
// effect tracking (Req 4).
//
// #[mvl::total] and #[mvl::effect(Console)] resolve via the `mvl`
// pass-through crate (#21) -- rust-total and rust-effect don't verify
// them yet (not implemented), but the annotation itself compiles as
// ordinary Rust, which is the whole point of the attribute-based story.
// Invoked via fully-qualified path (`mvl::...`), never `use` -- that's
// the coding pattern: one Cargo.toml dependency line, nothing else
// declared, reads like a namespaced built-in (same idiom as
// #[tokio::main]), not a language extension.
//
// Expected stdout:
//   Hello, world!

#[mvl::total]
#[mvl::effect(Console)]
fn main() {
    println!("Hello, world!");
}
