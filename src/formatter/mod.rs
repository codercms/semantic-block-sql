mod diagnostics;
mod layout_ir;
mod ownership;
mod procedural;
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
pub enum UnsupportedPolicy {
    /// Preserve unsupported syntax, emit warnings, and continue formatting.
    #[default]
    Skip,
    /// Preserve the complete input and emit unsupported syntax as errors.
    Error,
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
    pub unsupported_policy: UnsupportedPolicy,
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
            unsupported_policy: UnsupportedPolicy::Skip,
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
    let first = format_document_once(source, options)?;
    if options.unsupported_policy == UnsupportedPolicy::Error
        && first
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.rule_id == "syntax.unsupported")
    {
        return Ok(FormattedSql {
            output: source.to_owned(),
            changed: false,
            warnings: first.warnings,
            diagnostics: first.diagnostics,
        });
    }

    let second = format_document_once(&first.output, options)?;
    if first.output != second.output {
        return Err(FormatDiagnostic::NotIdempotent);
    }
    Ok(first)
}

fn format_document_once(
    source: &str,
    options: &FormatOptions,
) -> Result<FormattedSql, FormatDiagnostic> {
    let (output, mut diagnostics) = format_document_content(source, options)?;
    let warnings = semantic_block::validate_hard_width(&output, options)?;
    let unsupported_ranges = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.rule_id == "syntax.unsupported")
        .map(|diagnostic| diagnostic.source_range)
        .collect::<Vec<_>>();
    let mut style_diagnostics = diagnostics::style_diagnostics(source, &output, options)?;
    style_diagnostics.retain(|diagnostic| {
        !unsupported_ranges.iter().any(|range| {
            diagnostic.source_range.start >= range.start && diagnostic.source_range.end <= range.end
        })
    });
    diagnostics.extend(style_diagnostics);
    diagnostics.extend(diagnostics::warning_diagnostics(source, &warnings));

    Ok(FormattedSql {
        changed: output != source,
        output,
        warnings,
        diagnostics,
    })
}

fn format_document_content(
    source: &str,
    options: &FormatOptions,
) -> Result<(String, Vec<Diagnostic>), FormatDiagnostic> {
    let Some(region) = find_copy_stdin_region(source)? else {
        return format_regular_document_content(source, options);
    };

    let (prefix, mut diagnostics) =
        format_document_content(&source[..region.header_start], options)?;
    let header_line_offset = completed_line_count(&prefix);
    let (header, header_diagnostics) =
        format_regular_document_content(&source[region.header_start..region.header_end], options)
            .map_err(|error| shift_statement_error_lines(error, header_line_offset))?;
    diagnostics.extend(
        header_diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.shifted(region.header_start)),
    );
    let payload = &source[region.header_end..region.payload_end];
    let suffix_line_offset =
        header_line_offset + completed_line_count(&header) + completed_line_count(payload);
    let (suffix, suffix_diagnostics) =
        format_document_content(&source[region.payload_end..], options)
            .map_err(|error| shift_statement_error_lines(error, suffix_line_offset))?;
    diagnostics.extend(
        suffix_diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.shifted(region.payload_end)),
    );

    let mut output = String::with_capacity(source.len() + header.len());
    output.push_str(&prefix);
    output.push_str(&header);
    output.push_str(payload);
    output.push_str(&suffix);
    Ok((output, diagnostics))
}

