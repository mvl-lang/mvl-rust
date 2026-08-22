//! `impl`-method collection shared by every annotation-consuming tool
//! (`rust-refine`, `rust-total`, `rust-effect`) that needs to see a
//! method's own attributes/signature/body, not just a free function's
//! (ADR-0001's "largest practical coverage gap": methods were previously
//! invisible to every one of them end to end).
//!
//! Deliberately *not* part of [`crate::walker`] — that trait dispatches a
//! single shared traversal, but every tool here still builds its own
//! per-check `syn::visit::Visit` pass over free functions (ADR-0001 §3,
//! "no shared program representation"); this is just the one piece of
//! bookkeeping (finding methods, naming them collision-safely) that three
//! separate tools would otherwise triplicate.

use syn::{File, ImplItem, ImplItemFn, Item, ItemImpl, Type};

/// The simple type name of an `impl` block's `Self` type (`impl Foo { .. }`
/// -> `"Foo"`), used to qualify a method's name as `"Type::method"` so it
/// can't collide with a free function or another impl's identically named
/// method. `None` for any `Self` type shape other than a plain path (its
/// last segment's ident is taken, so `impl<T> Vec<T>` still resolves to
/// `"Vec"`) -- conservative, matching every tool's syn-only, no-type-info
/// scope.
pub fn impl_self_type_name(item_impl: &ItemImpl) -> Option<String> {
    match &*item_impl.self_ty {
        Type::Path(type_path) => type_path
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string()),
        _ => None,
    }
}

/// Every method in every `impl` block in the file, paired with its
/// qualified `"Type::method"` name. Covers both inherent and trait impls
/// identically; call resolution *into* a method is a separate, tool-owned
/// decision (every current caller keeps that same-file/free-function-only
/// boundary) -- this only ever hands back what to check the method's own
/// contract against, never a new call-site obligation at its call
/// expressions.
pub fn impl_methods(file: &File) -> Vec<(String, &ImplItemFn)> {
    let mut methods = Vec::new();
    for item in &file.items {
        let Item::Impl(item_impl) = item else {
            continue;
        };
        let Some(type_name) = impl_self_type_name(item_impl) else {
            continue;
        };
        for impl_item in &item_impl.items {
            if let ImplItem::Fn(method) = impl_item {
                methods.push((format!("{type_name}::{}", method.sig.ident), method));
            }
        }
    }
    methods
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn collects_a_method_qualified_by_its_impl_type() {
        let file: File =
            syn::parse_str("struct Foo;\nimpl Foo {\n    fn bar(&self) -> i32 { 0 }\n}").unwrap();
        let methods = impl_methods(&file);
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].0, "Foo::bar");
    }

    #[test]
    fn a_free_function_is_not_a_method() {
        let file: File = syn::parse_str("fn bar() -> i32 { 0 }").unwrap();
        assert!(impl_methods(&file).is_empty());
    }

    #[test]
    fn a_generic_impl_still_resolves_to_the_bare_type_name() {
        let file: File =
            syn::parse_str("struct Wrapper<T>(T);\nimpl<T> Wrapper<T> {\n    fn get(&self) {}\n}")
                .unwrap();
        let methods = impl_methods(&file);
        assert_eq!(methods[0].0, "Wrapper::get");
    }

    #[test]
    fn a_trait_impl_method_is_collected_too() {
        let file: File = syn::parse_str(
            "struct Foo;\ntrait T { fn bar(&self) -> i32; }\nimpl T for Foo {\n    fn bar(&self) -> i32 { 0 }\n}",
        )
        .unwrap();
        let methods = impl_methods(&file);
        assert_eq!(methods.len(), 1);
        assert_eq!(methods[0].0, "Foo::bar");
    }
}
