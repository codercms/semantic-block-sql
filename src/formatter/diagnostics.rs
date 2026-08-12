use pg_query::protobuf::Token;

use super::semantic_block::{
    is_compact_grammar_parenthesis, is_function_call_name, is_function_call_syntax,
    is_type_keyword, is_type_modifier_syntax, is_uppercase_builtin,
};
use super::tokens::{SqlToken, tokenize};
use super::{
    Diagnostic, FormatDiagnostic, FormatOptions, FormatWarning, SemicolonPolicy, Severity,
    SourceRange,
};

struct GapChange<'a> {
    previous: Option<usize>,
    current: Option<usize>,
    source_range: SourceRange,
    source_gap: &'a str,
    output_gap: &'a str,
    trailing: bool,
}

pub(super) fn style_diagnostics(
    source: &str,
    output: &str,
    options: &FormatOptions,
) -> Result<Vec<Diagnostic>, FormatDiagnostic> {
    let source_tokens = tokenize(source)?;
    let output_tokens = tokenize(output)?;
    let source_terminal = terminal_semicolon(&source_tokens);
    let output_terminal = terminal_semicolon(&output_tokens);
    let source_skip = source_terminal.filter(|_| output_terminal.is_none());
    let output_skip = output_terminal.filter(|_| source_terminal.is_none());
    let pairs = align_tokens(&source_tokens, &output_tokens, source_skip, output_skip)?;

    let mut diagnostics = Vec::new();
    if let Some(diagnostic) = semicolon_diagnostic(&source_tokens, options.semicolon_policy) {
        diagnostics.push(diagnostic);
    }

    for &(source_index, output_index) in &pairs {
        let expected = output_tokens[output_index].text;
        let actual = source_tokens[source_index].text;
        if actual != expected {
            diagnostics.push(token_diagnostic(
                &source_tokens,
                source_index,
                actual,
                expected,
            ));
        }
    }

    if let Some(&(source_index, output_index)) = pairs.first() {
        add_gap_diagnostic(
            &mut diagnostics,
            &source_tokens,
            GapChange {
                previous: None,
                current: Some(source_index),
                source_range: SourceRange::new(0, source_tokens[source_index].start),
                source_gap: &source[..source_tokens[source_index].start],
                output_gap: &output[..output_tokens[output_index].start],
                trailing: false,
            },
        );
    }

    for pair in pairs.windows(2) {
        let (previous_source, previous_output) = pair[0];
        let (current_source, current_output) = pair[1];
        if skipped_between(previous_source, current_source, source_skip)
            || skipped_between(previous_output, current_output, output_skip)
        {
            continue;
        }

        let source_start = source_tokens[previous_source].end;
        let source_end = source_tokens[current_source].start;
        let output_start = output_tokens[previous_output].end;
        let output_end = output_tokens[current_output].start;
        add_gap_diagnostic(
            &mut diagnostics,
            &source_tokens,
            GapChange {
                previous: Some(previous_source),
                current: Some(current_source),
                source_range: SourceRange::new(source_start, source_end),
                source_gap: &source[source_start..source_end],
                output_gap: &output[output_start..output_end],
                trailing: false,
            },
        );
    }

    if let Some(&(source_index, output_index)) = pairs.last()
        && !skip_after(source_index, source_skip)
        && !skip_after(output_index, output_skip)
    {
        let source_start = source_tokens[source_index].end;
        let output_start = output_tokens[output_index].end;
        add_gap_diagnostic(
            &mut diagnostics,
            &source_tokens,
            GapChange {
                previous: Some(source_index),
                current: None,
                source_range: SourceRange::new(source_start, source.len()),
                source_gap: &source[source_start..],
                output_gap: &output[output_start..],
                trailing: true,
            },
        );
    }

    let has_style_error = diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == Severity::Error);
    if source != output && !has_style_error {
        diagnostics.push(Diagnostic {
            rule_id: if source.trim().is_empty() {
                "spacing.trailing_whitespace".into()
            } else {
                "layout.statement".into()
            },
            severity: Severity::Error,
            message: "source does not satisfy the required formatting rules".into(),
            source_range: SourceRange::new(0, source.len()),
            fix_available: true,
        });
    }

    diagnostics.sort_by(|left, right| {
        left.source_range
            .start
            .cmp(&right.source_range.start)
            .then(left.source_range.end.cmp(&right.source_range.end))
            .then(left.rule_id.cmp(&right.rule_id))
    });
    diagnostics.dedup();
    Ok(diagnostics)
}

