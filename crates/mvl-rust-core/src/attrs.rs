//! Typed AST for mvl-rust's attribute grammar.
//!
//! Tool crates parse plain Rust source with `syn::parse_file`, so
//! `#[mvl::refine(...)]` and friends show up as ordinary [`syn::Attribute`]
//! nodes — no proc-macro registration required. [`MvlAttr::try_from_attribute`]
//! recognizes an attribute by its *last* path segment (so both the bare
//! `#[total]` form and the real, always-qualified `#[mvl::total]` form —
//! see the `mvl` crate — resolve the same way) and parses its argument
//! tokens into the matching typed variant.

use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Attribute, Expr, Ident, LitStr, Token};

mod predicate;
pub use predicate::Predicate;

/// `#[mvl::refine(pred)]` — a whole-function precondition.
#[derive(Debug, Clone)]
pub struct RefineAttr {
    pub predicate: Expr,
}

impl Parse for RefineAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(RefineAttr {
            predicate: input.parse()?,
        })
    }
}

/// `#[mvl::total]` on a `fn` declaration. Carries no arguments.
#[derive(Debug, Clone, Default)]
pub struct TotalAttr;

/// `#[mvl::partial]` on a `fn` declaration. Carries no arguments.
#[derive(Debug, Clone, Default)]
pub struct PartialAttr;

/// `#[mvl::decreases(measure)]` alongside `#[mvl::total]` on a recursive
/// function.
#[derive(Debug, Clone)]
pub struct DecreasesAttr {
    pub measure: Expr,
}

impl Parse for DecreasesAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(DecreasesAttr {
            measure: input.parse()?,
        })
    }
}

/// `#[mvl::effect(Console, Time, ...)]` declaring the effects a function
/// performs.
#[derive(Debug, Clone, Default)]
pub struct EffectAttr {
    pub effects: Vec<Ident>,
}

impl Parse for EffectAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let effects = Punctuated::<Ident, Token![,]>::parse_terminated(input)?;
        Ok(EffectAttr {
            effects: effects.into_iter().collect(),
        })
    }
}

/// `#[mvl::requires(pred)]` — a whole-function precondition, referencing
/// parameters by their real names. `pred` is a [`Predicate`]: a plain
/// Rust boolean/comparison expression, or a bounded quantifier
/// (`forall`/`exists i in [lo..hi]. pred`).
#[derive(Debug, Clone)]
pub struct RequiresAttr {
    pub predicate: Predicate,
}

impl Parse for RequiresAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(RequiresAttr {
            predicate: input.parse()?,
        })
    }
}

/// `#[mvl::ensures(pred)]` — a whole-function postcondition; `pred`
/// conventionally references the fixed identifier `result`. Same
/// [`Predicate`] grammar as [`RequiresAttr`].
#[derive(Debug, Clone)]
pub struct EnsuresAttr {
    pub predicate: Predicate,
}

impl Parse for EnsuresAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(EnsuresAttr {
            predicate: input.parse()?,
        })
    }
}

/// `#[mvl::label]` declaring a new IFC label (lattice point). Carries no
/// arguments — it decorates the label's own marker type.
#[derive(Debug, Clone, Default)]
pub struct LabelAttr;

/// `#[mvl::relabel(from = "...", to = "...", audit)]` declaring a named,
/// directional IFC label transition. `from`/`to` name labels (`"_"` means
/// unlabeled/`Public`); `audit` is a bare, optional flag.
#[derive(Debug, Clone)]
pub struct RelabelAttr {
    pub from: LitStr,
    pub to: LitStr,
    pub audit: bool,
}

impl Parse for RelabelAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let mut from: Option<LitStr> = None;
        let mut to: Option<LitStr> = None;
        let mut audit = false;

        let items = Punctuated::<RelabelItem, Token![,]>::parse_terminated(input)?;
        for item in items {
            match item {
                RelabelItem::From(lit) => from = Some(lit),
                RelabelItem::To(lit) => to = Some(lit),
                RelabelItem::Audit => audit = true,
            }
        }

        Ok(RelabelAttr {
            from: from.ok_or_else(|| input.error("expected `from = \"...\"`"))?,
            to: to.ok_or_else(|| input.error("expected `to = \"...\"`"))?,
            audit,
        })
    }
}

enum RelabelItem {
    From(LitStr),
    To(LitStr),
    Audit,
}

impl Parse for RelabelItem {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let ident: Ident = input.parse()?;
        match ident.to_string().as_str() {
            "from" => {
                input.parse::<Token![=]>()?;
                Ok(RelabelItem::From(input.parse()?))
            }
            "to" => {
                input.parse::<Token![=]>()?;
                Ok(RelabelItem::To(input.parse()?))
            }
            "audit" => Ok(RelabelItem::Audit),
            other => Err(syn::Error::new(
                ident.span(),
                format!("unknown `relabel` key `{other}`, expected `from`, `to`, or `audit`"),
            )),
        }
    }
}

/// The union of all attribute kinds `mvl-rust` recognizes, tagged by which
/// one a given [`syn::Attribute`] parsed as.
#[derive(Debug, Clone)]
pub enum MvlAttr {
    Refine(RefineAttr),
    Total(TotalAttr),
    Partial(PartialAttr),
    Decreases(DecreasesAttr),
    Effect(EffectAttr),
    Requires(RequiresAttr),
    Ensures(EnsuresAttr),
    Label(LabelAttr),
    Relabel(RelabelAttr),
}

impl MvlAttr {
    /// Recognizes `attr` by its last path segment and parses its argument
    /// tokens accordingly. Returns `None` for attributes mvl-rust doesn't
    /// own (e.g. `#[derive(...)]`) so callers can skip them without
    /// erroring; returns `Some(Err(_))` when the path matches but the
    /// argument tokens don't parse as that attribute's grammar.
    pub fn try_from_attribute(attr: &Attribute) -> Option<syn::Result<MvlAttr>> {
        let last = attr.path().segments.last()?;
        let parsed = match last.ident.to_string().as_str() {
            "refine" => attr.parse_args::<RefineAttr>().map(MvlAttr::Refine),
            "total" => Ok(MvlAttr::Total(TotalAttr)),
            "partial" => Ok(MvlAttr::Partial(PartialAttr)),
            "decreases" => attr.parse_args::<DecreasesAttr>().map(MvlAttr::Decreases),
            "effect" => attr.parse_args::<EffectAttr>().map(MvlAttr::Effect),
            "requires" => attr.parse_args::<RequiresAttr>().map(MvlAttr::Requires),
            "ensures" => attr.parse_args::<EnsuresAttr>().map(MvlAttr::Ensures),
            "label" => Ok(MvlAttr::Label(LabelAttr)),
            "relabel" => attr.parse_args::<RelabelAttr>().map(MvlAttr::Relabel),
            _ => return None,
        };
        Some(parsed)
    }
}
