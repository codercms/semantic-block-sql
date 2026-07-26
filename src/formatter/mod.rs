mod diagnostics;
mod layout_ir;
mod ownership;
mod semantic_block;
mod structure;
mod tokens;
mod validation;

use serde::Deserialize;
use thiserror::Error;

pub use validation::validate_equivalent;

pub(crate) const INDENT_WIDTH: usize = 4;

/// Byte range in the original source. Offsets are UTF-8 byte offsets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceRange {
    pub start: usize,
    pub end: usize,
}

impl SourceRange {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub const fn shifted(self, offset: usize) -> Self {
        Self {
            start: self.start + offset,
            end: self.end + offset,
        }
    }
}

/// Diagnostic severity independent from CLI exit-code policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Severity {
    Error,
    Warning,
}

/// One syntax, style, configuration, or safety diagnostic.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Diagnostic {
    pub rule_id: String,
    pub severity: Severity,
    pub message: String,
    pub source_range: SourceRange,
    pub fix_available: bool,
}

impl Diagnostic {
    pub fn shifted(mut self, offset: usize) -> Self {
        self.source_range = self.source_range.shifted(offset);
        self
    }

    pub fn with_source_range(mut self, source_range: SourceRange) -> Self {
        self.source_range = source_range;
        self
    }
}

/// Fail-safe formatting result. Failed formatting retains the original source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatResult {
    pub output: String,
    pub diagnostics: Vec<Diagnostic>,
    pub changed: bool,
}

/// Style-checking result for one complete SQL unit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckResult {
    pub diagnostics: Vec<Diagnostic>,
    pub compliant: bool,
}

/// Formatter style selected by callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Style {
    /// Semantic Block SQL.
    #[default]
    SemanticBlock,
}

/// Policy for the terminal semicolon of the formatted unit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemicolonPolicy {
    /// Preserve whether the source has a terminal semicolon.
    #[default]
    Preserve,
    /// Add a terminal semicolon when the parsed statement boundary is clear.
    Require,
    /// Remove only the terminal semicolon of the formatted unit.
    Omit,
}

/// Policy for PostgreSQL's two not-equal spellings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NotEqualPolicy {
    /// Preserve both `<>` and `!=` exactly as authored.
    #[default]
    Preserve,
    /// Normalize `<>` to `!=`.
    PreferBang,
}

/// Syntax-diagnostic capability selected by callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SyntaxDiagnostics {
    /// Surface diagnostics from the already-required PostgreSQL parser.
    #[default]
    ParserAvailable,
}

/// Stable options shared by CLI, stdin, embedded SQL, and future IDE adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatOptions {
    pub style: Style,
    pub soft_line_width: usize,
    pub hard_line_width: usize,
    pub semicolon_policy: SemicolonPolicy,
    pub not_equal_policy: NotEqualPolicy,
    pub syntax_diagnostics: SyntaxDiagnostics,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            style: Style::SemanticBlock,
            soft_line_width: 120,
            hard_line_width: 160,
            semicolon_policy: SemicolonPolicy::Preserve,
            not_equal_policy: NotEqualPolicy::Preserve,
            syntax_diagnostics: SyntaxDiagnostics::ParserAvailable,
        }
    }
}

impl FormatOptions {
    fn validate(&self) -> Result<(), FormatDiagnostic> {
        if self.soft_line_width == 0 {
            return Err(FormatDiagnostic::InvalidOptions(
                "soft_line_width must be greater than zero".into(),
            ));
        }
        if self.hard_line_width < self.soft_line_width {
            return Err(FormatDiagnostic::InvalidOptions(
                "hard_line_width must be greater than or equal to soft_line_width".into(),
            ));
        }
        Ok(())
    }
}

/// Pure formatter result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormattedSql {
    pub output: String,
    pub changed: bool,
    pub warnings: Vec<FormatWarning>,
    pub diagnostics: Vec<Diagnostic>,
}

/// Non-fatal layout condition that callers may surface to users.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormatWarning {
    /// A source token cannot be split, so its output line is necessarily wider
    /// than the configured hard limit.
    IndivisibleTokenExceedsHardWidth { line: usize, width: usize },
}

/// A formatter or safety-gate failure.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum FormatDiagnostic {
    #[error("invalid formatter options: {0}")]
    InvalidOptions(String),
    #[error("PostgreSQL parse failed: {0}")]
    PostgreSqlParse(String),
    #[error("PostgreSQL scan failed: {0}")]
    PostgreSqlScan(String),
    #[error("unsupported PostgreSQL syntax: {feature}")]
    UnsupportedSyntax {
        feature: String,
        start: usize,
        end: usize,
    },
    #[error("formatter ownership model failure: {0}")]
    Ownership(String),
    #[error("formatted SQL is not structurally equivalent to the input")]
    SemanticMismatch,
    #[error("protected token changed during formatting: {0}")]
    ProtectedTokenChanged(String),
    #[error("formatter is not idempotent")]
    NotIdempotent,
    #[error(
        "formatter left a breakable line {line} at width {width}, above hard limit {hard_limit}"
    )]
    HardLineExceeded {
        line: usize,
        width: usize,
        hard_limit: usize,
    },
}

/// Formats one or more complete PostgreSQL statements without touching files.
pub fn format_sql(source: &str, options: &FormatOptions) -> Result<FormattedSql, FormatDiagnostic> {
    options.validate()?;
    let document = validation::parse_supported_postgresql(source)?;

    let output = match options.style {
        Style::SemanticBlock => semantic_block::format(source, options, &document)?,
    };

    let output_document = validation::parse_supported_postgresql(&output)?;
    validation::validate_equivalent(source, &output)?;

    let second_pass = match options.style {
        Style::SemanticBlock => semantic_block::format(&output, options, &output_document)?,
    };
    if output != second_pass {
        return Err(FormatDiagnostic::NotIdempotent);
    }

    let warnings = semantic_block::validate_hard_width(&output, options)?;
    let mut result_diagnostics = diagnostics::style_diagnostics(source, &output, options)?;
    result_diagnostics.extend(diagnostics::warning_diagnostics(source, &warnings));

    Ok(FormattedSql {
        changed: output != source,
        output,
        warnings,
        diagnostics: result_diagnostics,
    })
}

/// Formats SQL without exposing an error-only partial-result path.
///
/// Any parse, scan, semantic-safety, idempotence, or hard-width failure returns
/// the original source unchanged together with a diagnostic.
pub fn format_sql_result(source: &str, options: &FormatOptions) -> FormatResult {
    match format_sql(source, options) {
        Ok(formatted) => FormatResult {
            output: formatted.output,
            diagnostics: formatted.diagnostics,
            changed: formatted.changed,
        },
        Err(error) => FormatResult {
            output: source.to_owned(),
            diagnostics: vec![diagnostics::failure_diagnostic(source, &error)],
            changed: false,
        },
    }
}

/// Checks mandatory formatting rules without modifying the source.
pub fn check_sql(source: &str, options: &FormatOptions) -> CheckResult {
    let formatted = format_sql_result(source, options);
    let has_error = formatted
        .diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error);
    CheckResult {
        compliant: !formatted.changed && !has_error,
        diagnostics: formatted.diagnostics,
    }
}