pub(super) fn warning_diagnostics(
    source: &str,
    output: &str,
    warnings: &[FormatWarning],
    options: &FormatOptions,
) -> Result<Vec<Diagnostic>, FormatDiagnostic> {
    let source_tokens = tokenize(source)?;
    let output_tokens = tokenize(output)?;
    let source_terminal = terminal_semicolon(&source_tokens);
    let output_terminal = terminal_semicolon(&output_tokens);
    let pairs = align_tokens(
        &source_tokens,
        &output_tokens,
        source_terminal.filter(|_| output_terminal.is_none()),
        output_terminal.filter(|_| source_terminal.is_none()),
    )?;

    warnings
        .iter()
        .map(|warning| match warning {
            FormatWarning::IndivisibleTokenExceedsHardWidth { line, width } => {
                let output_range = output_line_range(output, *line);
                let output_index = output_range.and_then(|range| {
                    let indent = output[range.start..range.end]
                        .chars()
                        .take_while(|character| *character == ' ')
                        .count();
                    output_tokens
                        .iter()
                        .enumerate()
                        .filter(|(_, token)| token.start >= range.start && token.end <= range.end)
                        .filter(|(_, token)| {
                            indent + token.text.chars().count() > options.hard_line_width
                                || (token.is_comment()
                                    && output[range.start..token.end].chars().count()
                                        > options.hard_line_width)
                        })
                        .max_by_key(|(_, token)| token.text.chars().count())
                        .map(|(index, _)| index)
                });
                let source_range = output_index
                    .and_then(|output_index| {
                        pairs
                            .iter()
                            .find(|(_, candidate)| *candidate == output_index)
                            .map(|(source_index, _)| {
                                let token = &source_tokens[*source_index];
                                SourceRange::new(token.start, token.end)
                            })
                    })
                    .unwrap_or_else(|| SourceRange::new(0, source.len()));
                let source_line =
                    source[..source_range.start].bytes().filter(|byte| *byte == b'\n').count() + 1;
                Ok(Diagnostic {
                    rule_id: "layout.hard_line_width".into(),
                    severity: Severity::Warning,
                    message: format!(
                        "formatting source line {source_line} produces a line of width {width} because an indivisible token cannot be split"
                    ),
                    source_range,
                    fix_available: false,
                })
            }
        })
        .collect()
}

fn output_line_range(output: &str, line: usize) -> Option<SourceRange> {
    let mut start = 0usize;
    for (index, segment) in output.split_inclusive('\n').enumerate() {
        let end = start + segment.strip_suffix('\n').unwrap_or(segment).len();
        if index + 1 == line {
            return Some(SourceRange::new(start, end));
        }
        start += segment.len();
    }
    None
}

pub(super) fn unsupported_diagnostic(
    source: &str,
    error: &FormatDiagnostic,
    policy: super::UnsupportedPolicy,
) -> Diagnostic {
    let mut diagnostic = failure_diagnostic(source, error);
    diagnostic.severity = match policy {
        super::UnsupportedPolicy::Skip => Severity::Warning,
        super::UnsupportedPolicy::Error => Severity::Error,
    };
    diagnostic
}

pub(super) fn statement_skipped_diagnostic(
    source: &str,
    error: &FormatDiagnostic,
    policy: super::UnsupportedPolicy,
    statement_line: usize,
) -> Diagnostic {
    Diagnostic {
        rule_id: "format.statement_skipped".into(),
        severity: match policy {
            super::UnsupportedPolicy::Skip => Severity::Warning,
            super::UnsupportedPolicy::Error => Severity::Error,
        },
        message: format!("statement formatting skipped at line {statement_line}: {error}"),
        source_range: SourceRange::new(0, source.len()),
        fix_available: false,
    }
}

pub(super) fn failure_diagnostic(source: &str, error: &FormatDiagnostic) -> Diagnostic {
    let rule_id = match error {
        FormatDiagnostic::InvalidOptions(_) => "config.invalid",
        FormatDiagnostic::PostgreSqlParse(_) | FormatDiagnostic::PostgreSqlScan(_) => {
            "syntax.parse_failure"
        }
        FormatDiagnostic::UnsupportedSyntax { .. } => "syntax.unsupported",
        FormatDiagnostic::HardLineExceeded { .. } => "layout.hard_line_width",
        FormatDiagnostic::SemanticMismatch
        | FormatDiagnostic::ProtectedTokenChanged(_)
        | FormatDiagnostic::NotIdempotent
        | FormatDiagnostic::Ownership(_) => "format.safety_failure",
    };
    let source_range = match error {
        FormatDiagnostic::UnsupportedSyntax { start, end, .. } => SourceRange::new(*start, *end),
        _ => SourceRange::new(0, source.len()),
    };
    Diagnostic {
        rule_id: rule_id.into(),
        severity: Severity::Error,
        message: error.to_string(),
        source_range,
        fix_available: false,
    }
}

