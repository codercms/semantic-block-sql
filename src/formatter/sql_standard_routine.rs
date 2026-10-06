use pg_query::protobuf::{CreateFunctionStmt, Token, node::Node};

use super::{FormatDiagnostic, FormatOptions, FormattedSql, SemicolonPolicy, StatementFormatError};

#[derive(Debug, Clone, Copy)]
enum BodyStatementKind {
    Sql,
    Return,
}

#[derive(Debug)]
enum BodySpec {
    Atomic(Vec<BodyStatementKind>),
    Return { after_options: usize },
    Dollar { as_location: usize, value: String },
}

#[derive(Debug)]
struct BoundBody {
    header_end: usize,
    start: usize,
    end: usize,
    footer_start: usize,
    statements: Vec<(usize, usize, BodyStatementKind)>,
    atomic: bool,
    leading_lines: usize,
}

pub(super) fn format_single_routine(
    source: &str,
    options: &FormatOptions,
) -> Result<FormattedSql, StatementFormatError> {
    // Input AST locations are relative to the enclosing document. Reparse the
    // owning statement so every body/header capability has the same frame as
    // the source being bound; do not patch individual protobuf locations.
    let parsed = pg_query::parse(source)
        .map_err(|error| FormatDiagnostic::PostgreSqlParse(error.to_string()))?;
    let [raw] = parsed.protobuf.stmts.as_slice() else {
        return Err(
            FormatDiagnostic::Ownership("SQL routine requires one statement".into()).into(),
        );
    };
    let Some(Node::CreateFunctionStmt(statement)) =
        raw.stmt.as_deref().and_then(|node| node.node.as_ref())
    else {
        return Err(
            FormatDiagnostic::Ownership("SQL routine declaration is missing".into()).into(),
        );
    };
    let spec = validate(statement, source)?;
    if let BodySpec::Dollar { as_location, value } = &spec {
        return format_dollar_body(source, statement, *as_location, value, options);
    }
    let body = bind_body(source, &spec)?;
    let mut body_options = options.clone();
    body_options.semicolon_policy = SemicolonPolicy::Preserve;
    if body.atomic {
        body_options.soft_line_width = options.soft_line_width.saturating_sub(4).max(1);
        body_options.hard_line_width = options
            .hard_line_width
            .saturating_sub(4)
            .max(body_options.soft_line_width);
    }
    let mut formatted_body = String::new();
    let mut cursor = body.start;
    for &(start, end, kind) in &body.statements {
        let gap = super::normalize_document_gap(&source[cursor..start], true);
        if cursor > body.start && gap.is_empty() {
            formatted_body.push('\n');
        }
        formatted_body.push_str(&gap);
        let formatted = format_body_statement(&source[start..end], kind, &body_options)
            .map_err(|error| error.shifted(start))?;
        formatted_body.push_str(&formatted.output);
        cursor = end;
    }
    formatted_body.push_str(&super::normalize_document_gap(
        &source[cursor..body.end],
        false,
    ));

    let outer_tokens = super::procedural::OuterTokenOwnership {
        language_location: routine_language_location(statement),
        routine_kind_location: super::procedural::routine_kind_location(
            source,
            statement.is_procedure,
        )?,
        returns_location: super::procedural::routine_returns_location(source, statement)?,
    };
    let header = super::procedural::normalize_outer_tokens(
        &source[..body.header_end],
        options,
        outer_tokens.within(0, body.header_end),
    )?;
    let footer = super::procedural::normalize_outer_tokens(
        &source[body.footer_start..],
        options,
        outer_tokens.within(body.footer_start, source.len()),
    )?;
    let mut output = header.trim_end().to_owned();
    if body.atomic {
        output.push('\n');
        for line in formatted_body
            .strip_prefix('\n')
            .unwrap_or(&formatted_body)
            .lines()
        {
            if !line.is_empty() {
                output.push_str("    ");
            }
            output.push_str(line);
            output.push('\n');
        }
        output.push_str(footer.trim_start());
    } else {
        let header_width = output.lines().last().map_or(0, |line| line.chars().count());
        if body.leading_lines == 0
            && !formatted_body.contains('\n')
            && header_width + 1 + formatted_body.chars().count() <= options.soft_line_width
        {
            output.push(' ');
        } else {
            output.extend(std::iter::repeat_n('\n', body.leading_lines.max(1)));
        }
        output.push_str(formatted_body.trim());
        output.push_str(&footer);
    }
    let reparsed = pg_query::parse(&output)
        .map_err(|error| FormatDiagnostic::PostgreSqlParse(error.to_string()))?;
    let Some(Node::CreateFunctionStmt(reparsed_statement)) = reparsed
        .protobuf
        .stmts
        .first()
        .and_then(|raw| raw.stmt.as_deref())
        .and_then(|node| node.node.as_ref())
    else {
        return Err(FormatDiagnostic::SemanticMismatch.into());
    };
    validate(reparsed_statement, &output)?;
    super::validation::equivalence::validate_equivalent_located(source, &output)?;
    let warnings = super::semantic_block::validate_hard_width(&output, options)?;
    Ok(FormattedSql {
        changed: output != source,
        output,
        warnings,
        diagnostics: Vec::new(),
    })
}

