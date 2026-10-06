//! Internal results shared by document, routine and procedural leaf adapters.
use super::{Diagnostic, FormatWarning, FormattedSql, SourceRange};

#[derive(Debug, Default)]
pub(super) struct FormattedContent {
    pub output: String,
    pub diagnostics: Vec<Diagnostic>,
    pub warnings: Vec<FormatWarning>,
    pub opaque_source_ranges: Vec<SourceRange>,
    pub opaque_output_ranges: Vec<SourceRange>,
}

impl From<FormattedSql> for FormattedContent {
    fn from(formatted: FormattedSql) -> Self {
        Self {
            output: formatted.output,
            diagnostics: formatted.diagnostics,
            warnings: formatted.warnings,
            ..Self::default()
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum LeafOutcome {
    Formatted(String),
    Preserved,
}

#[derive(Debug)]
pub(super) struct FormattedLeaf {
    pub outcome: LeafOutcome,
    pub diagnostics: Vec<Diagnostic>,
}

impl From<FormattedContent> for FormattedLeaf {
    fn from(content: FormattedContent) -> Self {
        Self {
            outcome: if content.opaque_source_ranges.is_empty() {
                LeafOutcome::Formatted(content.output)
            } else {
                LeafOutcome::Preserved
            },
            diagnostics: content
                .diagnostics
                .into_iter()
                .filter(|diagnostic| !diagnostic.fix_available)
                .collect(),
        }
    }
}
