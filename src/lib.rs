//! Reusable Semantic Block SQL formatting engine.
//!
//! The library never writes files. CLI, host-language extraction, and editor
//! adapters all call the same [`format_sql`] facade.

mod formatter;

pub use formatter::{
    FormatDiagnostic, FormatOptions, FormatWarning, FormattedSql, Style, format_sql,
    validate_equivalent,
};