struct DollarBody<'a> {
    literal_start: usize,
    literal_end: usize,
    start: usize,
    end: usize,
    delimiter: &'a str,
}

fn bind_dollar_body<'a>(
    source: &'a str,
    as_location: usize,
    value: &str,
) -> Result<DollarBody<'a>, FormatDiagnostic> {
    let tokens = super::tokens::tokenize(source)?;
    let as_index = tokens
        .iter()
        .position(|token| token.start == as_location && token.kind == Token::As)
        .ok_or_else(|| {
            FormatDiagnostic::Ownership("SQL AS clause disagrees with the AST".into())
        })?;
    let literal = tokens[as_index + 1..]
        .iter()
        .find(|token| !token.is_comment())
        .filter(|token| token.kind == Token::Sconst && token.text.starts_with('$'))
        .ok_or_else(|| unsupported(source, "SQL routine body outside reviewed dollar quoting"))?;
    let delimiter_end = literal.text[1..]
        .find('$')
        .map(|index| index + 2)
        .ok_or_else(|| FormatDiagnostic::Ownership("SQL body delimiter is missing".into()))?;
    let delimiter = &literal.text[..delimiter_end];
    if literal.text.len() < 2 * delimiter.len() || !literal.text.ends_with(delimiter) {
        return Err(FormatDiagnostic::Ownership(
            "SQL body delimiter is unclosed".into(),
        ));
    }
    let start = literal.start + delimiter.len();
    let end = literal.end - delimiter.len();
    if &source[start..end] != value {
        return Err(FormatDiagnostic::Ownership(
            "SQL body literal disagrees with its decoded AST value".into(),
        ));
    }
    Ok(DollarBody {
        literal_start: literal.start,
        literal_end: literal.end,
        start,
        end,
        delimiter,
    })
}

