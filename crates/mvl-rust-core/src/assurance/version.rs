//! The assurance-JSON schema's own version, independent of any crate's
//! version. Bump this whenever [`super::schema::AssuranceReport`]'s shape
//! changes in a way that could break an existing consumer — the
//! `tests/schema_stability.rs` snapshot test fails on any shape drift
//! precisely so this bump doesn't get missed.
pub const ASSURANCE_SCHEMA_VERSION: &str = "1.0";
