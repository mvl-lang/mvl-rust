//! Demonstrates `rust-ifc`'s check: a `Labeled` value's label can only be
//! stripped/attached inside a function whose own
//! `#[mvl::relabel(from = ..., to = ...)]` attribute declares exactly
//! that transition.

#[mvl::relabel(from = "_", to = "Tainted", audit)]
fn ingest(raw: String) -> mvl::Tainted<String> {
    mvl::Tainted::new(raw)
}

#[mvl::relabel(from = "Tainted", to = "_", audit)]
fn trust<T>(value: mvl::Tainted<T>, _audit_tag: &'static str) -> T {
    value.into_inner()
}

fn main() {
    let raw = ingest("from the environment".to_string());
    let trusted = trust(raw, "LOG-PATH-001");
    println!("{trusted}");
}
