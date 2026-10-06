//! Source capabilities proven by the pinned PL/pgSQL parser, not SQL keywords.
use serde_json::Value;

use super::super::{Diagnostic, FormatDiagnostic, FormatOptions, SourceRange, tokens::tokenize};
use super::ir::{BodyNodeKind, RoutineBody};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum LeafCapability {
    Into { range: SourceRange, strict: bool },
    TypeReference(SourceRange),
    Transaction,
}

struct SqlSpec {
    query: String,
    into: bool,
    strict: bool,
}

#[derive(Default)]
struct ParserCapabilities {
    sql: Vec<SqlSpec>,
    transactions: Vec<(bool, bool)>,
    type_references: Vec<String>,
}

pub(super) fn bind(body: &mut RoutineBody<'_>, parsed: &Value) -> Result<(), FormatDiagnostic> {
    let mut specs = ParserCapabilities::default();
    collect(parsed, &mut specs)?;
    for node in &mut body.nodes {
        match node.kind {
            BodyNodeKind::Sql => {
                let mut matched = None;
                for (index, spec) in specs.sql.iter().enumerate() {
                    if let Some(capability) = bind_sql(node.text, spec)? {
                        matched = Some((index, capability));
                        break;
                    }
                }
                let (index, capability) =
                    matched.ok_or_else(|| ownership("SQL leaf differs from parser query"))?;
                specs.sql.remove(index);
                node.capability = capability;
            }
            BodyNodeKind::Transaction => {
                let words = significant(node.text)?;
                let words = words
                    .iter()
                    .map(|token| token.text.to_ascii_uppercase())
                    .collect::<Vec<_>>();
                let (commit, chain) = match words
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
                    .as_slice()
                {
                    ["COMMIT"] => (true, false),
                    ["ROLLBACK"] => (false, false),
                    ["COMMIT", "AND", "CHAIN"] => (true, true),
                    ["ROLLBACK", "AND", "CHAIN"] => (false, true),
                    ["COMMIT", "AND", "NO", "CHAIN"] => (true, false),
                    ["ROLLBACK", "AND", "NO", "CHAIN"] => (false, false),
                    _ => return Err(ownership("unreviewed transaction spelling")),
                };
                let index = specs
                    .transactions
                    .iter()
                    .position(|spec| *spec == (commit, chain))
                    .ok_or_else(|| ownership("transaction leaf differs from parser command"))?;
                specs.transactions.remove(index);
                node.capability = Some(LeafCapability::Transaction);
            }
            BodyNodeKind::Declaration => {
                let tokens = tokenize(node.text)?;
                // The declaration name is outside the datatype span. Preserve
                // only a reference whose exact spelling is recorded by PLpgSQL_type.
                let type_end = tokens
                    .iter()
                    .position(|token| {
                        matches!(
                            token.text.to_ascii_uppercase().as_str(),
                            "DEFAULT" | ":=" | "=" | ";"
                        )
                    })
                    .unwrap_or(tokens.len());
                for start in 1..type_end {
                    for end in start..type_end {
                        let span = &node.text[tokens[start].start..tokens[end].end];
                        if specs.type_references.iter().any(|name| name == span) {
                            node.capability = Some(LeafCapability::TypeReference(
                                SourceRange::new(tokens[start].start, tokens[end].end),
                            ));
                            break;
                        }
                    }
                    if node.capability.is_some() {
                        break;
                    }
                }
            }
            _ => {}
        }
    }
    if !specs.sql.is_empty() || !specs.transactions.is_empty() {
        return Err(ownership("parser statement has no source leaf"));
    }
    Ok(())
}

