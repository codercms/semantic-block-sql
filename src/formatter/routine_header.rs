use pg_query::protobuf::{CreateFunctionStmt, FunctionParameterMode, Token, node::Node};

use super::{FormatDiagnostic, FormatOptions, structure::TokenStructure, tokens::tokenize};

/// A declaration capability separate from both SQL and procedural body IRs.
pub(super) fn format(
    source: &str,
    statement: &CreateFunctionStmt,
    options: &FormatOptions,
) -> Result<String, FormatDiagnostic> {
    format_bound(source, statement, options, &[])
}

pub(super) fn format_external(
    source: &str,
    statement: &CreateFunctionStmt,
    options: &FormatOptions,
    literal_starts: &[usize],
) -> Result<String, FormatDiagnostic> {
    format_bound(source, statement, options, literal_starts)
}

fn format_bound(
    source: &str,
    statement: &CreateFunctionStmt,
    options: &FormatOptions,
    literal_starts: &[usize],
) -> Result<String, FormatDiagnostic> {
    let tokens = tokenize(source)?;
    let structure = TokenStructure::new(&tokens);
    let kind_location = routine_kind_location(source, statement.is_procedure)?;
    let kind = tokens
        .iter()
        .position(|token| Some(token.start) == kind_location)
        .ok_or_else(|| FormatDiagnostic::Ownership("routine header kind is missing".into()))?;
    let open = (kind + 1..tokens.len())
        .find(|&index| structure.depth(index) == 0 && tokens[index].kind == Token::Ascii40)
        .ok_or_else(|| FormatDiagnostic::Ownership("routine signature is missing".into()))?;
    let close = structure
        .matching_parenthesis(open)
        .ok_or_else(|| FormatDiagnostic::Ownership("routine signature is unclosed".into()))?;
    let mut parameters = 0;
    let mut table_columns = 0;
    for parameter in &statement.parameters {
        let Some(Node::FunctionParameter(parameter)) = parameter.node.as_ref() else {
            return Err(FormatDiagnostic::Ownership(
                "unrecognized routine parameter".into(),
            ));
        };
        if FunctionParameterMode::try_from(parameter.mode)
            == Ok(FunctionParameterMode::FuncParamTable)
        {
            table_columns += 1;
        } else {
            parameters += 1;
        }
    }
    let mut lists = vec![(open, close)];
    verify_list(&tokens, &structure, open, close, parameters)?;
    if table_columns > 0 {
        let table_open = (close + 1..tokens.len())
            .find(|&index| structure.depth(index) == 0 && tokens[index].kind == Token::Ascii40)
            .ok_or_else(|| FormatDiagnostic::Ownership("RETURNS TABLE list is missing".into()))?;
        let table_close = structure
            .matching_parenthesis(table_open)
            .ok_or_else(|| FormatDiagnostic::Ownership("RETURNS TABLE list is unclosed".into()))?;
        verify_list(&tokens, &structure, table_open, table_close, table_columns)?;
        lists.push((table_open, table_close));
    }
    let mut clauses = statement
        .options
        .iter()
        .filter_map(|node| match node.node.as_ref() {
            Some(Node::DefElem(option)) => usize::try_from(option.location).ok(),
            _ => None,
        })
        .filter_map(|location| tokens.iter().position(|token| token.start == location))
        .collect::<Vec<_>>();
    if let Some(location) = routine_returns_location(source, statement)? {
        if let Some(index) = tokens.iter().position(|token| token.start == location) {
            clauses.push(index);
        }
    }
    let literals = literal_starts
        .iter()
        .map(|location| {
            tokens
                .iter()
                .position(|token| token.start == *location && token.kind == Token::Sconst)
                .ok_or_else(|| {
                    FormatDiagnostic::Ownership("external literal token is missing".into())
                })
        })
        .collect::<Result<Vec<_>, _>>()?;
    super::semantic_block::format_routine_header(source, &lists, &clauses, &literals, options)
}

pub(super) fn validate_options(
    statement: &CreateFunctionStmt,
    source: &str,
) -> Result<Option<String>, FormatDiagnostic> {
    let mut language = None;
    for node in &statement.options {
        let Some(Node::DefElem(option)) = node.node.as_ref() else {
            return Err(unsupported(source, "unrecognized routine option"));
        };
        match option.defname.as_str() {
            "language" => language = option_string(option),
            "as" | "volatility" | "strict" | "security" | "leakproof" | "cost" | "rows"
            | "support" | "set" => {}
            "parallel"
                if option_string(option).is_some_and(|value| {
                    matches!(value.as_str(), "safe" | "restricted" | "unsafe")
                }) => {}
            _ => return Err(unsupported(source, "unreviewed routine option")),
        }
    }
    Ok(language)
}

fn unsupported(source: &str, feature: &str) -> FormatDiagnostic {
    FormatDiagnostic::UnsupportedSyntax {
        feature: feature.into(),
        start: 0,
        end: source.len(),
    }
}