fn format_dollar_body(
    source: &str,
    statement: &CreateFunctionStmt,
    as_location: usize,
    value: &str,
    options: &FormatOptions,
) -> Result<FormattedSql, StatementFormatError> {
    let body = bind_dollar_body(source, as_location, value)?;
    let raw_body = &source[body.start..body.end];
    let parsed = pg_query::parse(raw_body)
        .map_err(|error| FormatDiagnostic::PostgreSqlParse(error.to_string()))?;
    if parsed.protobuf.stmts.is_empty() {
        return Err(unsupported(source, "empty SQL routine body").into());
    }
    let mut body_options = options.clone();
    body_options.semicolon_policy = SemicolonPolicy::Preserve;
    body_options.soft_line_width = options.soft_line_width.saturating_sub(4).max(1);
    body_options.hard_line_width = options
        .hard_line_width
        .saturating_sub(4)
        .max(body_options.soft_line_width);
    let mut formatted_body = String::new();
    let mut cursor = 0;
    for raw in &parsed.protobuf.stmts {
        let (start, end) = super::statement_span(raw_body, raw);
        let gap = super::normalize_document_gap(&raw_body[cursor..start], true);
        if cursor > 0 && gap.is_empty() {
            formatted_body.push('\n');
        }
        formatted_body.push_str(&gap);
        let formatted = super::format_supported_statement(&raw_body[start..end], &body_options)
            .map_err(|error| error.shifted(body.start + start))?;
        formatted_body.push_str(&formatted.output);
        cursor = end;
    }
    formatted_body.push_str(&super::normalize_document_gap(&raw_body[cursor..], true));
    let framed = indent_sql_body(formatted_body.strip_prefix('\n').unwrap_or(&formatted_body))?;
    super::semantic_block::validate_hard_width(&framed, options)?;
    super::validation::equivalence::validate_equivalent_located(raw_body, &framed)
        .map_err(|error| StatementFormatError::from(error).shifted(body.start))?;
    let outer = super::procedural::OuterTokenOwnership {
        language_location: routine_language_location(statement),
        routine_kind_location: super::procedural::routine_kind_location(
            source,
            statement.is_procedure,
        )?,
        returns_location: super::procedural::routine_returns_location(source, statement)?,
    };
    let header = super::procedural::normalize_outer_tokens(
        &source[..body.literal_start],
        options,
        outer.within(0, body.literal_start),
    )?;
    let footer = super::procedural::normalize_outer_tokens(
        &source[body.literal_end..],
        options,
        outer.within(body.literal_end, source.len()),
    )?;
    // Seal the unchanged declaration independently of the separately validated
    // embedded SQL. Never globally exempt routine literal contents from safety.
    let declaration = format!(
        "{header}{}{footer}",
        &source[body.literal_start..body.literal_end]
    );
    super::validation::equivalence::validate_equivalent_located(source, &declaration)?;
    let output = format!(
        "{header}{}\n{framed}{}{footer}",
        body.delimiter, body.delimiter
    );
    let reparsed = pg_query::parse(&output)
        .map_err(|error| FormatDiagnostic::PostgreSqlParse(error.to_string()))?;
    let Some(Node::CreateFunctionStmt(statement)) = reparsed.protobuf.stmts[0]
        .stmt
        .as_deref()
        .and_then(|node| node.node.as_ref())
    else {
        return Err(FormatDiagnostic::SemanticMismatch.into());
    };
    let BodySpec::Dollar { as_location, value } = validate(statement, &output)? else {
        return Err(FormatDiagnostic::SemanticMismatch.into());
    };
    bind_dollar_body(&output, as_location, &value)?;
    let warnings = super::semantic_block::validate_hard_width(&output, options)?;
    Ok(FormattedSql {
        changed: output != source,
        output,
        warnings,
        diagnostics: Vec::new(),
    })
}

/// Indent SQL trivia, never the continuation bytes of a multiline token.
fn indent_sql_body(source: &str) -> Result<String, FormatDiagnostic> {
    let tokens = super::tokens::tokenize(source)?;
    let mut output = String::new();
    let mut offset = 0;
    for line in source.split_inclusive('\n') {
        let continued_token = tokens
            .iter()
            .any(|token| token.start < offset && offset < token.end);
        if !line.trim().is_empty() && !continued_token {
            output.push_str("    ");
        }
        output.push_str(line);
        offset += line.len();
    }
    if !output.ends_with('\n') {
        output.push('\n');
    }
    Ok(output)
}

fn format_body_statement(
    source: &str,
    kind: BodyStatementKind,
    options: &FormatOptions,
) -> Result<FormattedSql, StatementFormatError> {
    match kind {
        BodyStatementKind::Sql => super::format_supported_statement(source, options),
        BodyStatementKind::Return => {
            // RETURN and SELECT have equal-length prefixes. A parser-owned
            // expression can use the canonical SELECT target adapter without
            // moving its diagnostic byte locations or changing protected text.
            let tokens = super::tokens::tokenize(source)?;
            let keyword = tokens
                .iter()
                .find(|token| !token.is_comment())
                .filter(|token| token.kind == Token::Return)
                .ok_or_else(|| {
                    FormatDiagnostic::Ownership("SQL RETURN has no owned keyword".into())
                })?;
            let mut adapted = source.to_owned();
            adapted.replace_range(keyword.start..keyword.end, "SELECT");
            let mut formatted = super::format_supported_statement(&adapted, options)?;
            let tokens = super::tokens::tokenize(&formatted.output)?;
            let keyword = tokens
                .iter()
                .find(|token| !token.is_comment())
                .filter(|token| token.kind == Token::Select)
                .ok_or_else(|| {
                    FormatDiagnostic::Ownership("SQL RETURN adapter lost SELECT".into())
                })?;
            let range = keyword.start..keyword.end;
            formatted.output.replace_range(range, "RETURN");
            formatted.changed = formatted.output != source;
            Ok(formatted)
        }
    }
}

