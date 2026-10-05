//! CST-based Rust string extraction. This adapter never expands macros or writes files.

use std::borrow::Cow;
use std::collections::HashSet;

use thiserror::Error;
use tree_sitter::{Node, Parser, Tree};

use crate::config::{RustConfig, RustMultilineStringStyle};
use crate::{
    Diagnostic, FormatDiagnostic, FormatOptions, FormatWarning, Severity, SourceRange,
    UnsupportedPolicy, format_sql,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormattedRust {
    pub output: String,
    pub warnings: Vec<FormatWarning>,
    pub diagnostics: Vec<Diagnostic>,
}

#[derive(Debug, Error)]
pub enum RustError {
    #[error("failed to initialize Rust parser: {0}")]
    Parser(String),
    #[error("Rust parse failed")]
    Parse,
    #[error("invalid or misplaced Rust directive at line {line}: {message}")]
    Directive { line: usize, message: String },
    #[error("invalid Rust string literal at line {line}: {message}")]
    Literal { line: usize, message: String },
    #[error("embedded SQL at Rust line {line}: {source}")]
    EmbeddedSql {
        line: usize,
        #[source]
        source: FormatDiagnostic,
    },
    #[error("rewritten Rust literal does not preserve its formatted runtime value at line {0}")]
    RuntimeValueMismatch(usize),
    #[error("rewritten Rust source does not parse")]
    Reparse,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DirectiveKind {
    FileIgnore,
    Ignore,
    Sql,
}

#[derive(Clone, Copy)]
struct Directive<'tree> {
    node: Node<'tree>,
    kind: DirectiveKind,
}

/// Format complete PostgreSQL statements in reviewed Rust expression positions.
pub fn format_rust_source(
    source: &str,
    options: &FormatOptions,
    config: &RustConfig,
) -> Result<FormattedRust, RustError> {
    let tree = parse_rust(source)?;
    let mut candidates = Vec::new();
    let mut directives = Vec::new();
    collect(tree.root_node(), source, &mut candidates, &mut directives)?;
    for directive in &directives {
        if directive.kind == DirectiveKind::FileIgnore {
            let leading = tree
                .root_node()
                .named_children(&mut tree.walk())
                .all(|node| node.start_byte() >= directive.node.start_byte() || is_comment(node));
            if !leading {
                return Err(directive_error(
                    directive.node,
                    "file-ignore must precede Rust items",
                ));
            }
            return Ok(FormattedRust {
                output: source.into(),
                warnings: Vec::new(),
                diagnostics: Vec::new(),
            });
        }
    }

    let mut consumed = HashSet::new();
    let mut replacements = Vec::new();
    let mut warnings = Vec::new();
    let mut diagnostics = Vec::new();
    for literal in candidates {
        let attached = attached_directives(literal, source, &directives);
        let ignored = attached.iter().any(|d| d.kind == DirectiveKind::Ignore);
        let explicit = attached.iter().any(|d| d.kind == DirectiveKind::Sql);
        for directive in &attached {
            consumed.insert(directive.node.start_byte());
        }
        if ignored && explicit {
            return Err(directive_error(
                literal,
                "conflicting ignore and SQL markers",
            ));
        }
        if ignored || (!explicit && !config.auto_detect) {
            continue;
        }
        let text = &source[literal.byte_range()];
        let raw = text.starts_with('r');
        if (raw && !config.raw_strings) || (!raw && !config.interpreted_strings) {
            if explicit {
                return Err(directive_error(
                    literal,
                    "SQL marker targets a disabled string kind",
                ));
            }
            continue;
        }
        let line = literal.start_position().row + 1;
        let decoded = decode(text, line)?;
        let envelope = Envelope::new(&decoded);
        if !explicit && !super::looks_like_complete_sql_prefix(&envelope.sql) {
            continue;
        }
        let formatted = match format_sql(&envelope.sql, options) {
            Ok(formatted) => formatted,
            Err(FormatDiagnostic::PostgreSqlParse(_) | FormatDiagnostic::PostgreSqlScan(_))
                if !explicit =>
            {
                continue;
            }
            Err(source) => return Err(RustError::EmbeddedSql { line, source }),
        };
        let unchanged_opaque = formatted.output == envelope.sql
            && formatted.diagnostics.iter().any(|d| {
                matches!(
                    d.rule_id.as_str(),
                    "syntax.unsupported" | "format.statement_skipped"
                )
            });
        warnings.extend(formatted.warnings);
        diagnostics.extend(formatted.diagnostics.into_iter().map(|mut diagnostic| {
            diagnostic.message = format!("embedded SQL: {}", diagnostic.message);
            diagnostic.with_source_range(SourceRange::new(literal.start_byte(), literal.end_byte()))
        }));
        // Do not re-encode opaque SQL, including its authored Rust escape spelling.
        if unchanged_opaque {
            continue;
        }
        let expected = envelope.wrap(&formatted.output);
        if expected == decoded {
            continue;
        }
        let prefer_raw = raw
            || (config.multiline_string_style == RustMultilineStringStyle::PreferRaw
                && expected.contains('\n'));
        let crlf = text.contains("\r\n")
            || (!text.contains('\n')
                && source.contains("\r\n")
                && !source.replace("\r\n", "").contains('\n'));
        let rendered = encode(&expected, text, prefer_raw, crlf);
        if decode(&rendered, line)? != expected {
            return Err(RustError::RuntimeValueMismatch(line));
        }
        replacements.push((literal.byte_range(), rendered));
    }
    if let Some(directive) = directives
        .iter()
        .find(|d| !consumed.contains(&d.node.start_byte()))
    {
        return Err(directive_error(
            directive.node,
            "marker has no reviewed string owner",
        ));
    }
    let strict_failure = options.unsupported_policy == UnsupportedPolicy::Error
        && diagnostics.iter().any(|d| {
            d.severity == Severity::Error
                && matches!(
                    d.rule_id.as_str(),
                    "syntax.unsupported" | "format.statement_skipped"
                )
        });
    let mut output = source.to_owned();
    if !strict_failure {
        replacements.sort_by_key(|(range, _)| range.start);
        for (range, text) in replacements.into_iter().rev() {
            output.replace_range(range, &text);
        }
    }
    parse_rust(&output).map_err(|_| RustError::Reparse)?;
    Ok(FormattedRust {
        output,
        warnings,
        diagnostics,
    })
}

fn parse_rust(source: &str) -> Result<Tree, RustError> {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_rust::LANGUAGE.into())
        .map_err(|error| RustError::Parser(error.to_string()))?;
    let tree = parser.parse(source, None).ok_or(RustError::Parse)?;
    if tree.root_node().has_error() {
        return Err(RustError::Parse);
    }
    Ok(tree)
}

