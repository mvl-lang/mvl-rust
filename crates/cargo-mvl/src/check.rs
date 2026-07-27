//! Gate subcommand: runs every tool against source text, in dependency
//! order (limit → total → refine → effect → ifc), collecting each tool's
//! diagnostics under its own origin marker.
//!
//! All five tools are depended on as **library crates** and called
//! in-process (`rust_limit::lints::check_source`,
//! `rust_total::checks::check_source`, `rust_refine::checks::check_source`,
//! `rust_effect::checks::check_source`, `rust_ifc::checks::check_source`)
//! rather than shelled out to their separate `cargo-mvl-limit`/
//! `cargo-mvl-total`/`cargo-mvl-refine`/`cargo-mvl-effect`/`cargo-mvl-ifc`
//! binaries — each already exposes its logic as a public library
//! function, so this avoids subprocess overhead and PATH-discovery
//! entirely.

use mvl_rust_core::diagnostics::Diagnostic;

/// The five tools, in the dependency order `cargo mvl check` runs them.
pub const TOOL_ORDER: &[&str] = &["limit", "total", "refine", "effect", "ifc"];

/// Outcome of running one tool against a file.
#[derive(Debug)]
pub enum ToolOutcome {
    /// The tool ran and produced these diagnostics (may be empty).
    Ran(Vec<Diagnostic>),
    /// The tool ran but the source failed to parse.
    Error(String),
}

#[derive(Debug)]
pub struct ToolResult {
    pub tool: &'static str,
    pub outcome: ToolOutcome,
}

fn run_tool(tool: &str, source: &str) -> ToolOutcome {
    match tool {
        "limit" => match rust_limit::lints::check_source(source) {
            Ok(diagnostics) => ToolOutcome::Ran(diagnostics),
            Err(err) => ToolOutcome::Error(err.to_string()),
        },
        "total" => match rust_total::checks::check_source(source) {
            Ok(diagnostics) => ToolOutcome::Ran(diagnostics),
            Err(err) => ToolOutcome::Error(err.to_string()),
        },
        "refine" => match rust_refine::checks::check_source(source) {
            Ok(diagnostics) => ToolOutcome::Ran(diagnostics),
            Err(err) => ToolOutcome::Error(err.to_string()),
        },
        "effect" => match rust_effect::checks::check_source(source) {
            Ok(diagnostics) => ToolOutcome::Ran(diagnostics),
            Err(err) => ToolOutcome::Error(err.to_string()),
        },
        "ifc" => match rust_ifc::checks::check_source(source) {
            Ok(diagnostics) => ToolOutcome::Ran(diagnostics),
            Err(err) => ToolOutcome::Error(err.to_string()),
        },
        _ => unreachable!("run_tool called with an unrecognized tool name: {tool}"),
    }
}

/// Runs every tool in [`TOOL_ORDER`] against already-loaded source text.
pub fn check_source(source: &str) -> Vec<ToolResult> {
    TOOL_ORDER
        .iter()
        .map(|&tool| ToolResult {
            tool,
            outcome: run_tool(tool, source),
        })
        .collect()
}

/// Runs just one named tool (for `cargo mvl <tool>` passthroughs). Returns
/// `None` if `tool` isn't one of [`TOOL_ORDER`].
pub fn check_single(tool: &str, source: &str) -> Option<ToolResult> {
    let canonical: &'static str = TOOL_ORDER.iter().find(|&&t| t == tool)?;
    Some(ToolResult {
        tool: canonical,
        outcome: run_tool(canonical, source),
    })
}
