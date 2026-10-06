use pg_query::protobuf::{Token, node::Node};

use super::{FormatDiagnostic, FormatOptions, FormattedSql, StatementFormatError};

#[derive(Clone, Copy)]
enum ExternalLanguage {
    C,
    Internal,
}

pub(super) fn format_single_routine(
    source: &str,
    options: &FormatOptions,
) -> Result<FormattedSql, StatementFormatError> {
    let parsed = pg_query::parse(source)
        .map_err(|error| FormatDiagnostic::PostgreSqlParse(error.to_string()))?;
    let [raw] = parsed.protobuf.stmts.as_slice() else {
        return Err(
            FormatDiagnostic::Ownership("external routine requires one statement".into()).into(),
        );
    };
    let Some(Node::CreateFunctionStmt(statement)) =
        raw.stmt.as_deref().and_then(|node| node.node.as_ref())
    else {
        return Err(FormatDiagnostic::Ownership("external routine is missing".into()).into());
    };
    let language = match super::routine_header::validate_options(statement, source)?.as_deref() {
        Some("c") => ExternalLanguage::C,
        Some("internal") => ExternalLanguage::Internal,
        _ => return Err(unsupported(source, "unreviewed external language").into()),
    };
    if statement.sql_body.is_some() {
        return Err(unsupported(source, "external SQL-standard body").into());
    }
    let bodies = statement
        .options
        .iter()
        .filter_map(|node| match node.node.as_ref() {
            Some(Node::DefElem(option)) if option.defname == "as" => Some(option),
            _ => None,
        })
        .collect::<Vec<_>>();
    let [body] = bodies.as_slice() else {
        return Err(unsupported(source, "external body count").into());
    };
    let Some(Node::List(literals)) = body.arg.as_deref().and_then(|node| node.node.as_ref()) else {
        return Err(unsupported(source, "external literal list").into());
    };
    if !literals
        .items
        .iter()
        .all(|node| matches!(node.node.as_ref(), Some(Node::String(_))))
        || !match language {
            ExternalLanguage::C => (1..=2).contains(&literals.items.len()),
            ExternalLanguage::Internal => literals.items.len() == 1,
        }
    {
        return Err(unsupported(source, "external literal arguments").into());
    }
    let tokens = super::tokens::tokenize(source)?;
    let as_location =
        usize::try_from(body.location).map_err(|_| unsupported(source, "unlocated external AS"))?;
    let as_index = tokens
        .iter()
        .position(|token| token.start == as_location && token.kind == Token::As)
        .ok_or_else(|| FormatDiagnostic::Ownership("external AS differs from AST".into()))?;
    let mut literal_starts = Vec::new();
    let mut index = as_index + 1;
    for position in 0..literals.items.len() {
        while tokens.get(index).is_some_and(|token| token.is_comment()) {
            index += 1;
        }
        let token = tokens
            .get(index)
            .filter(|token| token.kind == Token::Sconst)
            .ok_or_else(|| {
                FormatDiagnostic::Ownership("external argument is not a literal".into())
            })?;
        literal_starts.push(token.start);
        index += 1;
        if position + 1 < literals.items.len() {
            while tokens.get(index).is_some_and(|token| token.is_comment()) {
                index += 1;
            }
            if tokens
                .get(index)
                .is_none_or(|token| token.kind != Token::Ascii44)
            {
                return Err(FormatDiagnostic::Ownership(
                    "external argument comma is missing".into(),
                )
                .into());
            }
            index += 1;
        }
    }
    let header =
        super::routine_header::format_external(source, statement, options, &literal_starts)?;
    let parsed_header = pg_query::parse(&header)
        .map_err(|error| FormatDiagnostic::PostgreSqlParse(error.to_string()))?;
    let Some(Node::CreateFunctionStmt(statement)) = parsed_header
        .protobuf
        .stmts
        .first()
        .and_then(|raw| raw.stmt.as_deref())
        .and_then(|node| node.node.as_ref())
    else {
        return Err(FormatDiagnostic::SemanticMismatch.into());
    };
    let ownership = super::procedural::OuterTokenOwnership {
        language_location: statement
            .options
            .iter()
            .find_map(|node| match node.node.as_ref() {
                Some(Node::DefElem(option)) if option.defname == "language" => {
                    usize::try_from(option.location).ok()
                }
                _ => None,
            }),
        routine_kind_location: super::procedural::routine_kind_location(
            &header,
            statement.is_procedure,
        )?,
        returns_location: super::procedural::routine_returns_location(&header, statement)?,
    };
    let output = super::procedural::normalize_outer_tokens(&header, options, ownership)?;
    super::validation::equivalence::validate_equivalent_located(source, &output)?;
    let warnings = super::semantic_block::validate_hard_width(&output, options)?;
    Ok(FormattedSql {
        changed: output != source,
        output,
        warnings,
        diagnostics: Vec::new(),
    })
}

fn unsupported(source: &str, feature: &str) -> FormatDiagnostic {
    FormatDiagnostic::UnsupportedSyntax {
        feature: feature.into(),
        start: 0,
        end: source.len(),
    }
}
