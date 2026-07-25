mod semantic_block;
mod tokens;
mod validation;

use thiserror::Error;

pub use validation::validate_equivalent;

/// Formatter style selected by callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Style {
    /// Semantic Block SQL.
    #[default]
    SemanticBlock,
}

/// Stable options shared by CLI, stdin, embedded SQL, and future IDE adapters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatOptions {
    pub style: Style,
    pub indent_width: usize,
    pub soft_line_width: usize,
    pub hard_line_width: usize,
    pub preserve_list_groups: bool,
    pub preserve_blank_lines: bool,
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            style: Style::SemanticBlock,
            indent_width: 4,
            soft_line_width: 120,
            hard_line_width: 160,
            preserve_list_groups: true,
            preserve_blank_lines: true,
        }
    }
}

impl FormatOptions {
    fn validate(&self) -> Result<(), FormatDiagnostic> {
        if self.indent_width == 0 {
            return Err(FormatDiagnostic::InvalidOptions(
                "indent_width must be greater than zero".into(),
            ));
        }
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
    validation::parse_postgresql(source)?;

    let output = match options.style {
        Style::SemanticBlock => semantic_block::format(source, options)?,
    };

    validation::parse_postgresql(&output)?;
    validation::validate_equivalent(source, &output)?;

    let second_pass = match options.style {
        Style::SemanticBlock => semantic_block::format(&output, options)?,
    };
    if output != second_pass {
        return Err(FormatDiagnostic::NotIdempotent);
    }

    let warnings = semantic_block::validate_hard_width(&output, options)?;

    Ok(FormattedSql {
        changed: output != source,
        output,
        warnings,
    })
}
