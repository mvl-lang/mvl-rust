//! `#[mvl::requires]`/`#[mvl::ensures]` refinement obligations for
//! mvl-rust, discharged via the native `L1`+`L2` solver backend
//! (ADR-0005). See [`checks`] for the scanning/discharge logic.

pub mod checks;