fn verify_list(
    tokens: &[super::tokens::SqlToken<'_>],
    structure: &TokenStructure,
    open: usize,
    close: usize,
    expected: usize,
) -> Result<(), FormatDiagnostic> {
    let has_item = tokens[open + 1..close]
        .iter()
        .any(|token| !token.is_comment());
    let count = usize::from(has_item)
        + (open + 1..close)
            .filter(|&index| tokens[index].kind == Token::Ascii44 && structure.depth(index) == 1)
            .count();
    if count != expected {
        return Err(FormatDiagnostic::Ownership(
            "routine list disagrees with its AST parameter count".into(),
        ));
    }
    Ok(())
}

pub(super) fn format_dollar_declaration(
    source: &str,
    options: &FormatOptions,
) -> Result<String, FormatDiagnostic> {
    let parsed = pg_query::parse(source)
        .map_err(|error| FormatDiagnostic::PostgreSqlParse(error.to_string()))?;
    let node = parsed
        .protobuf
        .stmts
        .first()
        .and_then(|raw| raw.stmt.as_deref())
        .and_then(|node| node.node.as_ref());
    let statement = match node {
        Some(Node::CreateFunctionStmt(statement)) => statement,
        Some(Node::DoStmt(_)) => return Ok(source.to_owned()),
        _ => {
            return Err(FormatDiagnostic::Ownership(
                "routine header requires a routine or DO statement".into(),
            ));
        }
    };
    let as_location = statement
        .options
        .iter()
        .find_map(|node| match node.node.as_ref() {
            Some(Node::DefElem(option)) if option.defname == "as" => {
                usize::try_from(option.location).ok()
            }
            _ => None,
        })
        .ok_or_else(|| FormatDiagnostic::Ownership("routine AS location is missing".into()))?;
    let tokens = tokenize(source)?;
    let as_index = tokens
        .iter()
        .position(|token| token.start == as_location && token.kind == Token::As)
        .ok_or_else(|| FormatDiagnostic::Ownership("routine AS token is missing".into()))?;
    let literal = tokens[as_index + 1..]
        .iter()
        .find(|token| !token.is_comment())
        .filter(|token| token.kind == Token::Sconst)
        .ok_or_else(|| FormatDiagnostic::Ownership("routine body literal is missing".into()))?;
    Ok(format!(
        "{}{}",
        format(&source[..literal.start], statement, options)?,
        &source[literal.start..]
    ))
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct OuterTokenOwnership {
    pub language_location: Option<usize>,
    pub routine_kind_location: Option<usize>,
    pub returns_location: Option<usize>,
}

impl OuterTokenOwnership {
    pub fn from_statement(
        source: &str,
        statement: &CreateFunctionStmt,
    ) -> Result<Self, FormatDiagnostic> {
        Ok(Self {
            routine_kind_location: routine_kind_location(source, statement.is_procedure)?,
            returns_location: routine_returns_location(source, statement)?,
            ..Self::from_options(&statement.options)
        })
    }

    pub fn from_options(options: &[pg_query::protobuf::Node]) -> Self {
        Self {
            language_location: options.iter().find_map(|node| match node.node.as_ref() {
                Some(Node::DefElem(option)) if option.defname == "language" => {
                    usize::try_from(option.location).ok()
                }
                _ => None,
            }),
            ..Self::default()
        }
    }

    pub fn within(self, start: usize, end: usize) -> Self {
        Self {
            language_location: self
                .language_location
                .filter(|location| start <= *location && *location < end)
                .map(|location| location - start),
            routine_kind_location: self
                .routine_kind_location
                .filter(|location| start <= *location && *location < end)
                .map(|location| location - start),
            returns_location: self
                .returns_location
                .filter(|location| start <= *location && *location < end)
                .map(|location| location - start),
        }
    }
}

pub(super) fn routine_returns_location(
    source: &str,
    statement: &pg_query::protobuf::CreateFunctionStmt,
) -> Result<Option<usize>, FormatDiagnostic> {
    if statement.is_procedure {
        return Ok(None);
    }
    let Some(return_type) = statement.return_type.as_ref() else {
        return Ok(None);
    };
    let Some(return_type_location) = usize::try_from(return_type.location).ok() else {
        return Ok(None);
    };
    Ok(super::tokens::tokenize(source)?
        .into_iter()
        .filter(|token| {
            token.kind == pg_query::protobuf::Token::Returns && token.start < return_type_location
        })
        .map(|token| token.start)
        .next_back())
}

pub(super) fn routine_kind_location(
    source: &str,
    is_procedure: bool,
) -> Result<Option<usize>, FormatDiagnostic> {
    let expected = if is_procedure {
        pg_query::protobuf::Token::Procedure
    } else {
        pg_query::protobuf::Token::Function
    };
    Ok(super::tokens::tokenize(source)?
        .into_iter()
        .find(|token| token.kind == expected)
        .map(|token| token.start))
}

pub(super) fn option_string(option: &pg_query::protobuf::DefElem) -> Option<String> {
    use pg_query::protobuf::node::Node;
    match option.arg.as_deref()?.node.as_ref()? {
        Node::String(value) => Some(value.sval.to_ascii_lowercase()),
        Node::List(list) if list.items.len() == 1 => match list.items[0].node.as_ref()? {
            Node::String(value) => Some(value.sval.to_ascii_lowercase()),
            _ => None,
        },
        _ => None,
    }
}

pub(super) fn normalize_outer_tokens(
    source: &str,
    options: &FormatOptions,
    ownership: OuterTokenOwnership,
) -> Result<String, FormatDiagnostic> {
    let tokens = super::tokens::tokenize(source)?;
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        output.push_str(&source[cursor..token.start]);
        let actual_language_clause = ownership.language_location == Some(token.start);
        let actual_routine_kind = ownership.routine_kind_location == Some(token.start);
        let actual_returns_clause = ownership.returns_location == Some(token.start);
        let sql_language_name = token.text.eq_ignore_ascii_case("sql")
            && index > 0
            && ownership.language_location == Some(tokens[index - 1].start);
        if actual_language_clause
            || actual_routine_kind
            || actual_returns_clause
            || sql_language_name
        {
            output.push_str(&token.text.to_ascii_uppercase());
        } else {
            output.push_str(&super::semantic_block::render_token(
                &tokens, index, options,
            ));
        }
        cursor = token.end;
    }
    output.push_str(&source[cursor..]);
    Ok(output)
}
