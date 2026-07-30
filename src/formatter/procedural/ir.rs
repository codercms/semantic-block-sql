use super::super::tokens::{SqlToken, tokenize};
use super::super::{FormatDiagnostic, SourceRange};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RoutineBody<'a> {
    pub source: &'a str,
    pub newline: &'static str,
    pub nodes: Vec<BodyNode<'a>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct BodyNode<'a> {
    pub kind: BodyNodeKind,
    pub text: &'a str,
    pub range: SourceRange,
    pub blank_before: bool,
    pub trailing_comment: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BodyNodeKind {
    Label,
    Declare,
    Begin,
    Exception,
    If,
    Elsif,
    Else,
    EndIf,
    Loop,
    EndLoop,
    Case,
    When,
    EndCase,
    EndBlock,
    Declaration,
    Sql,
    Assignment,
    Perform,
    Return,
    ReturnNext,
    ReturnQuery,
    Assert,
    Raise,
    Diagnostics,
    DynamicExecute,
    Cursor,
    Exit,
    Continue,
    Opaque,
    Comment,
}

impl BodyNodeKind {
    pub(super) fn is_opaque(self) -> bool {
        self == Self::Opaque
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ParserNodeKind {
    Container,
    Statement,
    Assert,
    ReturnQuery,
    Opaque,
    Expression,
    Datum,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ParserRoutine {
    pub nodes: Vec<ParserNodeKind>,
}

pub(super) fn adapt_parser(value: &serde_json::Value) -> Result<ParserRoutine, FormatDiagnostic> {
    let mut nodes = Vec::new();
    adapt_value(value, &mut nodes)?;
    Ok(ParserRoutine { nodes })
}

pub(super) fn validate_parser_alignment(
    body: &RoutineBody<'_>,
    parser: &ParserRoutine,
) -> Result<(), FormatDiagnostic> {
    let lexical_asserts = body
        .nodes
        .iter()
        .filter(|node| node.kind == BodyNodeKind::Assert)
        .count();
    let lexical_return_queries = body
        .nodes
        .iter()
        .filter(|node| node.kind == BodyNodeKind::ReturnQuery)
        .count();
    let lexical_opaque = body
        .nodes
        .iter()
        .filter(|node| node.kind == BodyNodeKind::Opaque)
        .count();
    let parser_asserts = parser
        .nodes
        .iter()
        .filter(|kind| **kind == ParserNodeKind::Assert)
        .count();
    let parser_return_queries = parser
        .nodes
        .iter()
        .filter(|kind| **kind == ParserNodeKind::ReturnQuery)
        .count();
    let parser_opaque = parser
        .nodes
        .iter()
        .filter(|kind| **kind == ParserNodeKind::Opaque)
        .count();
    if (lexical_asserts, lexical_return_queries, lexical_opaque)
        != (parser_asserts, parser_return_queries, parser_opaque)
    {
        return Err(FormatDiagnostic::Ownership(
            "PL/pgSQL parser model and source-span IR disagree".into(),
        ));
    }
    Ok(())
}

fn adapt_value(
    value: &serde_json::Value,
    nodes: &mut Vec<ParserNodeKind>,
) -> Result<(), FormatDiagnostic> {
    match value {
        serde_json::Value::Object(fields) => {
            for (name, child) in fields {
                if name.starts_with("PLpgSQL_") {
                    nodes.push(classify_parser_node(name)?);
                }
                adapt_value(child, nodes)?;
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                adapt_value(item, nodes)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn classify_parser_node(name: &str) -> Result<ParserNodeKind, FormatDiagnostic> {
    let kind = match name {
        "PLpgSQL_function"
        | "PLpgSQL_stmt_block"
        | "PLpgSQL_exception_block"
        | "PLpgSQL_exception"
        | "PLpgSQL_condition"
        | "PLpgSQL_case_when" => ParserNodeKind::Container,
        "PLpgSQL_stmt_assert" => ParserNodeKind::Assert,
        "PLpgSQL_stmt_return_query" => ParserNodeKind::ReturnQuery,
        "PLpgSQL_stmt_commit" | "PLpgSQL_stmt_rollback" => ParserNodeKind::Opaque,
        "PLpgSQL_expr" => ParserNodeKind::Expression,
        "PLpgSQL_var" | "PLpgSQL_type" | "PLpgSQL_rec" | "PLpgSQL_recfield" | "PLpgSQL_row"
        | "PLpgSQL_diag_item" => ParserNodeKind::Datum,
        "PLpgSQL_stmt_execsql"
        | "PLpgSQL_stmt_perform"
        | "PLpgSQL_stmt_return"
        | "PLpgSQL_stmt_if"
        | "PLpgSQL_stmt_assign"
        | "PLpgSQL_stmt_raise"
        | "PLpgSQL_stmt_getdiag"
        | "PLpgSQL_stmt_loop"
        | "PLpgSQL_stmt_while"
        | "PLpgSQL_stmt_fori"
        | "PLpgSQL_stmt_fors"
        | "PLpgSQL_stmt_forc"
        | "PLpgSQL_stmt_foreach_a"
        | "PLpgSQL_stmt_exit"
        | "PLpgSQL_stmt_case"
        | "PLpgSQL_stmt_dynexecute"
        | "PLpgSQL_stmt_open"
        | "PLpgSQL_stmt_fetch"
        | "PLpgSQL_stmt_close" => ParserNodeKind::Statement,
        _ => {
            return Err(FormatDiagnostic::UnsupportedSyntax {
                feature: format!("PL/pgSQL node {name}"),
                start: 0,
                end: 0,
            });
        }
    };
    Ok(kind)
}

pub(super) fn parse(source: &str) -> Result<RoutineBody<'_>, FormatDiagnostic> {
    let tokens = tokenize(source)?;
    let newline = if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let mut nodes = Vec::new();
    let mut index = 0usize;
    let mut in_declare = false;

    while index < tokens.len() {
        if tokens[index].is_comment() {
            let token = &tokens[index];
            nodes.push(BodyNode {
                kind: BodyNodeKind::Comment,
                text: token.text,
                range: SourceRange::new(token.start, token.end),
                blank_before: token.line_breaks_before >= 2,
                trailing_comment: None,
            });
            index += 1;
            continue;
        }

        let start_index = index;
        let first = upper(&tokens[index]);
        let (kind, end_index) = match first.as_str() {
            "DECLARE" => {
                in_declare = true;
                (BodyNodeKind::Declare, index)
            }
            "BEGIN" => {
                in_declare = false;
                (BodyNodeKind::Begin, index)
            }
            "EXCEPTION" => (BodyNodeKind::Exception, index),
            "IF" => (BodyNodeKind::If, find_keyword(&tokens, index, "THEN")?),
            "ELSIF" => (BodyNodeKind::Elsif, find_keyword(&tokens, index, "THEN")?),
            "ELSE" => (BodyNodeKind::Else, index),
            "WHILE" | "FOR" | "FOREACH" => {
                (BodyNodeKind::Loop, find_keyword(&tokens, index, "LOOP")?)
            }
            "LOOP" => (BodyNodeKind::Loop, index),
            "CASE" => {
                let next = find_keyword(&tokens, index, "WHEN")?;
                (BodyNodeKind::Case, next.saturating_sub(1).max(index))
            }
            "WHEN" => (BodyNodeKind::When, find_keyword(&tokens, index, "THEN")?),
            "END" => {
                let semi = find_semicolon(&tokens, index)?;
                let second = tokens.get(index + 1).map(upper);
                let kind = match second.as_deref() {
                    Some("IF") => BodyNodeKind::EndIf,
                    Some("LOOP") => BodyNodeKind::EndLoop,
                    Some("CASE") => BodyNodeKind::EndCase,
                    _ => BodyNodeKind::EndBlock,
                };
                (kind, semi)
            }
            _ if is_label_start(&tokens, index) => {
                (BodyNodeKind::Label, find_label_end(&tokens, index)?)
            }
            _ => {
                let semi = find_semicolon(&tokens, index)?;
                (classify_statement(&tokens, index, semi, in_declare), semi)
            }
        };

        let start = tokens[start_index].start;
        let end = tokens[end_index].end;
        let mut next = end_index + 1;
        let trailing_comment = if let Some(comment) = tokens.get(next)
            && comment.is_comment()
            && comment.line_breaks_before == 0
        {
            next += 1;
            Some(comment.text)
        } else {
            None
        };
        nodes.push(BodyNode {
            kind,
            text: &source[start..end],
            range: SourceRange::new(start, end),
            blank_before: tokens[start_index].line_breaks_before >= 2,
            trailing_comment,
        });
        index = next;
    }

    Ok(RoutineBody {
        source,
        newline,
        nodes,
    })
}

fn classify_statement(
    tokens: &[SqlToken<'_>],
    start: usize,
    end: usize,
    in_declare: bool,
) -> BodyNodeKind {
    if in_declare {
        return BodyNodeKind::Declaration;
    }
    let first = upper(&tokens[start]);
    let second = tokens.get(start + 1).map(upper);
    match first.as_str() {
        "SELECT" | "INSERT" | "UPDATE" | "DELETE" | "MERGE" | "WITH" => BodyNodeKind::Sql,
        "PERFORM" => BodyNodeKind::Perform,
        "RETURN" if second.as_deref() == Some("NEXT") => BodyNodeKind::ReturnNext,
        "RETURN" if second.as_deref() == Some("QUERY") => BodyNodeKind::ReturnQuery,
        "RETURN" => BodyNodeKind::Return,
        "ASSERT" => BodyNodeKind::Assert,
        "RAISE" => BodyNodeKind::Raise,
        "GET" => BodyNodeKind::Diagnostics,
        "EXECUTE" => BodyNodeKind::DynamicExecute,
        "OPEN" | "FETCH" | "MOVE" | "CLOSE" => BodyNodeKind::Cursor,
        "EXIT" => BodyNodeKind::Exit,
        "CONTINUE" => BodyNodeKind::Continue,
        "COMMIT" | "ROLLBACK" => BodyNodeKind::Opaque,
        _ if contains_assignment(tokens, start, end) => BodyNodeKind::Assignment,
        _ => BodyNodeKind::Opaque,
    }
}

fn contains_assignment(tokens: &[SqlToken<'_>], start: usize, end: usize) -> bool {
    (start..=end).any(|index| tokens[index].text == ":=")
}

fn find_keyword(
    tokens: &[SqlToken<'_>],
    start: usize,
    keyword: &str,
) -> Result<usize, FormatDiagnostic> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(start + 1) {
        match token.text {
            "(" | "[" => depth += 1,
            ")" | "]" => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth == 0 && upper(token) == keyword {
            return Ok(index);
        }
    }
    Err(FormatDiagnostic::Ownership(format!(
        "PL/pgSQL header has no {keyword} boundary"
    )))
}

fn find_semicolon(tokens: &[SqlToken<'_>], start: usize) -> Result<usize, FormatDiagnostic> {
    let mut depth = 0usize;
    for (index, token) in tokens.iter().enumerate().skip(start) {
        match token.text {
            "(" | "[" => depth += 1,
            ")" | "]" => depth = depth.saturating_sub(1),
            ";" if depth == 0 => return Ok(index),
            _ => {}
        }
    }
    Err(FormatDiagnostic::Ownership(
        "PL/pgSQL statement has no terminal semicolon".into(),
    ))
}

fn is_label_start(tokens: &[SqlToken<'_>], index: usize) -> bool {
    tokens.get(index).is_some_and(|token| token.text == "<<")
        || (tokens.get(index).is_some_and(|token| token.text == "<")
            && tokens.get(index + 1).is_some_and(|token| token.text == "<"))
}

fn find_label_end(tokens: &[SqlToken<'_>], start: usize) -> Result<usize, FormatDiagnostic> {
    for index in start + 1..tokens.len() {
        if tokens[index].text == ">>"
            || (tokens[index].text == ">"
                && tokens.get(index + 1).is_some_and(|token| token.text == ">"))
        {
            return Ok(if tokens[index].text == ">>" {
                index
            } else {
                index + 1
            });
        }
    }
    Err(FormatDiagnostic::Ownership(
        "PL/pgSQL label is not closed".into(),
    ))
}

fn upper(token: &SqlToken<'_>) -> String {
    token.text.to_ascii_uppercase()
}
