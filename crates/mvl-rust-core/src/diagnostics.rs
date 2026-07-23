//! Diagnostic emission shared by every tool crate.
//!
//! Renders through `annotate-snippets` for rustc-style caret output (source
//! snippet, underline, suggested fix) — spec Requirement 7. Tool crates run
//! as `cargo` subcommands parsing plain source text, not proc-macros, so
//! [`proc_macro2::Span::byte_range`] is always accurate here regardless of
//! toolchain (its docs note this only requires nightly *inside* an actual
//! proc-macro expansion).

use annotate_snippets::{Level as AnnotateLevel, Renderer, Snippet};
use proc_macro2::Span;

/// Severity of a diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Error,
    Warning,
    Note,
}

impl Level {
    fn to_annotate(self) -> AnnotateLevel {
        match self {
            Level::Error => AnnotateLevel::Error,
            Level::Warning => AnnotateLevel::Warning,
            Level::Note => AnnotateLevel::Note,
        }
    }
}

/// A single mvl-rust diagnostic: a headline message, the offending span, and
/// an optional concrete-fix suggestion (spec Requirement 7d).
#[derive(Debug, Clone)]
pub struct Diagnostic {
    pub level: Level,
    pub message: String,
    pub span: Span,
    pub label: Option<String>,
    pub suggestion: Option<String>,
}

impl Diagnostic {
    pub fn new(level: Level, message: impl Into<String>, span: Span) -> Self {
        Diagnostic {
            level,
            message: message.into(),
            span,
            label: None,
            suggestion: None,
        }
    }

    pub fn with_label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }

    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestion = Some(suggestion.into());
        self
    }

    /// Renders this diagnostic against `source`, the full text of the file
    /// the span was parsed from, in rustc's source-caret style.
    pub fn render(&self, source: &str, origin: &str) -> String {
        let range = self.span.byte_range();
        let label = self.label.as_deref().unwrap_or("here");
        let level = self.level.to_annotate();
        let snippet = Snippet::source(source)
            .line_start(1)
            .origin(origin)
            .fold(true)
            .annotation(level.span(range).label(label));
        let message = level.title(&self.message).snippet(snippet);

        let renderer = Renderer::plain();
        let mut out = renderer.render(message).to_string();
        if let Some(suggestion) = &self.suggestion {
            out.push_str(&format!("\nhelp: {suggestion}\n"));
        }
        out
    }

    /// Renders and prints this diagnostic to stderr.
    pub fn emit(&self, source: &str, origin: &str) {
        eprintln!("{}", self.render(source, origin));
    }
}