fn format_regular_document_content(
    source: &str,
    options: &FormatOptions,
) -> Result<(String, Vec<Diagnostic>), FormatDiagnostic> {
    let parsed = pg_query::parse(source)
        .map_err(|error| FormatDiagnostic::PostgreSqlParse(error.to_string()))?;
    let split = pg_query::split_with_parser(source)
        .map_err(|error| FormatDiagnostic::PostgreSqlParse(error.to_string()))?;
    if parsed.protobuf.stmts.is_empty() {
        let formatted = format_supported_statement(source, options)?;
        return Ok((formatted.output, Vec::new()));
    }
    if split.len() != parsed.protobuf.stmts.len() {
        return Err(FormatDiagnostic::Ownership(
            "PostgreSQL parser and splitter disagree on statement count".into(),
        ));
    }
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0usize;
    let mut diagnostics = Vec::new();

    for raw in &parsed.protobuf.stmts {
        let (start, end) = statement_span(source, raw);
        if start < cursor || end < start {
            return Err(FormatDiagnostic::Ownership(
                "PostgreSQL statement ranges overlap or are out of order".into(),
            ));
        }
        output.push_str(&normalize_document_gap(&source[cursor..start], false));
        let statement = &source[start..end];
        let routine = is_routine_statement(raw);
        match format_statement_once(statement, raw, options) {
            Ok(formatted) => {
                output.push_str(&formatted.output);
                if routine {
                    diagnostics.extend(
                        formatted
                            .diagnostics
                            .into_iter()
                            .map(|diagnostic| diagnostic.shifted(start)),
                    );
                }
            }
            Err(error @ FormatDiagnostic::UnsupportedSyntax { .. }) => {
                output.push_str(statement);
                diagnostics.push(
                    diagnostics::unsupported_diagnostic(
                        statement,
                        &error,
                        options.unsupported_policy,
                    )
                    .shifted(start),
                );
            }
            Err(error) => {
                let line_offset = completed_line_count(&output);
                return Err(shift_statement_error_lines(error, line_offset));
            }
        }
        cursor = end;
    }
    output.push_str(&normalize_document_gap(&source[cursor..], true));
    Ok((output, diagnostics))
}

fn completed_line_count(source: &str) -> usize {
    source.bytes().filter(|byte| *byte == b'\n').count()
}

fn shift_statement_error_lines(error: FormatDiagnostic, line_offset: usize) -> FormatDiagnostic {
    match error {
        FormatDiagnostic::HardLineExceeded {
            line,
            width,
            hard_limit,
        } => FormatDiagnostic::HardLineExceeded {
            line: line + line_offset,
            width,
            hard_limit,
        },
        other => other,
    }
}

#[derive(Debug, Clone, Copy)]
struct CopyStdinRegion {
    header_start: usize,
    header_end: usize,
    payload_end: usize,
}

fn find_copy_stdin_region(source: &str) -> Result<Option<CopyStdinRegion>, FormatDiagnostic> {
    use pg_query::protobuf::node::Node;

    let mut statement_start = 0usize;
    for semicolon in top_level_semicolons(source) {
        let candidate = &source[statement_start..=semicolon];
        let leading = candidate
            .char_indices()
            .find(|(_, character)| !character.is_whitespace())
            .map_or(candidate.len(), |(index, _)| index);
        let header_start = statement_start + leading;
        if header_start <= semicolon {
            let header = &source[header_start..=semicolon];
            if let Ok(parsed) = pg_query::parse(header)
                && parsed.protobuf.stmts.len() == 1
                && matches!(
                    parsed.protobuf.stmts[0]
                        .stmt
                        .as_deref()
                        .and_then(|node| node.node.as_ref()),
                    Some(Node::CopyStmt(copy))
                        if copy.is_from && !copy.is_program && copy.filename.is_empty()
                )
            {
                let header_end = semicolon + 1;
                if let Some(payload_end) = copy_stdin_payload_end(source, header_end) {
                    return Ok(Some(CopyStdinRegion {
                        header_start,
                        header_end,
                        payload_end,
                    }));
                }
            }
        }
        statement_start = semicolon + 1;
    }
    Ok(None)
}

fn copy_stdin_payload_end(source: &str, header_end: usize) -> Option<usize> {
    let mut cursor = header_end;
    while cursor < source.len() {
        let line_end = source[cursor..]
            .find('\n')
            .map_or(source.len(), |offset| cursor + offset + 1);
        let line = source[cursor..line_end].trim_end_matches(['\n', '\r']);
        if line == r"\." {
            return Some(line_end);
        }
        cursor = line_end;
    }
    None
}