fn align_tokens(
    source: &[SqlToken<'_>],
    output: &[SqlToken<'_>],
    source_skip: Option<usize>,
    output_skip: Option<usize>,
) -> Result<Vec<(usize, usize)>, FormatDiagnostic> {
    let mut source_index = 0;
    let mut output_index = 0;
    let mut pairs = Vec::with_capacity(source.len().min(output.len()));

    while source_index < source.len() || output_index < output.len() {
        if source_skip == Some(source_index) {
            source_index += 1;
            continue;
        }
        if output_skip == Some(output_index) {
            output_index += 1;
            continue;
        }
        let (Some(source_token), Some(output_token)) =
            (source.get(source_index), output.get(output_index))
        else {
            return Err(FormatDiagnostic::SemanticMismatch);
        };
        if source_token.kind != output_token.kind {
            return Err(FormatDiagnostic::SemanticMismatch);
        }
        pairs.push((source_index, output_index));
        source_index += 1;
        output_index += 1;
    }

    Ok(pairs)
}

fn terminal_semicolon(tokens: &[SqlToken<'_>]) -> Option<usize> {
    tokens
        .iter()
        .rposition(|token| !token.is_comment())
        .filter(|&index| tokens[index].kind == Token::Ascii59)
}

fn semicolon_diagnostic(tokens: &[SqlToken<'_>], policy: SemicolonPolicy) -> Option<Diagnostic> {
    let last_syntax = tokens.iter().rposition(|token| !token.is_comment())?;
    let has_semicolon = tokens[last_syntax].kind == Token::Ascii59;
    match (policy, has_semicolon) {
        (SemicolonPolicy::Require, false) => Some(Diagnostic {
            rule_id: "statement.semicolon".into(),
            severity: Severity::Error,
            message: "terminal semicolon is required".into(),
            source_range: SourceRange::new(tokens[last_syntax].end, tokens[last_syntax].end),
            fix_available: true,
        }),
        (SemicolonPolicy::Omit, true) => Some(Diagnostic {
            rule_id: "statement.semicolon".into(),
            severity: Severity::Error,
            message: "terminal semicolon must be omitted".into(),
            source_range: SourceRange::new(tokens[last_syntax].start, tokens[last_syntax].end),
            fix_available: true,
        }),
        _ => None,
    }
}

fn token_diagnostic(
    tokens: &[SqlToken<'_>],
    index: usize,
    actual: &str,
    expected: &str,
) -> Diagnostic {
    let token = &tokens[index];
    let (rule_id, subject) = if token.kind == Token::NotEquals {
        ("operator.not_equal", "not-equal operator")
    } else if is_function_call_name(tokens, index) {
        if is_uppercase_builtin(token.text) {
            ("casing.builtin", "built-in function")
        } else {
            ("casing.function", "function name")
        }
    } else if token.kind == Token::Interval {
        if tokens
            .get(index + 1)
            .is_some_and(|next| matches!(next.kind, Token::Sconst | Token::Usconst))
        {
            ("casing.keyword", "INTERVAL literal introducer")
        } else {
            ("casing.type", "type name")
        }
    } else if is_type_keyword(token.kind)
        || (token.kind == Token::Ident
            && index
                .checked_sub(1)
                .is_some_and(|previous| tokens[previous].kind == Token::Typecast))
    {
        ("casing.type", "type name")
    } else {
        ("casing.keyword", "SQL keyword or grammar construct")
    };

    Diagnostic {
        rule_id: rule_id.into(),
        severity: Severity::Error,
        message: format!("{subject} must be `{expected}` instead of `{actual}`"),
        source_range: SourceRange::new(token.start, token.end),
        fix_available: true,
    }
}

fn add_gap_diagnostic(
    diagnostics: &mut Vec<Diagnostic>,
    source_tokens: &[SqlToken<'_>],
    change: GapChange<'_>,
) {
    if change.source_gap == change.output_gap {
        return;
    }

    let (rule_id, message) = if change.trailing && contains_trailing_whitespace(change.source_gap) {
        (
            "spacing.trailing_whitespace",
            "trailing whitespace must be removed".to_string(),
        )
    } else {
        let source_breaks = line_breaks(change.source_gap);
        let output_breaks = line_breaks(change.output_gap);
        if source_breaks != output_breaks {
            let rule_id = layout_rule(source_tokens, change.previous, change.current);
            (
                rule_id,
                "line breaks do not match the required syntax layout".to_string(),
            )
        } else if source_breaks > 0 || change.source_gap.contains('\t') {
            (
                "indent.nesting",
                "indentation must use four-space syntax nesting without tabs".to_string(),
            )
        } else {
            let rule_id = spacing_rule(source_tokens, change.previous, change.current);
            (
                rule_id,
                "token spacing does not match the mandatory spacing rule".to_string(),
            )
        }
    };

    diagnostics.push(Diagnostic {
        rule_id: rule_id.into(),
        severity: Severity::Error,
        message,
        source_range: change.source_range,
        fix_available: true,
    });
}

fn spacing_rule(
    tokens: &[SqlToken<'_>],
    previous: Option<usize>,
    current: Option<usize>,
) -> &'static str {
    let previous_kind = previous.map(|index| tokens[index].kind);
    let current_kind = current.map(|index| tokens[index].kind);
    if matches!(previous_kind, Some(Token::Ascii44)) || matches!(current_kind, Some(Token::Ascii44))
    {
        return "spacing.comma";
    }
    if matches!(previous_kind, Some(Token::Typecast))
        || matches!(current_kind, Some(Token::Typecast))
    {
        return "spacing.cast";
    }
    if current_kind == Some(Token::Ascii40)
        && previous.is_some_and(|index| is_function_call_syntax(tokens, index))
    {
        return "spacing.function_call";
    }
    if current_kind == Some(Token::Ascii40)
        && previous.is_some_and(|index| {
            is_type_modifier_syntax(tokens, index) || is_compact_grammar_parenthesis(tokens, index)
        })
    {
        return "spacing.sql_parenthesis";
    }
    if matches!(previous_kind, Some(Token::Ascii40))
        || matches!(current_kind, Some(Token::Ascii40 | Token::Ascii41))
    {
        return "spacing.sql_parenthesis";
    }
    if previous_kind.is_some_and(is_binary_operator) || current_kind.is_some_and(is_binary_operator)
    {
        return "spacing.binary_operator";
    }
    "spacing.token"
}

fn layout_rule(
    tokens: &[SqlToken<'_>],
    previous: Option<usize>,
    current: Option<usize>,
) -> &'static str {
    let kinds = [
        previous.map(|index| tokens[index].kind),
        current.map(|index| tokens[index].kind),
    ];
    if kinds
        .iter()
        .flatten()
        .any(|kind| matches!(kind, Token::Conflict | Token::Do))
    {
        return "layout.on_conflict";
    }
    if kinds
        .iter()
        .flatten()
        .any(|kind| matches!(kind, Token::Set))
    {
        return "layout.update_set";
    }
    if kinds.iter().flatten().any(|kind| {
        matches!(
            kind,
            Token::Join
                | Token::Left
                | Token::Right
                | Token::Full
                | Token::InnerP
                | Token::Cross
                | Token::Natural
                | Token::On
        )
    }) {
        return "layout.join_on";
    }
    if kinds
        .iter()
        .flatten()
        .any(|kind| matches!(kind, Token::Values))
    {
        return "layout.values";
    }
    if kinds
        .iter()
        .flatten()
        .any(|kind| matches!(kind, Token::Where | Token::And | Token::Or))
    {
        return "layout.boolean_group";
    }
    if kinds
        .iter()
        .flatten()
        .any(|kind| matches!(kind, Token::Case | Token::When | Token::Else | Token::EndP))
    {
        return "layout.case";
    }
    if kinds
        .iter()
        .flatten()
        .any(|kind| matches!(kind, Token::With | Token::Recursive))
    {
        return "layout.cte";
    }
    "layout.statement"
}

fn is_binary_operator(kind: Token) -> bool {
    matches!(
        kind,
        Token::Ascii37
            | Token::Ascii42
            | Token::Ascii43
            | Token::Ascii45
            | Token::Ascii47
            | Token::Ascii58
            | Token::Ascii60
            | Token::Ascii61
            | Token::Ascii62
            | Token::Ascii94
            | Token::ColonEquals
            | Token::EqualsGreater
            | Token::GreaterEquals
            | Token::LessEquals
            | Token::NotEquals
            | Token::Op
    )
}

fn contains_trailing_whitespace(gap: &str) -> bool {
    gap.strip_suffix('\n')
        .unwrap_or(gap)
        .bytes()
        .any(|byte| matches!(byte, b' ' | b'\t' | b'\r'))
}

fn line_breaks(gap: &str) -> usize {
    gap.bytes().filter(|byte| *byte == b'\n').count()
}

fn skipped_between(previous: usize, current: usize, skipped: Option<usize>) -> bool {
    skipped.is_some_and(|index| previous < index && index < current)
}

fn skip_after(last: usize, skipped: Option<usize>) -> bool {
    skipped.is_some_and(|index| index > last)
}