fn is_comment(node: Node<'_>) -> bool {
    matches!(node.kind(), "line_comment" | "block_comment")
}

fn is_string(node: Node<'_>, source: &str) -> bool {
    matches!(node.kind(), "string_literal" | "raw_string_literal")
        && matches!(source.as_bytes()[node.start_byte()], b'"' | b'r')
}

fn collect<'tree>(
    node: Node<'tree>,
    source: &str,
    candidates: &mut Vec<Node<'tree>>,
    directives: &mut Vec<Directive<'tree>>,
) -> Result<(), RustError> {
    if is_comment(node) {
        let text = source[node.byte_range()].trim();
        // Documentation comments are never control directives.
        if text.starts_with("///")
            || text.starts_with("//!")
            || text.starts_with("/**")
            || text.starts_with("/*!")
        {
            return Ok(());
        }
        let body = text
            .strip_prefix("//")
            .or_else(|| text.strip_prefix("/*").and_then(|s| s.strip_suffix("*/")))
            .unwrap_or(text)
            .trim();
        let kind = match body {
            "semblock:file-ignore" => Some(DirectiveKind::FileIgnore),
            "semblock:ignore" => Some(DirectiveKind::Ignore),
            "semblock:sql" | "language=SQL" => Some(DirectiveKind::Sql),
            _ if body.starts_with("semblock:") => {
                return Err(directive_error(node, "unknown or malformed directive"));
            }
            _ => None,
        };
        if let Some(kind) = kind {
            directives.push(Directive { node, kind });
        }
        return Ok(());
    }
    match node.kind() {
        "attribute_item" | "inner_attribute_item" | "macro_definition" => return Ok(()),
        "macro_invocation" => {
            collect_macro(node, source, candidates);
            // Comments in token trees still need directive validation.
            collect_macro_comments(node, source, directives)?;
            return Ok(());
        }
        "string_literal" | "raw_string_literal" => {
            if is_string(node, source) && !in_dynamic_expression(node) {
                candidates.push(node);
            }
            return Ok(());
        }
        _ => {}
    }
    for child in node.named_children(&mut node.walk()) {
        collect(child, source, candidates, directives)?;
    }
    Ok(())
}