fn routine_language_location(statement: &CreateFunctionStmt) -> Option<usize> {
    statement.options.iter().find_map(|option| {
        let Node::DefElem(option) = option.node.as_ref()? else {
            return None;
        };
        (option.defname == "language")
            .then(|| usize::try_from(option.location).ok())
            .flatten()
    })
}

fn validate(statement: &CreateFunctionStmt, source: &str) -> Result<BodySpec, FormatDiagnostic> {
    let spec = match statement
        .sql_body
        .as_deref()
        .and_then(|node| node.node.as_ref())
    {
        Some(Node::List(body)) => {
            let [body] = body.items.as_slice() else {
                return Err(unsupported(
                    source,
                    "unrecognized SQL-standard routine body",
                ));
            };
            let Some(Node::List(body)) = body.node.as_ref() else {
                return Err(unsupported(
                    source,
                    "unrecognized SQL-standard statement list",
                ));
            };
            if body.items.is_empty() {
                return Err(unsupported(source, "empty SQL-standard routine body"));
            }
            BodySpec::Atomic(
                body.items
                    .iter()
                    .map(|node| body_statement_kind(node.node.as_ref(), source))
                    .collect::<Result<_, _>>()?,
            )
        }
        Some(Node::ReturnStmt(body)) if body.returnval.is_some() => BodySpec::Return {
            after_options: statement
                .options
                .iter()
                .filter_map(|node| {
                    let Node::DefElem(option) = node.node.as_ref()? else {
                        return None;
                    };
                    usize::try_from(option.location).ok()
                })
                .max()
                .unwrap_or(0),
        },
        None => {
            let mut bodies = statement.options.iter().filter_map(|node| {
                let Node::DefElem(option) = node.node.as_ref()? else {
                    return None;
                };
                (option.defname == "as").then_some(option)
            });
            let option = bodies
                .next()
                .ok_or_else(|| unsupported(source, "SQL routine without a body"))?;
            if bodies.next().is_some() {
                return Err(unsupported(source, "multiple SQL routine bodies"));
            }
            let Some(Node::List(body)) = option.arg.as_deref().and_then(|node| node.node.as_ref())
            else {
                return Err(unsupported(source, "unrecognized SQL routine body literal"));
            };
            let [body] = body.items.as_slice() else {
                return Err(unsupported(
                    source,
                    "SQL routine with multiple body literals",
                ));
            };
            let Some(Node::String(value)) = body.node.as_ref() else {
                return Err(unsupported(source, "unrecognized SQL routine body literal"));
            };
            BodySpec::Dollar {
                as_location: usize::try_from(option.location)
                    .map_err(|_| unsupported(source, "unlocated SQL routine body"))?,
                value: value.sval.clone(),
            }
        }
        _ => {
            return Err(unsupported(
                source,
                "unrecognized SQL-standard routine body",
            ));
        }
    };
    let mut language = None;
    for option in &statement.options {
        let Some(Node::DefElem(option)) = option.node.as_ref() else {
            return Err(unsupported(source, "unrecognized SQL routine option"));
        };
        match option.defname.as_str() {
            "language" => language = super::procedural::option_string(option),
            "volatility" | "strict" | "security" | "leakproof" | "cost" | "rows" | "support"
            | "set" => {}
            "as" if matches!(spec, BodySpec::Dollar { .. }) => {}
            "parallel"
                if super::procedural::option_string(option).is_some_and(|value| {
                    matches!(value.as_str(), "safe" | "restricted" | "unsafe")
                }) => {}
            _ => return Err(unsupported(source, "unreviewed SQL routine option")),
        }
    }
    if language.as_deref() != Some("sql") {
        return Err(unsupported(source, "non-SQL standard routine body"));
    }
    Ok(spec)
}

