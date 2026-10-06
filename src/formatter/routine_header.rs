use pg_query::protobuf::{CreateFunctionStmt, FunctionParameterMode, Token, node::Node};

use super::{FormatDiagnostic, FormatOptions, structure::TokenStructure, tokens::tokenize};

/// A declaration capability separate from both SQL and procedural body IRs.
pub(super) fn format(
    source: &str,
    statement: &CreateFunctionStmt,
    options: &FormatOptions,
) -> Result<String, FormatDiagnostic> {
    let tokens = tokenize(source)?;
    let structure = TokenStructure::new(&tokens);
    let kind_location = super::procedural::routine_kind_location(source, statement.is_procedure)?;
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
    if let Some(location) = super::procedural::routine_returns_location(source, statement)? {
        if let Some(index) = tokens.iter().position(|token| token.start == location) {
            clauses.push(index);
        }
    }
    super::semantic_block::format_routine_header(source, &lists, &clauses, options)
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