fn collect_macro_comments<'tree>(
    node: Node<'tree>,
    source: &str,
    directives: &mut Vec<Directive<'tree>>,
) -> Result<(), RustError> {
    for child in node.named_children(&mut node.walk()) {
        if is_comment(child) {
            collect(child, source, &mut Vec::new(), directives)?;
        } else if child.kind() == "token_tree" {
            collect_macro_comments(child, source, directives)?;
        }
    }
    Ok(())
}

fn collect_macro<'tree>(node: Node<'tree>, source: &str, candidates: &mut Vec<Node<'tree>>) {
    let Some(name) = node.child_by_field_name("macro") else {
        return;
    };
    let name = source[name.byte_range()]
        .split_whitespace()
        .collect::<String>();
    let typed = match name.trim_start_matches("::") {
        "sqlx::query"
        | "sqlx::query_unchecked"
        | "sqlx::query_scalar"
        | "sqlx::query_scalar_unchecked" => false,
        "sqlx::query_as" | "sqlx::query_as_unchecked" => true,
        _ => return,
    };
    let Some(tokens) = node
        .named_children(&mut node.walk())
        .find(|n| n.kind() == "token_tree")
    else {
        return;
    };
    let parts = tokens
        .children(&mut tokens.walk())
        .filter(|n| !is_comment(*n))
        .collect::<Vec<_>>();
    for (index, literal) in parts
        .iter()
        .enumerate()
        .filter(|(_, n)| is_string(**n, source))
    {
        // A reviewed SQL argument is exactly one literal, never an expression or macro.
        if !parts
            .get(index + 1)
            .is_some_and(|n| matches!(n.kind(), "," | ")" | "]" | "}"))
        {
            continue;
        }
        let valid_prefix = if typed {
            parts.get(index.wrapping_sub(1)).is_some_and(|comma| {
                comma.kind() == ","
                    && syn::parse_str::<syn::Type>(
                        &source[tokens.start_byte() + 1..comma.start_byte()],
                    )
                    .is_ok()
            })
        } else {
            // Only comments and whitespace may precede the first argument.
            parts
                .iter()
                .take(index)
                .all(|n| matches!(n.kind(), "(" | "[" | "{"))
        };
        if valid_prefix {
            candidates.push(*literal);
            break;
        }
    }
}

fn in_dynamic_expression(mut node: Node<'_>) -> bool {
    let mut string_receiver = true;
    while let Some(parent) = node.parent() {
        if parent.kind() == "field_expression" && string_receiver {
            return true;
        }
        if matches!(parent.kind(), "arguments" | "call_expression") {
            string_receiver = false;
        }
        if parent
            .child_by_field_name("pattern")
            .is_some_and(|pattern| {
                pattern.start_byte() <= node.start_byte() && pattern.end_byte() >= node.end_byte()
            })
        {
            return true;
        }
        match parent.kind() {
            "binary_expression" | "extern_modifier" | "closure_parameters" | "match_pattern" => {
                return true;
            }
            "block"
            | "const_item"
            | "static_item"
            | "let_declaration"
            | "expression_statement"
            | "closure_expression" => return false,
            _ => node = parent,
        }
    }
    false
}