fn body_statement_kind(
    node: Option<&Node>,
    source: &str,
) -> Result<BodyStatementKind, FormatDiagnostic> {
    match node {
        Some(Node::ReturnStmt(statement)) if statement.returnval.is_some() => {
            Ok(BodyStatementKind::Return)
        }
        Some(
            Node::SelectStmt(_)
            | Node::InsertStmt(_)
            | Node::UpdateStmt(_)
            | Node::DeleteStmt(_)
            | Node::MergeStmt(_),
        ) => Ok(BodyStatementKind::Sql),
        _ => Err(unsupported(
            source,
            "unreviewed SQL-standard body statement",
        )),
    }
}

fn bind_body(source: &str, spec: &BodySpec) -> Result<BoundBody, FormatDiagnostic> {
    let tokens = super::tokens::tokenize(source)?;
    let structure = super::structure::TokenStructure::new(&tokens);
    match spec {
        BodySpec::Atomic(kinds) => {
            let (header_end, footer_start) = body_span(source)?;
            let semicolons = tokens
                .iter()
                .enumerate()
                .filter(|(index, token)| {
                    structure.depth(*index) == 0
                        && token.kind == Token::Ascii59
                        && header_end <= token.start
                        && token.end <= footer_start
                })
                .map(|(_, token)| token.end)
                .collect::<Vec<_>>();
            if semicolons.len() != kinds.len() {
                return Err(FormatDiagnostic::Ownership(
                    "SQL body statement cardinality disagrees with its AST".into(),
                ));
            }
            let mut statements = Vec::new();
            let mut cursor = header_end;
            for (index, (end, kind)) in semicolons.into_iter().zip(kinds).enumerate() {
                let end = if index + 1 == kinds.len() {
                    footer_start
                } else {
                    end
                };
                let start =
                    cursor + source[cursor..end].len() - source[cursor..end].trim_start().len();
                statements.push((start, end, *kind));
                cursor = end;
            }
            Ok(BoundBody {
                header_end,
                start: header_end,
                end: footer_start,
                footer_start,
                statements,
                atomic: true,
                leading_lines: 0,
            })
        }
        BodySpec::Dollar { .. } => Err(FormatDiagnostic::Ownership(
            "dollar SQL body requires its literal binder".into(),
        )),
        BodySpec::Return { after_options } => {
            let keyword = tokens
                .iter()
                .enumerate()
                .find(|(index, token)| {
                    structure.depth(*index) == 0
                        && token.kind == Token::Return
                        && token.start > *after_options
                })
                .map(|(_, token)| token)
                .ok_or_else(|| {
                    FormatDiagnostic::Ownership("SQL routine has no RETURN body".into())
                })?;
            Ok(BoundBody {
                header_end: keyword.start,
                start: keyword.start,
                end: source.len(),
                footer_start: source.len(),
                statements: vec![(keyword.start, source.len(), BodyStatementKind::Return)],
                atomic: false,
                leading_lines: keyword.line_breaks_before,
            })
        }
    }
}

fn body_span(source: &str) -> Result<(usize, usize), FormatDiagnostic> {
    let tokens = super::tokens::tokenize(source)?;
    let structure = super::structure::TokenStructure::new(&tokens);
    let begin = tokens
        .windows(2)
        .enumerate()
        .find(|(index, pair)| {
            structure.depth(*index) == 0
                && pair[0].kind == Token::BeginP
                && pair[1].kind == Token::Atomic
        })
        .map(|(index, _)| index)
        .ok_or_else(|| unsupported(source, "SQL routine without BEGIN ATOMIC"))?;
    let end = (begin + 2..tokens.len())
        .rev()
        .find(|index| structure.depth(*index) == 0 && tokens[*index].kind == Token::EndP)
        .ok_or_else(|| unsupported(source, "SQL routine without END"))?;
    Ok((tokens[begin + 1].end, tokens[end].start))
}

fn unsupported(source: &str, feature: impl Into<String>) -> FormatDiagnostic {
    FormatDiagnostic::UnsupportedSyntax {
        feature: feature.into(),
        start: 0,
        end: source.len(),
    }
}
