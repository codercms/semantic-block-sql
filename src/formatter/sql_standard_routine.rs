use pg_query::protobuf::{CreateFunctionStmt, node::Node};

use super::{FormatDiagnostic, FormatOptions, FormattedSql, SemicolonPolicy};

pub(super) fn format_single_routine(
    source: &str,
    statement: &CreateFunctionStmt,
    options: &FormatOptions,
) -> Result<FormattedSql, FormatDiagnostic> {
    validate(statement, source)?;
    let (header_end, footer_start) = body_span(source)?;
    let body = source[header_end..footer_start].trim();
    let mut body_options = options.clone();
    body_options.semicolon_policy = SemicolonPolicy::Preserve;
    let formatted_body = super::format_supported_statement(body, &body_options)?;

    let outer_tokens = super::procedural::OuterTokenOwnership {
        language_location: routine_language_location(statement),
        routine_kind_location: super::procedural::routine_kind_location(
            source,
            statement.is_procedure,
        )?,
    };
    let header = super::procedural::normalize_outer_tokens(
        &source[..header_end],
        options,
        outer_tokens.within(0, header_end),
    )?;
    let footer = super::procedural::normalize_outer_tokens(
        &source[footer_start..],
        options,
        outer_tokens.within(footer_start, source.len()),
    )?;
    let mut output = String::with_capacity(source.len() + formatted_body.output.len());
    output.push_str(header.trim_end());
    output.push('\n');
    for line in formatted_body.output.lines() {
        if !line.is_empty() {
            output.push_str("    ");
        }
        output.push_str(line);
        output.push('\n');
    }
    output.push_str(footer.trim_start());

    let reparsed = pg_query::parse(&output)
        .map_err(|error| FormatDiagnostic::PostgreSqlParse(error.to_string()))?;
    let reparsed_statement = reparsed
        .protobuf
        .stmts
        .first()
        .and_then(|raw| raw.stmt.as_deref())
        .and_then(|node| node.node.as_ref());
    let Some(Node::CreateFunctionStmt(reparsed_statement)) = reparsed_statement else {
        return Err(FormatDiagnostic::SemanticMismatch);
    };
    validate(reparsed_statement, &output)?;
    super::validation::validate_equivalent(source, &output)?;

    Ok(FormattedSql {
        changed: output != source,
        output,
        warnings: Vec::new(),
        diagnostics: Vec::new(),
    })
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

fn validate(statement: &CreateFunctionStmt, source: &str) -> Result<(), FormatDiagnostic> {
    let Some(Node::List(body)) = statement
        .sql_body
        .as_deref()
        .and_then(|node| node.node.as_ref())
    else {
        return Err(unsupported(
            source,
            "unrecognized SQL-standard routine body",
        ));
    };
    if body.items.len() != 1
        || !matches!(
            body.items[0].node.as_ref(),
            Some(Node::List(statement)) if statement.items.len() == 1
        )
    {
        return Err(unsupported(
            source,
            "SQL-standard routine body with multiple statements",
        ));
    }

    let mut language = None;
    for option in &statement.options {
        let Some(Node::DefElem(option)) = option.node.as_ref() else {
            return Err(unsupported(source, "unrecognized SQL routine option"));
        };
        match option.defname.as_str() {
            "language" => language = super::procedural::option_string(option),
            "volatility" | "strict" => {}
            _ => return Err(unsupported(source, "unreviewed SQL routine option")),
        }
    }
    if language.as_deref() != Some("sql") {
        return Err(unsupported(source, "non-SQL standard routine body"));
    }
    Ok(())
}

fn body_span(source: &str) -> Result<(usize, usize), FormatDiagnostic> {
    let tokens = super::tokens::tokenize(source)?;
    let structure = super::structure::TokenStructure::new(&tokens);
    let begin = tokens
        .windows(2)
        .enumerate()
        .find(|(index, pair)| {
            structure.depth(*index) == 0
                && pair[0].text.eq_ignore_ascii_case("begin")
                && pair[1].text.eq_ignore_ascii_case("atomic")
        })
        .map(|(index, _)| index)
        .ok_or_else(|| unsupported(source, "SQL routine without BEGIN ATOMIC"))?;
    let end = (begin + 2..tokens.len())
        .rev()
        .find(|index| {
            structure.depth(*index) == 0 && tokens[*index].text.eq_ignore_ascii_case("end")
        })
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
