//! Typed AST for mvl-rust's attribute grammar.
//!
//! Tool crates parse plain Rust source with `syn::parse_file`, so
//! `#[refine(...)]` and friends show up as ordinary [`syn::Attribute`] nodes
//! — no proc-macro registration required. [`MvlAttr::try_from_attribute`]
//! recognizes an attribute by its path and parses its argument tokens into
//! the matching typed variant.

use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Attribute, Expr, Ident, Token};

/// `#[refine(pred)]` on a function parameter or return type.
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

/// `#[refine_ret(binder => pred)]` on a function return type.
#[derive(Debug, Clone)]
pub struct RefineRetAttr {
    pub binder: Ident,
    pub predicate: Expr,
}

impl Parse for RefineRetAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        let binder: Ident = input.parse()?;
        input.parse::<Token![=>]>()?;
        let predicate: Expr = input.parse()?;
        Ok(RefineRetAttr { binder, predicate })
    }
}

/// `#[total]` on a `fn` declaration. Carries no arguments.
#[derive(Debug, Clone, Default)]
pub struct TotalAttr;

/// `#[decreases(measure)]` alongside `#[total]` on a recursive function.
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

/// `#[effect(Console, Time, ...)]` declaring the effects a function performs.
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

/// `#[label(l)]` on a type declaration, placing it in the Denning lattice.
#[derive(Debug, Clone)]
pub struct LabelAttr {
    pub label: Ident,
}

impl Parse for LabelAttr {
    fn parse(input: ParseStream) -> syn::Result<Self> {
        Ok(LabelAttr {
            label: input.parse()?,
        })
    }
}

/// `#[declassify]` on a function that is permitted to lower an IFC label.
/// Carries no arguments.
#[derive(Debug, Clone, Default)]
pub struct DeclassifyAttr;

/// The union of all attribute kinds `mvl-rust` recognizes, tagged by which
/// one a given [`syn::Attribute`] parsed as.
#[derive(Debug, Clone)]
pub enum MvlAttr {
    Refine(RefineAttr),
    RefineRet(RefineRetAttr),
    Total(TotalAttr),
    Decreases(DecreasesAttr),
    Effect(EffectAttr),
    Label(LabelAttr),
    Declassify(DeclassifyAttr),
}

impl MvlAttr {
    /// Recognizes `attr` by its path and parses its argument tokens
    /// accordingly. Returns `None` for attributes mvl-rust doesn't own (e.g.
    /// `#[derive(...)]`) so callers can skip them without erroring; returns
    /// `Some(Err(_))` when the path matches but the argument tokens don't
    /// parse as that attribute's grammar.
    pub fn try_from_attribute(attr: &Attribute) -> Option<syn::Result<MvlAttr>> {
        let ident = attr.path().get_ident()?;
        let parsed = match ident.to_string().as_str() {
            "refine" => attr.parse_args::<RefineAttr>().map(MvlAttr::Refine),
            "refine_ret" => attr.parse_args::<RefineRetAttr>().map(MvlAttr::RefineRet),
            "total" => Ok(MvlAttr::Total(TotalAttr)),
            "decreases" => attr.parse_args::<DecreasesAttr>().map(MvlAttr::Decreases),
            "effect" => attr.parse_args::<EffectAttr>().map(MvlAttr::Effect),
            "label" => attr.parse_args::<LabelAttr>().map(MvlAttr::Label),
            "declassify" => Ok(MvlAttr::Declassify(DeclassifyAttr)),
            _ => return None,
        };
        Some(parsed)
    }
}
