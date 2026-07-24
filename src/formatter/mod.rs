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
}

impl Default for FormatOptions {
    fn default() -> Self {
        Self {
            style: Style::SemanticBlock,
            indent_width: 4,
            soft_line_width: 120,
            hard_line_width: 160,
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

    Ok(FormattedSql {
        changed: output != source,
        output,
    })
}