fn collect(value: &Value, specs: &mut ParserCapabilities) -> Result<(), FormatDiagnostic> {
    match value {
        Value::Object(fields) => {
            for (name, child) in fields {
                match name.as_str() {
                    "PLpgSQL_stmt_execsql" => {
                        let query = child
                            .pointer("/sqlstmt/PLpgSQL_expr/query")
                            .and_then(Value::as_str)
                            .ok_or_else(|| ownership("static SQL node has no query"))?;
                        let into = child.get("into").and_then(Value::as_bool).unwrap_or(false);
                        if into && child.get("target").is_none() {
                            return Err(ownership("INTO node has no parser target"));
                        }
                        specs.sql.push(SqlSpec {
                            query: query.into(),
                            into,
                            strict: child
                                .get("strict")
                                .and_then(Value::as_bool)
                                .unwrap_or(false),
                        });
                    }
                    "PLpgSQL_stmt_commit" | "PLpgSQL_stmt_rollback" => {
                        specs.transactions.push((
                            name == "PLpgSQL_stmt_commit",
                            child.get("chain").and_then(Value::as_bool).unwrap_or(false),
                        ));
                    }
                    "PLpgSQL_type" => {
                        if let Some(name) = child.get("typname").and_then(Value::as_str)
                            && (name.to_ascii_uppercase().ends_with("%TYPE")
                                || name.to_ascii_uppercase().ends_with("%ROWTYPE"))
                        {
                            specs.type_references.push(name.into());
                        }
                    }
                    _ => {}
                }
                collect(child, specs)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                collect(item, specs)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn significant(source: &str) -> Result<Vec<super::super::tokens::SqlToken<'_>>, FormatDiagnostic> {
    Ok(tokenize(source)?
        .into_iter()
        .filter(|token| !token.is_comment() && token.text != ";")
        .collect())
}

fn bind_sql(
    source: &str,
    spec: &SqlSpec,
) -> Result<Option<Option<LeafCapability>>, FormatDiagnostic> {
    let original = significant(source)?;
    let query = significant(&spec.query)?;
    if !spec.into {
        return Ok((original
            .iter()
            .map(|token| token.text)
            .eq(query.iter().map(|token| token.text)))
        .then_some(None));
    }
    let prefix = original
        .iter()
        .zip(&query)
        .take_while(|(a, b)| a.text == b.text)
        .count();
    let suffix = original[prefix..]
        .iter()
        .rev()
        .zip(query[prefix..].iter().rev())
        .take_while(|(a, b)| a.text == b.text)
        .count();
    if prefix + suffix != query.len() || original.len() <= query.len() {
        return Ok(None);
    }
    let end_index = original.len() - suffix;
    let removed = &original[prefix..end_index];
    if !removed[0].text.eq_ignore_ascii_case("INTO")
        || removed
            .get(1)
            .is_some_and(|token| token.text.eq_ignore_ascii_case("STRICT"))
            != spec.strict
    {
        return Ok(None);
    }
    let end = original.get(end_index).map_or_else(
        || {
            source
                .trim_end()
                .strip_suffix(';')
                .map_or(source.len(), |text| text.len())
        },
        |token| token.start,
    );
    Ok(Some(Some(LeafCapability::Into {
        range: SourceRange::new(removed[0].start, end),
        strict: spec.strict,
    })))
}

pub(super) fn format(
    source: &str,
    capability: Option<&LeafCapability>,
    options: &FormatOptions,
) -> Result<Option<(String, Vec<Diagnostic>)>, FormatDiagnostic> {
    let Some(capability) = capability else {
        return Ok(None);
    };
    let output = match capability {
        LeafCapability::TypeReference(range) => {
            let prefix =
                super::normalize_procedural_code(source[..range.start].trim_end(), options)?;
            let suffix =
                super::normalize_procedural_code(source[range.end..].trim_start(), options)?;
            format!(
                "{prefix} {}{}{}",
                &source[range.start..range.end],
                if suffix.starts_with(';') || suffix.is_empty() {
                    ""
                } else {
                    " "
                },
                suffix
            )
        }
        LeafCapability::Transaction => {
            super::super::semantic_block::format_procedural_command(source, options)?
        }
        LeafCapability::Into { range, strict } => {
            let sql = format!("{} {}", &source[..range.start], &source[range.end..]);
            let formatted = super::super::format_sql(&sql, options)?;
            let child_diagnostics = formatted
                .diagnostics
                .iter()
                .filter(|diagnostic| !diagnostic.fix_available)
                .cloned()
                .map(|mut diagnostic| {
                    let map = |offset: usize| {
                        if offset <= range.start {
                            offset
                        } else {
                            offset + range.end - range.start - 1
                        }
                    };
                    diagnostic.source_range = SourceRange::new(
                        map(diagnostic.source_range.start),
                        map(diagnostic.source_range.end),
                    );
                    diagnostic
                })
                .collect::<Vec<_>>();
            if child_diagnostics.iter().any(|diagnostic| {
                matches!(
                    diagnostic.rule_id.as_str(),
                    "syntax.unsupported" | "format.statement_skipped"
                )
            }) {
                return Ok(Some((source.to_owned(), child_diagnostics)));
            }
            // Normalize the same parser-owned SQL before binding its insertion
            // boundary: allowed multiword aliases may change token cardinality.
            let normalized = super::super::type_aliases::normalize(&sql, options, &[])?;
            let boundary = normalized.map_boundary(range.start)?;
            let prefix_tokens = significant(&normalized.output)?
                .iter()
                .take_while(|token| token.end <= boundary)
                .count();
            let tokens = significant(&formatted.output)?;
            if tokens.len() != significant(&normalized.output)?.len() {
                return Err(ownership("SQL token cardinality changed around INTO"));
            }
            let clause_tokens = significant(&source[range.start..range.end])?;
            let owned_clause_tokens = tokenize(&source[range.start..range.end])?;
            let trailing_lines = owned_clause_tokens.last().map_or(0, |token| {
                source[range.start + token.end..range.end]
                    .bytes()
                    .filter(|&byte| byte == b'\n')
                    .count()
            });
            let target_start = clause_tokens[usize::from(*strict) + 1].start;
            let target = &source[range.start + target_start..range.end];
            let mut prefix = source[range.start..range.start + target_start].to_owned();
            for token in clause_tokens.iter().take(usize::from(*strict) + 1).rev() {
                prefix.replace_range(token.start..token.end, &token.text.to_ascii_uppercase());
            }
            let clause = format_into_targets(&prefix, target, options)?;
            let insert = tokens.get(prefix_tokens).map_or_else(
                || {
                    formatted
                        .output
                        .trim_end()
                        .strip_suffix(';')
                        .map_or(formatted.output.len(), |text| text.len())
                },
                |token| token.start,
            );
            let before = formatted.output[..insert].trim_end();
            let after = formatted.output[insert..].trim_start();
            let indent = before
                .lines()
                .last()
                .map_or(0, |line| line.len() - line.trim_start().len());
            let clause = clause.trim_end();
            let clause_tokens = tokenize(clause)?;
            let ends_line_comment = clause_tokens
                .last()
                .is_some_and(|token| token.text.starts_with("--"));
            let output = format!(
                "{before}\n{}{clause}{}{}",
                " ".repeat(indent),
                if trailing_lines > 0 || ends_line_comment {
                    "\n".repeat(trailing_lines.max(1))
                } else if after.starts_with(';') || after.is_empty() {
                    String::new()
                } else {
                    "\n".to_owned()
                },
                after
            );
            return Ok(Some((output, child_diagnostics)));
        }
    };
    Ok(Some((output, Vec::new())))
}

/// Targets use the canonical SQL list planner without flattening physical lines.
/// The PL-owned introducer retains its own comments and exact target boundary.
fn format_into_targets(
    prefix: &str,
    target: &str,
    options: &FormatOptions,
) -> Result<String, FormatDiagnostic> {
    let prefix_tokens = tokenize(prefix)?;
    let authored_lines = prefix_tokens.last().map_or(0, |token| {
        prefix[token.end..]
            .bytes()
            .filter(|&byte| byte == b'\n')
            .count()
    });
    let prefix = super::super::semantic_block::format_procedural_command(prefix, options)?;
    let prefix = prefix.trim_end();
    let tokens = tokenize(prefix)?;
    let ends_comment = tokens
        .last()
        .is_some_and(|token| token.text.starts_with("--"));
    let mut target_options = options.clone();
    target_options.semicolon_policy = super::super::SemicolonPolicy::Omit;
    let extra = if ends_comment || authored_lines > 0 {
        0
    } else {
        prefix
            .lines()
            .last()
            .map_or(0, |line| line.chars().count())
            .saturating_sub("SELECT".len())
    };
    target_options.soft_line_width = options.soft_line_width.saturating_sub(extra).max(1);
    target_options.hard_line_width = options
        .hard_line_width
        .saturating_sub(extra)
        .max(target_options.soft_line_width);
    // A synthetic terminator follows a physical newline, so a final -- comment
    // cannot consume it. Only this terminator is omitted by the SQL adapter.
    let adapted = format!("SELECT {target}\n;");
    let formatted = super::super::format_sql(&adapted, &target_options)?;
    let output = formatted
        .output
        .strip_prefix("SELECT")
        .ok_or_else(|| ownership("target adapter lost SELECT"))?;
    let separator = if output.starts_with('\n') || ends_comment || authored_lines > 0 {
        "\n".repeat(authored_lines.max(1))
    } else {
        " ".to_owned()
    };
    Ok(format!(
        "{prefix}{separator}{}",
        if output.starts_with('\n') {
            output.trim_start_matches('\n')
        } else {
            output.trim_start()
        }
    ))
}

fn ownership(message: &str) -> FormatDiagnostic {
    FormatDiagnostic::Ownership(format!("PL/pgSQL capability: {message}"))
}