fn attached_directives<'tree>(
    literal: Node<'tree>,
    source: &str,
    directives: &[Directive<'tree>],
) -> Vec<Directive<'tree>> {
    let mut comments = HashSet::new();
    let mut owner = Some(literal);
    while let Some(node) = owner {
        if matches!(
            node.kind(),
            "block" | "source_file" | "function_item" | "closure_expression"
        ) {
            break;
        }
        let mut sibling = node.prev_named_sibling();
        let mut boundary = node.start_byte();
        while let Some(comment) = sibling {
            if !is_comment(comment) || !source[comment.end_byte()..boundary].trim().is_empty() {
                break;
            }
            comments.insert(comment.start_byte());
            boundary = comment.start_byte();
            sibling = comment.prev_named_sibling();
        }
        owner = node.parent();
    }
    directives
        .iter()
        .copied()
        .filter(|d| comments.contains(&d.node.start_byte()))
        .collect()
}

fn directive_error(node: Node<'_>, message: &str) -> RustError {
    RustError::Directive {
        line: node.start_position().row + 1,
        message: message.into(),
    }
}

fn decode(text: &str, line: usize) -> Result<String, RustError> {
    // Rust normalizes physical CRLF before tokenization; escaped CR remains a value byte.
    syn::parse_str::<syn::LitStr>(&text.replace("\r\n", "\n"))
        .map(|literal| literal.value())
        .map_err(|error| RustError::Literal {
            line,
            message: error.to_string(),
        })
}

fn encode(value: &str, original: &str, prefer_raw: bool, crlf: bool) -> String {
    if prefer_raw
        && !value
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        let mut hashes = original
            .strip_prefix('r')
            .map_or(0, |s| s.bytes().take_while(|b| *b == b'#').count());
        while value.contains(&format!("\"{}", "#".repeat(hashes))) {
            hashes += 1;
        }
        if hashes <= 255 {
            let hashes = "#".repeat(hashes);
            let value = if crlf {
                value.replace('\n', "\r\n")
            } else {
                value.to_owned()
            };
            return format!("r{hashes}\"{value}\"{hashes}");
        }
    }
    format!("{value:?}")
}

struct Envelope<'a> {
    sql: Cow<'a, str>,
    leading_newline: bool,
    suffix: &'a str,
}

impl<'a> Envelope<'a> {
    fn new(value: &'a str) -> Self {
        let leading_newline = value.starts_with('\n');
        let body = value.strip_prefix('\n').unwrap_or(value);
        let (sql, suffix) = match body.rsplit_once('\n') {
            Some((sql, tail)) if tail.chars().all(char::is_whitespace) => (sql, &body[sql.len()..]),
            _ => (body, ""),
        };
        let indent = sql
            .lines()
            .find(|line| !line.trim().is_empty())
            .map(|line| &line[..line.len() - line.trim_start_matches([' ', '\t']).len()])
            .unwrap_or_default();
        let sql = if leading_newline && !indent.is_empty() {
            Cow::Owned(
                sql.split('\n')
                    .map(|line| line.strip_prefix(indent).unwrap_or(line))
                    .collect::<Vec<_>>()
                    .join("\n"),
            )
        } else {
            Cow::Borrowed(sql.trim_start_matches([' ', '\t']))
        };
        Self {
            sql,
            leading_newline,
            suffix,
        }
    }

    fn wrap(&self, sql: &str) -> String {
        format!(
            "{}{}{}",
            if self.leading_newline { "\n" } else { "" },
            sql.trim_end_matches('\n'),
            self.suffix
        )
    }
}
