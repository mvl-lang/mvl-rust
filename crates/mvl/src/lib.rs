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
//! The same grammar rule rules out an attribute on a `while`/`loop`
//! expression, so **`loop_decreases!(measure)` is a function-like macro
//! invocation, not an attribute** — the loop body's first statement, not a
//! decoration on the loop header. See its own doc for why (spec 003
//! Requirement 6, ADR-0010).
//!
//! (The proc-macro definitions themselves live in the internal `mvl-macros`
//! crate and are re-exported here — a `proc-macro = true` crate can only
//! export proc-macro items, and this crate also needs to export the
//! ordinary IFC types below, so the two are split the way
//! `tokio`/`tokio-macros` are.)
//!
//! ## IFC: `label` declares a label, `relabel` declares a transition
//!
//! Matches `mvl-lang/mvl`'s actual model (see `examples/hipaa_healthcare/ifc.mvl`
//! and `examples/log_to_file/paths.mvl`), not a single generic "trust":
//!
//! - **`#[mvl::label]`** declares a new label — a lattice point — as a
//!   zero-sized marker struct. `Labeled<L, T>` then wraps a value `T` under
//!   label `L`; unlabeled (`Public`) values need no wrapper at all.
//! - **`#[mvl::relabel(from = ..., to = ..., audit)]`** decorates a
//!   *named, directional* transition function between two labels (`_`
//!   meaning unlabeled/`Public`), mirroring MVL's
//!   `relabel NAME: From -> To [audit]` declarations. Like every other
//!   attribute here, it's a no-op marker for `rust-ifc` (not yet built) to
//!   scan for and enforce — the transition's actual body (unwrap, wrap, and
//!   any audit-logging) is ordinary code you write, the same way
//!   `#[mvl::total]` decorates a function whose body you write yourself.
//!
//! `Tainted`/`Secret` are just the two labels MVL's own `std.ifc` ships
//! built in; nothing stops a crate from declaring its own, as
//! `hipaa_healthcare` does with `PHI`:
//!
//! ```
//! let raw: mvl::Tainted<String> = mvl::Labeled::new("from the environment".to_string());
//! let trusted: String = mvl::trust(raw, "LOG-PATH-001");
//! assert_eq!(trusted, "from the environment");
//! ```
//!
//! See [`docs/overview.md`](https://github.com/mvl-lang/mvl-rust/blob/main/docs/overview.md)
//! for which checker enforces which attribute above.

pub use mvl_macros::{
    decreases, effect, ensures, label, loop_decreases, relabel, requires, total, unchecked,
};

use std::marker::PhantomData;

/// A value tagged with IFC label `L`. `L` is a zero-sized marker type
/// declared with `#[mvl::label]`; unlabeled (`Public`) values need no
/// wrapper at all — they're just the bare inner type.
pub struct Labeled<L, T> {
    value: T,
    _label: PhantomData<L>,
}

impl<L, T> Labeled<L, T> {
    /// Wrap `value` under label `L`. Called from within a `#[mvl::relabel]`
    /// transition whose `from` is `_` (ingesting/classifying plain data
    /// into this label) — e.g. `ingest_phi` below.
    pub fn new(value: T) -> Self {
        Labeled {
            value,
            _label: PhantomData,
        }
    }

    /// Unwrap the value, discarding the label. Called from within a
    /// `#[mvl::relabel]` transition whose `to` is `_` (releasing/
    /// declassifying this label) — e.g. `trust` or `hipaa_release` below.
    pub fn into_inner(self) -> T {
        self.value
    }
}

// ── Built-in labels, mirroring MVL's `std.ifc` ──────────────────────────

#[crate::label]
pub struct TaintedLabel;

#[crate::label]
pub struct SecretLabel;

pub type Tainted<T> = Labeled<TaintedLabel, T>;
pub type Secret<T> = Labeled<SecretLabel, T>;

/// Declassify `Tainted` data, matching MVL's `relabel trust(raw, tag)`
/// (`examples/log_to_file/paths.mvl`). `audit_tag` names *why* this is
/// trusted — for human auditability only; not checked at compile time here
/// (that's `rust-ifc`'s job) or consulted at runtime by this crate.
#[crate::relabel(from = "Tainted", to = "_", audit)]
pub fn trust<T>(value: Tainted<T>, _audit_tag: &'static str) -> T {
    value.into_inner()
}
