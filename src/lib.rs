//! Reusable Semantic Block SQL formatting engine.
//!
//! The library never writes files. CLI, host-language extraction, and editor
//! adapters all call the same [`format_sql`] facade.

pub mod config;
pub mod diff;
pub mod directives;
pub mod discover;
pub mod git;
pub mod host;
pub mod rewrite;
pub mod source;

mod formatter;

pub use formatter::{
    CheckResult, Diagnostic, FormatDiagnostic, FormatOptions, FormatResult, FormatWarning,
    FormattedSql, NotEqualPolicy, SemicolonPolicy, Severity, SourceRange, Style, SyntaxDiagnostics,
    TypeAliasFamily, UnsupportedPolicy, check_sql, format_sql, format_sql_result,
    validate_equivalent,
};
