//! Demonstrates the two IFC violations `rust-ifc` rejects: stripping a
//! label with no `relabel` attribute at all, and stripping it with a
//! `relabel` attribute that names the wrong label. This is valid,
//! compiling Rust (`Labeled::into_inner`/`::new` are ordinary public
//! methods; `#[mvl::relabel]` is a no-op pass-through) -- it just
//! bypasses the intended declassification boundary. Run
//! `cargo mvl-ifc src/main.rs` (with the binary on `PATH`) to see the
//! diagnostics.

#[mvl::relabel(from = "_", to = "Tainted", audit)]
fn ingest(raw: String) -> mvl::Tainted<String> {
    mvl::Tainted::new(raw)
}

// No `#[mvl::relabel]` at all -- strips the label with zero audit trail.
fn leak(value: mvl::Tainted<String>) -> String {
    value.into_inner()
}

// Has a `relabel`, but it names the wrong `from` label.
#[mvl::relabel(from = "Secret", to = "_", audit)]
fn mislabeled_release(value: mvl::Tainted<String>) -> String {
    value.into_inner()
}

fn main() {
    println!("{}", leak(ingest("from the environment".to_string())));
    println!("{}", mislabeled_release(ingest("also tainted".to_string())));
}
