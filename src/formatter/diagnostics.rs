use pg_query::protobuf::Token;

use super::semantic_block::{
    is_compact_grammar_parenthesis, is_function_call_name, is_function_call_syntax,
    is_type_keyword, is_type_modifier_syntax, is_uppercase_builtin,
};
use super::tokens::{
    SqlToken, comment_trailing_whitespace_ranges, normalize_comment_trailing_whitespace, tokenize,
};
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
            let token = &source_tokens[source_index];
            if token.is_comment()
                && normalize_comment_trailing_whitespace(actual).as_ref() == expected
            {
                diagnostics.extend(comment_trailing_whitespace_ranges(actual).into_iter().map(
                    |range| Diagnostic {
                        rule_id: "spacing.trailing_whitespace".into(),
                        severity: Severity::Error,
                        message: "trailing whitespace must be removed".into(),
                        source_range: SourceRange::new(
                            token.start + range.start,
                            token.start + range.end,
                        ),
                        fix_available: true,
                    },
                ));
            } else {
                diagnostics.push(token_diagnostic(
                    &source_tokens,
                    source_index,
                    actual,
                    expected,
                ));
            }
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
    if warnings.is_empty() {
        return Ok(Vec::new());
    }
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

    let body_literals = routine_body_literals(source, &source_tokens);
    warnings
        .iter()
        .map(|warning| match warning {
            FormatWarning::IndivisibleTokenExceedsHardWidth { line, width } => {
                let output_range = output_line_range(output, *line);
                let output_index = output_range.and_then(|range| warning_output_token(&output_tokens, output, range, options.hard_line_width));
                let source_range = output_index
                    .and_then(|output_index| {
                        pairs
                            .iter()
                            .find(|(_, candidate)| *candidate == output_index)
                            .map(|(source_index, _)| {
                                let token = &source_tokens[*source_index];
                                warning_token_range(token, &output_tokens[output_index], output_range, options, body_literals.contains(&token.start))
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

fn warning_token_range(
    source: &SqlToken<'_>,
    output: &SqlToken<'_>,
    line: Option<SourceRange>,
    options: &FormatOptions,
    routine_body: bool,
) -> SourceRange {
    let whole = SourceRange::new(source.start, source.end);
    let Some(line) = line else {
        return whole;
    };
    if source.kind == Token::Sconst && (routine_body || source.text != output.text) {
        if let (Some((source_start, source_body)), Some((output_start, output_body))) = (
            dollar_token_content(source.text),
            dollar_token_content(output.text),
        ) {
            let relative = line.start.saturating_sub(output.start + output_start);
            if relative <= output_body.len() {
                let body_line = output_body[..relative]
                    .bytes()
                    .filter(|byte| *byte == b'\n')
                    .count()
                    + 1;
                if let Some(mapped) =
                    body_warning_range(source_body, output_body, body_line, options)
                {
                    return SourceRange::new(
                        source.start + source_start + mapped.start,
                        source.start + source_start + mapped.end,
                    );
                }
            }
        }
        return whole;
    }
    if output.text.contains('\n') {
        // Protected multiline token contents are unchanged apart from permitted
        // comment trailing whitespace. Physical line ordinals remain stable.
        let relative = line
            .start
            .saturating_sub(output.start)
            .min(output.text.len());
        let ordinal = output.text[..relative]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count()
            + 1;
        if let Some(fragment) = output_line_range(source.text, ordinal) {
            return SourceRange::new(source.start + fragment.start, source.start + fragment.end);
        }
    }
    whole
}

fn warning_output_token(
    tokens: &[SqlToken<'_>],
    output: &str,
    range: SourceRange,
    hard_width: usize,
) -> Option<usize> {
    let indent = output[range.start..range.end]
        .chars()
        .take_while(|character| *character == ' ')
        .count();
    tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| token.start < range.end && range.start < token.end)
        .filter(|(_, token)| {
            let start = token.start.max(range.start);
            let end = token.end.min(range.end);
            indent + output[start..end].chars().count() > hard_width
                || (token.is_comment() && output[range.start..end].chars().count() > hard_width)
        })
        .max_by_key(|(_, token)| token.text.chars().count())
        .map(|(index, _)| index)
}

fn body_warning_range(
    source: &str,
    output: &str,
    line: usize,
    options: &FormatOptions,
) -> Option<SourceRange> {
    let source_tokens = tokenize(source).ok()?;
    let output_tokens = tokenize(output).ok()?;
    let range = output_line_range(output, line)?;
    let output_index =
        warning_output_token(&output_tokens, output, range, options.hard_line_width)?;
    let target = &output_tokens[output_index];
    // Match exact token identity and occurrence, independently of optional type
    // aliases changing surrounding token cardinality. The formatter's safety
    // gate preserves protected token order; ambiguous multiplicity stays local
    // to the enclosing body token rather than guessing a source occurrence.
    let matches = |token: &SqlToken<'_>| {
        token.kind == target.kind
            && if target.is_comment() {
                normalize_comment_trailing_whitespace(token.text)
                    == normalize_comment_trailing_whitespace(target.text)
            } else {
                token.text == target.text
            }
    };
    let source_matches = source_tokens
        .iter()
        .filter(|token| matches(token))
        .collect::<Vec<_>>();
    let output_matches = output_tokens
        .iter()
        .enumerate()
        .filter(|(_, token)| matches(token))
        .map(|(index, _)| index)
        .collect::<Vec<_>>();
    if source_matches.len() != output_matches.len() {
        return None;
    }
    let ordinal = output_matches
        .iter()
        .position(|index| *index == output_index)?;
    Some(warning_token_range(
        source_matches[ordinal],
        target,
        Some(range),
        options,
        false,
    ))
}

fn routine_body_literals(
    source: &str,
    tokens: &[SqlToken<'_>],
) -> std::collections::HashSet<usize> {
    use pg_query::protobuf::node::Node;
    let mut locations = std::collections::HashSet::new();
    let Ok(parsed) = pg_query::parse(source) else {
        return locations;
    };
    for raw in &parsed.protobuf.stmts {
        let (options, anonymous) = match raw.stmt.as_deref().and_then(|node| node.node.as_ref()) {
            Some(Node::CreateFunctionStmt(statement)) if statement.sql_body.is_none() => {
                (&statement.options, false)
            }
            Some(Node::DoStmt(statement)) => (&statement.args, true),
            _ => continue,
        };
        let language = options.iter().find_map(|node| match node.node.as_ref() {
            Some(Node::DefElem(option)) if option.defname == "language" => {
                super::procedural::option_string(option)
            }
            _ => None,
        });
        if !matches!(language.as_deref().unwrap_or("plpgsql"), "sql" | "plpgsql") {
            continue;
        }
        let (start, end) = super::statement_span(source, raw);
        for option in options {
            let Some(Node::DefElem(option)) = option.node.as_ref() else {
                continue;
            };
            if option.defname != "as" {
                continue;
            }
            let value = match option.arg.as_deref().and_then(|node| node.node.as_ref()) {
                Some(Node::String(value)) => Some(value.sval.as_str()),
                Some(Node::List(list)) if list.items.len() == 1 => {
                    match list.items[0].node.as_ref() {
                        Some(Node::String(value)) => Some(value.sval.as_str()),
                        _ => None,
                    }
                }
                _ => None,
            };
            let Some(value) = value else {
                continue;
            };
            let first = if anonymous {
                None
            } else {
                tokens
                    .iter()
                    .position(|token| {
                        usize::try_from(option.location).ok() == Some(token.start)
                            && token.kind == Token::As
                    })
                    .and_then(|index| tokens[index + 1..].iter().find(|token| !token.is_comment()))
                    .map(|token| token.start)
            };
            for token in tokens.iter().filter(|token| {
                token.kind == Token::Sconst && start <= token.start && token.end <= end
            }) {
                if (anonymous || first == Some(token.start))
                    && dollar_token_content(token.text).is_some_and(|(_, body)| body == value)
                {
                    locations.insert(token.start);
                }
            }
        }
    }
    locations
}

fn dollar_token_content(text: &str) -> Option<(usize, &str)> {
    if !text.starts_with('$') {
        return None;
    }
    let delimiter_end = text[1..].find('$')? + 2;
    let delimiter = &text[..delimiter_end];
    (text.len() >= delimiter_end * 2 && text.ends_with(delimiter)).then(|| {
        (
            delimiter_end,
            &text[delimiter_end..text.len() - delimiter_end],
        )
    })
}

fn output_line_range(output: &str, line: usize) -> Option<SourceRange> {
    let mut start = 0usize;
    for (index, segment) in output.split_inclusive('\n').enumerate() {
        let end = start + segment.trim_end_matches(['\r', '\n']).len();
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
    cause_range: Option<SourceRange>,
) -> Diagnostic {
    let statement_range = syntax_source_range(source, SourceRange::new(0, source.len()));
    let statement_line = statement_line
        + source[..statement_range.start]
            .bytes()
            .filter(|byte| *byte == b'\n')
            .count();
    Diagnostic {
        rule_id: "format.statement_skipped".into(),
        severity: match policy {
            super::UnsupportedPolicy::Skip => Severity::Warning,
            super::UnsupportedPolicy::Error => Severity::Error,
        },
        message: format!("statement formatting skipped at line {statement_line}: {error}"),
        source_range: cause_range.unwrap_or(statement_range),
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
        FormatDiagnostic::UnsupportedSyntax { start, end, .. } => {
            syntax_source_range(source, SourceRange::new(*start, *end))
        }
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

/// Parser statement ranges can include attached leading comments. Diagnostic
/// fallbacks point to SQL syntax without changing the statement's rewrite span.
/// Keep the original range if scanner provenance is unavailable.
fn syntax_source_range(source: &str, range: SourceRange) -> SourceRange {
    source
        .get(range.start..range.end)
        .and_then(|slice| tokenize(slice).ok())
        .and_then(|tokens| {
            tokens
                .iter()
                .find(|token| !token.is_comment())
                .map(|token| token.start)
        })
        .map_or(range, |offset| {
            SourceRange::new(range.start + offset, range.end)
        })
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
        .chars()
        .any(super::tokens::is_removable_trailing_whitespace)
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

#[cfg(test)]
mod statement_skip_tests {
    use super::*;

    #[test]
    fn statement_skip_uses_a_trusted_cause_range() {
        let source = "SELECT 1; -- changed";
        let cause = SourceRange::new(10, source.len());
        let diagnostic = statement_skipped_diagnostic(
            source,
            &FormatDiagnostic::ProtectedTokenChanged("comment changed".into()),
            super::super::UnsupportedPolicy::Skip,
            1,
            Some(cause),
        );

        assert_eq!(diagnostic.source_range, cause);
        assert!(!diagnostic.fix_available);
    }
}
