//! Predicate mini-language for `#[mvl::requires]`/`#[mvl::ensures]`:
//! either a plain Rust boolean/comparison expression, or a bounded
//! quantifier over a literal integer range.
//!
//! Grammar (matches `mvl-lang/mvl`'s real, accepted implementation —
//! ADR-0056, confirmed directly against `src/mvl/checker/refinements.rs`
//! and its test fixtures under `tests/solver/layer3/`, not just the ADR's
//! own prose):
//!
//! ```text
//! predicate := expr
//!            | "forall" IDENT "in" "[" int_lit ".." int_lit "]" "." predicate
//!            | "exists" IDENT "in" "[" int_lit ".." int_lit "]" "." predicate
//! ```
//!
//! Not valid Rust expression syntax on its own — `forall`/`exists`/`in`
//! aren't a quantifier form Rust's grammar has, so this needs a dedicated
//! parser rather than reusing `syn::Expr` wholesale (attribute-argument
//! tokens only need to *tokenize* as Rust, not parse as a valid
//! expression). The bracketed range is parsed as a real `syn::ExprArray`
//! specifically (not a generic `syn::Expr`) — parsing it as a generic
//! `Expr` would greedily consume postfix continuation past the closing
//! bracket (e.g. `[1..50].sections.get(i)` parses as one combined
//! `Expr::MethodCall` on the array literal), confirmed empirically before
//! writing this. Both range endpoints must be literal integers (optionally
//! negated) — non-literal endpoints (`[0..len-1]`) aren't supported yet,
//! matching `mvl-lang/mvl`'s own current limitation, not a reduction
//! unique to this port.

use syn::parse::{Parse, ParseStream};
use syn::{Expr, ExprLit, ExprUnary, Ident, Lit, Token, UnOp};

/// A `requires`/`ensures` predicate.
#[derive(Debug, Clone)]
pub enum Predicate {
    Expr(Expr),
    Forall {
        var: Ident,
        lo: i64,
        hi: i64,
        body: Box<Predicate>,
    },
    Exists {
        var: Ident,
        lo: i64,
        hi: i64,
        body: Box<Predicate>,
    },
}

impl Predicate {
    /// Renders back to source text — used for the assurance-JSON
    /// `predicate: String` field, and to round-trip through
    /// `solver::Obligation::predicate` (re-parsed via this same type's
    /// `Parse` impl, so this must stay parseable by it).
    pub fn render(&self) -> String {
        match self {
            Predicate::Expr(expr) => quote::quote!(#expr).to_string(),
            Predicate::Forall { var, lo, hi, body } => {
                format!("forall {var} in [{lo}..{hi}] . {}", body.render())
            }
            Predicate::Exists { var, lo, hi, body } => {
                format!("exists {var} in [{lo}..{hi}] . {}", body.render())
            }
        }
    }
}

impl Parse for Predicate {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        if let Some(quantifier) = peek_quantifier_keyword(input) {
            return parse_quantifier(input, quantifier);
        }
        Ok(Predicate::Expr(input.parse()?))
    }
}

#[derive(Clone, Copy)]
enum Quantifier {
    Forall,
    Exists,
}

/// `forall`/`exists` are contextual, not real Rust keywords — peek via a
/// fork so a predicate that's just an ordinary expression starting with
/// an identifier (e.g. a function call `forall_helper(x)`) isn't
/// misdetected. Only an exact, standalone `forall`/`exists` ident
/// triggers the quantifier form.
fn peek_quantifier_keyword(input: ParseStream) -> Option<Quantifier> {
    let fork = input.fork();
    let ident: Ident = fork.parse().ok()?;
    match ident.to_string().as_str() {
        "forall" => Some(Quantifier::Forall),
        "exists" => Some(Quantifier::Exists),
        _ => None,
    }
}

fn parse_quantifier(input: ParseStream, quantifier: Quantifier) -> syn::Result<Predicate> {
    input.parse::<Ident>()?; // consume "forall"/"exists" itself
    let var: Ident = input.parse()?;
    input.parse::<Token![in]>()?;

    // Parsed as `ExprArray` specifically, not a generic `Expr` -- see the
    // module doc comment for why that distinction matters here.
    let array: syn::ExprArray = input.parse()?;
    if array.elems.len() != 1 {
        return Err(syn::Error::new_spanned(
            &array,
            "expected a single bounded range `[lo..hi]`",
        ));
    }
    let Expr::Range(range) = &array.elems[0] else {
        return Err(syn::Error::new_spanned(
            &array.elems[0],
            "expected a range `lo..hi` with literal integer endpoints",
        ));
    };
    let start = range.start.as_deref().ok_or_else(|| {
        syn::Error::new_spanned(range, "quantifier range needs an explicit lower bound")
    })?;
    let end = range.end.as_deref().ok_or_else(|| {
        syn::Error::new_spanned(range, "quantifier range needs an explicit upper bound")
    })?;
    let lo = literal_i64(start)?;
    let hi = literal_i64(end)?;

    input.parse::<Token![.]>()?;
    let body = Box::new(Predicate::parse(input)?);

    Ok(match quantifier {
        Quantifier::Forall => Predicate::Forall { var, lo, hi, body },
        Quantifier::Exists => Predicate::Exists { var, lo, hi, body },
    })
}

