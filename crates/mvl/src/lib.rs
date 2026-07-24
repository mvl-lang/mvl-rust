//! Attributes and IFC types for annotating ordinary Rust with `mvl-rust`'s
//! guarantees.
//!
//! ## Attributes
//!
//! `#[mvl::total]`, `#[mvl::requires(...)]`, `#[mvl::effect(...)]`, and
//! friends aren't real Rust syntax on their own — nothing registers them,
//! so annotated code fails to compile on stable `rustc` ("cannot find
//! attribute `total` in this scope") without this crate. Each one does
//! nothing but return the annotated item unchanged, just enough to make the
//! name resolvable. All actual verification happens separately, via the
//! `cargo mvl-*` subcommands scanning the same source with `syn` (see
//! `mvl-rust-core`, `rust-limit`) — independent of, and unaffected by,
//! whether this crate is even a dependency.
//!
//! Always invoked via a fully-qualified path (`#[mvl::total]`), never via
//! `use` — a `use mvl::total;` import reads as "extending the language,"
//! which undersells that this is meant to feel like a namespaced built-in
//! (the same idiom as `#[tokio::main]` or `#[rustfmt::skip]`), not a new
//! keyword. One `Cargo.toml` dependency line, nothing else declared.
//!
//! Attributes attach only at the function-item level, never to individual
//! parameters — Rust's grammar disallows attribute *macros* (as opposed to
//! built-in attributes) in parameter position entirely
//! ("expected non-macro attribute, found attribute macro"), so `requires`/
//! `ensures` reference parameters by their real names and `ensures` uses
//! the fixed identifier `result` for the return value.
//!
//! (The proc-macro definitions themselves live in the internal `mvl-macros`
//! crate and are re-exported here — a `proc-macro = true` crate can only
//! export proc-macro items, and this crate also needs to export the
//! ordinary IFC types below, so the two are split the way
//! `tokio`/`tokio-macros` are.)
//!
//! ## IFC: `Tainted`, `Secret`, `trust`
//!
//! Denning-lattice information flow (`Public <= Tainted <= Secret`) is
//! modeled with real wrapper types, not an attribute — unlike the markers
//! above, these carry actual data at runtime. `Public` values need no
//! wrapper; they're just the bare inner type. Declassification matches
//! MVL's own `relabel trust(value, tag)`: a real function call, named
//! `trust`, not an attribute.
//!
//! ```
//! let raw: mvl::Tainted<String> = mvl::Tainted("from the environment".to_string());
//! let trusted: String = mvl::trust(raw, "LOG-PATH-001");
//! assert_eq!(trusted, "from the environment");
//! ```

pub use mvl_macros::{decreases, effect, ensures, label, partial, refine, requires, total};

/// A value labeled above `Public` in the Denning lattice. `Public` values
/// need no wrapper — they're just the bare inner type.
pub trait Labeled {
    type Inner;
    fn declassified(self) -> Self::Inner;
}

/// Data from an untrusted source (e.g. an OS environment variable) — one
/// level above `Public` in the lattice.
pub struct Tainted<T>(pub T);

impl<T> Labeled for Tainted<T> {
    type Inner = T;
    fn declassified(self) -> T {
        self.0
    }
}

/// Data that must never flow to a `Public` sink without explicit
/// declassification — the top of the lattice.
pub struct Secret<T>(pub T);

impl<T> Labeled for Secret<T> {
    type Inner = T;
    fn declassified(self) -> T {
        self.0
    }
}

/// Explicit declassification, matching MVL's `relabel trust(value, tag)`.
/// `audit_tag` names *why* this declassification is trusted (e.g. an
/// audit-log tag) — it exists purely for human auditability. It isn't
/// checked at compile time by this crate (that's `rust-ifc`'s job) or
/// consulted at runtime.
pub fn trust<L: Labeled>(value: L, _audit_tag: &'static str) -> L::Inner {
    value.declassified()
}
