//! `syn`-based AST walker shared by every tool crate.
//!
//! Tool crates implement [`Walker`] and override just the hooks they need;
//! [`walk_file`] drives a single traversal over a parsed source file and
//! dispatches to those hooks, so no tool crate writes its own
//! [`syn::visit::Visit`] impl.

use syn::visit::{self, Visit};
use syn::{Expr, Field, File, ItemFn};

/// Callbacks a tool crate subscribes to during a single AST pass.
///
/// Default implementations do nothing, so a tool only overrides the hooks
/// relevant to it (e.g. `rust-total` only needs [`Walker::visit_fn`]).
pub trait Walker {
    fn visit_fn(&mut self, _item: &ItemFn) {}
    fn visit_field(&mut self, _field: &Field) {}
    fn visit_expr(&mut self, _expr: &Expr) {}
}

struct Driver<'w, W: Walker> {
    walker: &'w mut W,
}

impl<'ast, W: Walker> Visit<'ast> for Driver<'_, W> {
    fn visit_item_fn(&mut self, node: &'ast ItemFn) {
        self.walker.visit_fn(node);
        visit::visit_item_fn(self, node);
    }

    fn visit_field(&mut self, node: &'ast Field) {
        self.walker.visit_field(node);
        visit::visit_field(self, node);
    }

    fn visit_expr(&mut self, node: &'ast Expr) {
        self.walker.visit_expr(node);
        visit::visit_expr(self, node);
    }
}

/// Runs a single AST pass over `file`, dispatching to `walker`'s hooks.
pub fn walk_file(file: &File, walker: &mut impl Walker) {
    let mut driver = Driver { walker };
    driver.visit_file(file);
}