/// A literal integer, including a leading unary `-` (`-5` parses as
/// `Expr::Unary(Neg, Expr::Lit(5))`, not as a single negative literal
/// token) -- same handling as the interval solver's own `int_value`.
fn literal_i64(expr: &Expr) -> syn::Result<i64> {
    match expr {
        Expr::Lit(ExprLit {
            lit: Lit::Int(int), ..
        }) => int.base10_parse::<i64>(),
        Expr::Unary(ExprUnary {
            op: UnOp::Neg(_),
            expr,
            ..
        }) => literal_i64(expr).map(|v| -v),
        _ => Err(syn::Error::new_spanned(
            expr,
            "quantifier range endpoints must be literal integers (non-literal endpoints aren't \
             supported yet, matching mvl-lang/mvl's own current limitation)",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_expr_predicate_parses() {
        let pred: Predicate = syn::parse_str("x >= 0 && x < 100").unwrap();
        assert!(matches!(pred, Predicate::Expr(_)));
    }

    #[test]
    fn forall_over_literal_range_parses() {
        let pred: Predicate = syn::parse_str("forall i in [1..50] . i >= 0").unwrap();
        match pred {
            Predicate::Forall { var, lo, hi, .. } => {
                assert_eq!(var, "i");
                assert_eq!(lo, 1);
                assert_eq!(hi, 50);
            }
            other => panic!("expected Forall, got {other:?}"),
        }
    }

    #[test]
    fn exists_over_literal_range_parses() {
        let pred: Predicate = syn::parse_str("exists i in [0..9] . i == 5").unwrap();
        assert!(matches!(pred, Predicate::Exists { .. }));
    }

    #[test]
    fn negative_bounds_parse() {
        let pred: Predicate = syn::parse_str("forall i in [-5..5] . i >= -5").unwrap();
        match pred {
            Predicate::Forall { lo, hi, .. } => {
                assert_eq!(lo, -5);
                assert_eq!(hi, 5);
            }
            other => panic!("expected Forall, got {other:?}"),
        }
    }

    #[test]
    fn body_referencing_the_bound_var_via_a_method_call_parses_correctly() {
        // Regression guard: parsing the range as a generic `Expr` would
        // greedily consume this trailing method-call chain into the
        // range expression itself.
        let pred: Predicate =
            syn::parse_str("forall i in [1..50] . sections.get(i) != None").unwrap();
        match pred {
            Predicate::Forall { lo, hi, body, .. } => {
                assert_eq!((lo, hi), (1, 50));
                assert!(matches!(*body, Predicate::Expr(_)));
            }
            other => panic!("expected Forall, got {other:?}"),
        }
    }

    #[test]
    fn non_literal_endpoint_is_a_parse_error() {
        assert!(syn::parse_str::<Predicate>("forall i in [0..n] . i >= 0").is_err());
    }

    #[test]
    fn render_round_trips_through_parse() {
        let original: Predicate =
            syn::parse_str("forall i in [1..50] . i >= 0 && i <= 50").unwrap();
        let rendered = original.render();
        let reparsed: Predicate = syn::parse_str(&rendered).unwrap();
        match (original, reparsed) {
            (
                Predicate::Forall {
                    lo: lo1, hi: hi1, ..
                },
                Predicate::Forall {
                    lo: lo2, hi: hi2, ..
                },
            ) => {
                assert_eq!((lo1, hi1), (lo2, hi2));
            }
            _ => panic!("round-trip changed predicate shape"),
        }
    }

    #[test]
    fn nested_quantifiers_parse() {
        let pred: Predicate =
            syn::parse_str("forall i in [0..2] . exists j in [0..2] . i != j").unwrap();
        match pred {
            Predicate::Forall { body, .. } => {
                assert!(matches!(*body, Predicate::Exists { .. }));
            }
            other => panic!("expected Forall, got {other:?}"),
        }
    }

    #[test]
    fn an_ordinary_call_named_forall_is_not_misdetected_as_a_quantifier() {
        let pred: Predicate = syn::parse_str("forall_helper(x)").unwrap();
        assert!(matches!(pred, Predicate::Expr(_)));
    }
}