fn top_level_semicolons(source: &str) -> Vec<usize> {
    #[derive(Clone, Copy)]
    enum State<'a> {
        Normal,
        Single,
        Double,
        Dollar(&'a str),
        LineComment,
        BlockComment(usize),
    }

    let bytes = source.as_bytes();
    let mut semicolons = Vec::new();
    let mut state = State::Normal;
    let mut index = 0usize;
    while index < bytes.len() {
        match state {
            State::Normal => match bytes[index] {
                b'\'' => {
                    state = State::Single;
                    index += 1;
                }
                b'"' => {
                    state = State::Double;
                    index += 1;
                }
                b'-' if bytes.get(index + 1) == Some(&b'-') => {
                    state = State::LineComment;
                    index += 2;
                }
                b'/' if bytes.get(index + 1) == Some(&b'*') => {
                    state = State::BlockComment(1);
                    index += 2;
                }
                b'$' => {
                    if let Some((tag, end)) = dollar_tag_at(source, index) {
                        state = State::Dollar(tag);
                        index = end;
                    } else {
                        index += 1;
                    }
                }
                b';' => {
                    semicolons.push(index);
                    index += 1;
                }
                _ => index += 1,
            },
            State::Single => {
                if bytes[index] == b'\'' {
                    if bytes.get(index + 1) == Some(&b'\'') {
                        index += 2;
                    } else {
                        state = State::Normal;
                        index += 1;
                    }
                } else {
                    index += 1;
                }
            }
            State::Double => {
                if bytes[index] == b'"' {
                    if bytes.get(index + 1) == Some(&b'"') {
                        index += 2;
                    } else {
                        state = State::Normal;
                        index += 1;
                    }
                } else {
                    index += 1;
                }
            }
            State::Dollar(tag) => {
                if source[index..].starts_with(tag) {
                    state = State::Normal;
                    index += tag.len();
                } else {
                    index += 1;
                }
            }
            State::LineComment => {
                if bytes[index] == b'\n' {
                    state = State::Normal;
                }
                index += 1;
            }
            State::BlockComment(depth) => {
                if bytes[index] == b'/' && bytes.get(index + 1) == Some(&b'*') {
                    state = State::BlockComment(depth + 1);
                    index += 2;
                } else if bytes[index] == b'*' && bytes.get(index + 1) == Some(&b'/') {
                    state = if depth == 1 {
                        State::Normal
                    } else {
                        State::BlockComment(depth - 1)
                    };
                    index += 2;
                } else {
                    index += 1;
                }
            }
        }
    }
    semicolons
}

fn dollar_tag_at(source: &str, start: usize) -> Option<(&str, usize)> {
    let bytes = source.as_bytes();
    let mut end = start + 1;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    if bytes.get(end) == Some(&b'$') {
        let tag = &source[start..=end];
        Some((tag, end + 1))
    } else {
        None
    }
}

fn is_routine_statement(raw: &pg_query::protobuf::RawStmt) -> bool {
    use pg_query::protobuf::node::Node;
    matches!(
        raw.stmt.as_deref().and_then(|node| node.node.as_ref()),
        Some(Node::DoStmt(_) | Node::CreateFunctionStmt(_))
    )
}

fn format_statement_once(
    source: &str,
    raw: &pg_query::protobuf::RawStmt,
    options: &FormatOptions,
) -> Result<FormattedSql, FormatDiagnostic> {
    if is_routine_statement(raw) {
        return procedural::format_single_routine(source, options);
    }
    format_supported_statement(source, options)
}

fn format_supported_statement(
    source: &str,
    options: &FormatOptions,
) -> Result<FormattedSql, FormatDiagnostic> {
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

fn normalize_document_gap(source: &str, final_gap: bool) -> String {
    let mut output = String::with_capacity(source.len());
    for segment in source.split_inclusive('\n') {
        if let Some(line) = segment.strip_suffix('\n') {
            output.push_str(line.trim_end_matches([' ', '\t', '\r']));
            output.push('\n');
        } else if final_gap {
            output.push_str(segment.trim_end_matches([' ', '\t', '\r']));
        } else {
            output.push_str(segment);
        }
    }
    output
}

fn statement_span(source: &str, raw: &pg_query::protobuf::RawStmt) -> (usize, usize) {
    let raw_start = usize::try_from(raw.stmt_location)
        .unwrap_or(0)
        .min(source.len());
    let leading_whitespace = source[raw_start..]
        .char_indices()
        .find(|(_, character)| !character.is_whitespace())
        .map_or(source.len() - raw_start, |(index, _)| index);
    let start = raw_start
        .saturating_add(leading_whitespace)
        .min(source.len());
    let length = usize::try_from(raw.stmt_len).unwrap_or(0);
    let mut end = if length == 0 {
        source.len()
    } else {
        raw_start.saturating_add(length).min(source.len())
    };
    if source.as_bytes().get(end) == Some(&b';') {
        end += 1;
    }
    (start, end)
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
