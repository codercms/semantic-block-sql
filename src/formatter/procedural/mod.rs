mod ir;
mod layout;

use serde_json::Value;

use super::{FormatDiagnostic, FormatOptions, FormattedSql};

pub(super) fn format_single_routine(
    source: &str,
    options: &FormatOptions,
) -> Result<FormattedSql, FormatDiagnostic> {
    let _ = validate_outer(source)?;
    let parsed = pg_query::parse_plpgsql(source)
        .map_err(|error| FormatDiagnostic::PostgreSqlParse(error.to_string()))?;
    let parser_model = ir::adapt_parser(&parsed)?;

    let (open_start, open_end, close_start, close_end) = dollar_body_span(source)?;
    let body = &source[open_end..close_start];
    let body_ir = ir::parse(body)?;
    ir::validate_parser_alignment(&body_ir, &parser_model)?;
    let formatted_body = layout::format(&body_ir, options)?;
    let mut output = String::with_capacity(source.len() + formatted_body.output.len());
    output.push_str(&source[..open_start]);
    output.push_str(&source[open_start..open_end]);
    output.push_str(&formatted_body.output);
    output.push_str(&source[close_start..close_end]);
    output.push_str(&source[close_end..]);
    let outer_tokens = validate_outer(&output)?;
    let output = normalize_outer_tokens(&output, options, outer_tokens)?;

    validate_outer(&output)?;
    let reparsed = pg_query::parse_plpgsql(&output)
        .map_err(|error| FormatDiagnostic::PostgreSqlParse(error.to_string()))?;
    let reparsed_model = ir::adapt_parser(&reparsed)?;
    validate_plpgsql_equivalent(&parsed, &reparsed)?;
    if parser_model != reparsed_model {
        return Err(FormatDiagnostic::SemanticMismatch);
    }

    let second_body = {
        let (_, second_open, second_close, _) = dollar_body_span(&output)?;
        let second_ir = ir::parse(&output[second_open..second_close])?;
        ir::validate_parser_alignment(&second_ir, &reparsed_model)?;
        layout::format(&second_ir, options)?
    };
    if second_body.output != formatted_body.output {
        return Err(FormatDiagnostic::NotIdempotent);
    }

    Ok(FormattedSql {
        changed: output != source,
        output,
        warnings: Vec::new(),
        diagnostics: formatted_body
            .diagnostics
            .into_iter()
            .map(|diagnostic| diagnostic.shifted(open_end))
            .collect(),
    })
}

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct OuterTokenOwnership {
    pub language_location: Option<usize>,
    pub routine_kind_location: Option<usize>,
    pub returns_location: Option<usize>,
}

impl OuterTokenOwnership {
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

fn validate_outer(source: &str) -> Result<OuterTokenOwnership, FormatDiagnostic> {
    let parsed = pg_query::parse(source)
        .map_err(|error| FormatDiagnostic::PostgreSqlParse(error.to_string()))?;
    if parsed.protobuf.stmts.len() != 1 {
        return Err(unsupported(
            source,
            "multiple statements containing a routine",
        ));
    }
    let node = parsed.protobuf.stmts[0]
        .stmt
        .as_deref()
        .and_then(|node| node.node.as_ref())
        .ok_or_else(|| unsupported(source, "empty routine statement"))?;
    use pg_query::protobuf::node::Node;
    let (options, routine_kind_location, returns_location) = match node {
        Node::DoStmt(statement) => (&statement.args, None, None),
        Node::CreateFunctionStmt(statement) => {
            if statement.sql_body.is_some() {
                return Err(unsupported(source, "SQL-standard routine body"));
            }
            (
                &statement.options,
                routine_kind_location(source, statement.is_procedure)?,
                routine_returns_location(source, statement)?,
            )
        }
        _ => return Err(unsupported(source, "non-routine statement")),
    };

    let mut language = None;
    let mut language_location = None;
    let mut body_count = 0usize;
    for option in options {
        let Some(Node::DefElem(option)) = option.node.as_ref() else {
            return Err(unsupported(source, "unrecognized routine option"));
        };
        match option.defname.as_str() {
            "language" => {
                language = option_string(option);
                language_location = usize::try_from(option.location).ok();
            }
            "as" => body_count += 1,
            _ => {}
        }
    }
    if language.as_deref().unwrap_or("plpgsql") != "plpgsql" {
        return Err(unsupported(source, "non-plpgsql routine"));
    }
    if body_count != 1 {
        return Err(unsupported(source, "routine without exactly one body"));
    }
    Ok(OuterTokenOwnership {
        language_location,
        routine_kind_location,
        returns_location,
    })
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

fn dollar_body_span(source: &str) -> Result<(usize, usize, usize, usize), FormatDiagnostic> {
    let bytes = source.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'$' {
            index += 1;
            continue;
        }
        let mut end = index + 1;
        while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
            end += 1;
        }
        if end >= bytes.len() || bytes[end] != b'$' {
            index += 1;
            continue;
        }
        let delimiter = &source[index..=end];
        if let Some(relative) = source[end + 1..].find(delimiter) {
            let close_start = end + 1 + relative;
            return Ok((index, end + 1, close_start, close_start + delimiter.len()));
        }
        index = end + 1;
    }
    Err(unsupported(source, "non-dollar-quoted routine body"))
}

fn format_leaf(
    kind: ir::BodyNodeKind,
    text: &str,
    options: &FormatOptions,
    indent: usize,
) -> Result<String, FormatDiagnostic> {
    let mut nested_options = options.clone();
    let indent_width = indent * 4;
    nested_options.soft_line_width = options.soft_line_width.saturating_sub(indent_width).max(1);
    nested_options.hard_line_width = options
        .hard_line_width
        .saturating_sub(indent_width)
        .max(nested_options.soft_line_width);
    format_body_statement(kind, text, &nested_options)
}

fn format_body_statement(
    kind: ir::BodyNodeKind,
    text: &str,
    options: &FormatOptions,
) -> Result<String, FormatDiagnostic> {
    let (raw_code, comment) = split_line_comment(text);
    let code = raw_code.trim();
    if code.is_empty() && !comment.is_empty() {
        return Ok(comment.to_owned());
    }
    let upper = code.to_ascii_uppercase();
    if let Some(rendered) = format_return_expression(kind, code, options)? {
        return Ok(attach_line_comment(rendered, comment));
    }
    if let Some(rendered) = format_assignment_expression(kind, code, options)? {
        return Ok(attach_line_comment(rendered, comment));
    }
    if let Some(rendered) = format_dynamic_execute(kind, code, options)? {
        return Ok(attach_line_comment(rendered, comment));
    }
    if kind == ir::BodyNodeKind::ReturnQuery {
        return format_return_query(code, &upper, options)
            .map(|rendered| attach_line_comment(rendered, comment));
    }
    for keyword in ["SELECT", "INSERT", "UPDATE", "DELETE", "MERGE"] {
        if upper.starts_with(keyword) && code.ends_with(';') {
            let formatted = super::format_sql(code, options)?.output;
            return Ok(attach_line_comment(formatted, comment));
        }
    }
    if let Some(rendered) = format_for_query_header(code, &upper, options)? {
        return Ok(attach_line_comment(rendered, comment));
    }
    if let Some(rendered) = format_cursor_query_declaration(code, &upper, options)? {
        return Ok(attach_line_comment(rendered, comment));
    }
    if let Some(rendered) = format_open_query(code, &upper, options)? {
        return Ok(attach_line_comment(rendered, comment));
    }
    let normalized = if kind == ir::BodyNodeKind::Declaration {
        normalize_procedural_code(
            &super::type_aliases::normalize_declaration(code, options)?,
            options,
        )?
    } else if code.starts_with("<<") && code.ends_with(">>") {
        code.to_owned()
    } else {
        normalize_procedural_code(code, options)?
    };
    Ok(attach_line_comment(
        uppercase_procedural_words(&normalized),
        comment,
    ))
}

fn normalize_procedural_code(
    source: &str,
    options: &FormatOptions,
) -> Result<String, FormatDiagnostic> {
    let tokens = super::tokens::tokenize(source)?;
    let mut output = String::with_capacity(source.len() + 8);
    let mut previous = None;
    for index in 0..tokens.len() {
        if procedural_needs_space(&tokens, previous, index) {
            output.push(' ');
        }
        output.push_str(&super::semantic_block::render_token(
            &tokens, index, options,
        ));
        previous = Some(index);
    }
    Ok(output)
}

fn format_return_expression(
    kind: ir::BodyNodeKind,
    code: &str,
    options: &FormatOptions,
) -> Result<Option<String>, FormatDiagnostic> {
    if kind != ir::BodyNodeKind::Return {
        return Ok(None);
    }
    let prefix = "RETURN";
    let expression = code[prefix.len()..].trim().trim_end_matches(';').trim();
    if expression.is_empty() {
        return Ok(None);
    }
    format_sql_expression(prefix, expression, options).map(Some)
}

fn format_assignment_expression(
    kind: ir::BodyNodeKind,
    code: &str,
    options: &FormatOptions,
) -> Result<Option<String>, FormatDiagnostic> {
    if kind != ir::BodyNodeKind::Assignment {
        return Ok(None);
    }
    let tokens = super::tokens::tokenize(code)?;
    let assignment = tokens
        .iter()
        .find(|token| token.text == ":=")
        .ok_or_else(|| FormatDiagnostic::Ownership("assignment has no := boundary".into()))?;
    let prefix = uppercase_procedural_words(&normalize_procedural_code(
        code[..assignment.end].trim(),
        options,
    )?);
    let expression = code[assignment.end..].trim().trim_end_matches(';').trim();
    if expression.is_empty() {
        return Err(FormatDiagnostic::Ownership(
            "assignment has no expression".into(),
        ));
    }
    format_sql_expression(&prefix, expression, options).map(Some)
}

fn format_dynamic_execute(
    kind: ir::BodyNodeKind,
    code: &str,
    options: &FormatOptions,
) -> Result<Option<String>, FormatDiagnostic> {
    if kind != ir::BodyNodeKind::DynamicExecute {
        return Ok(None);
    }
    let tokens = super::tokens::tokenize(code)?;
    let execute = tokens
        .first()
        .filter(|token| token.text.eq_ignore_ascii_case("EXECUTE"))
        .ok_or_else(|| FormatDiagnostic::Ownership("dynamic EXECUTE has no owner".into()))?;
    let statement_end = code
        .trim_end()
        .strip_suffix(';')
        .map_or(code.len(), str::len);
    let mut depth = 0usize;
    let mut clauses = Vec::new();
    for (index, token) in tokens.iter().enumerate().skip(1) {
        match token.kind {
            pg_query::protobuf::Token::Ascii41 | pg_query::protobuf::Token::Ascii93 => {
                depth = depth.saturating_sub(1);
            }
            pg_query::protobuf::Token::Into | pg_query::protobuf::Token::Using if depth == 0 => {
                clauses.push(index);
            }
            pg_query::protobuf::Token::Ascii40 | pg_query::protobuf::Token::Ascii91 => depth += 1,
            _ => {}
        }
    }

    let command_end = clauses
        .first()
        .map_or(statement_end, |index| tokens[*index].start);
    let command = code[execute.end..command_end].trim();
    if command.is_empty() {
        return Err(FormatDiagnostic::Ownership(
            "dynamic EXECUTE has no command expression".into(),
        ));
    }
    let mut rendered = format_sql_expression("EXECUTE", command, options)?
        .trim_end_matches(';')
        .to_owned();

    for (position, index) in clauses.iter().copied().enumerate() {
        let token = &tokens[index];
        let end = clauses
            .get(position + 1)
            .map_or(statement_end, |next| tokens[*next].start);
        let clause = if token.kind == pg_query::protobuf::Token::Using {
            let expressions = code[token.end..end].trim();
            if expressions.is_empty() {
                return Err(FormatDiagnostic::Ownership(
                    "dynamic EXECUTE USING has no expressions".into(),
                ));
            }
            format_sql_expression("USING", expressions, options)?
                .trim_end_matches(';')
                .to_owned()
        } else {
            let strict = tokens
                .get(index + 1)
                .filter(|next| next.start < end && next.text.eq_ignore_ascii_case("STRICT"));
            let prefix = if strict.is_some() {
                "INTO STRICT"
            } else {
                "INTO"
            };
            let targets_start = strict.map_or(token.end, |strict| strict.end);
            let targets = code[targets_start..end].trim();
            if targets.is_empty() {
                return Err(FormatDiagnostic::Ownership(
                    "dynamic EXECUTE INTO has no target".into(),
                ));
            }
            format_sql_expression(prefix, targets, options)?
                .trim_end_matches(';')
                .to_owned()
        };
        let combined_width = rendered
            .lines()
            .next_back()
            .map_or(0, |line| line.chars().count())
            + 1
            + clause.lines().next().map_or(0, |line| line.chars().count());
        if token.line_breaks_before > 0
            || rendered.contains('\n')
            || clause.contains('\n')
            || combined_width > options.hard_line_width
        {
            rendered.push_str(if token.line_breaks_before > 1 {
                "\n\n"
            } else {
                "\n"
            });
        } else {
            rendered.push(' ');
        }
        rendered.push_str(&clause);
    }
    rendered.push(';');
    Ok(Some(rendered))
}

fn format_sql_expression(
    prefix: &str,
    expression: &str,
    options: &FormatOptions,
) -> Result<String, FormatDiagnostic> {
    let formatted = super::format_sql(&format!("SELECT {expression};"), options)?.output;
    let body = formatted.strip_prefix("SELECT").ok_or_else(|| {
        FormatDiagnostic::Ownership("formatted procedural expression lost SELECT".into())
    })?;
    if let Some(inline) = body.strip_prefix(' ') {
        return Ok(format!("{prefix} {inline}"));
    }
    let mut lines = body
        .strip_prefix('\n')
        .ok_or_else(|| {
            FormatDiagnostic::Ownership("formatted procedural expression has no body".into())
        })?
        .lines()
        .map(|line| line.strip_prefix("    ").unwrap_or(line));
    let first = lines.next().ok_or_else(|| {
        FormatDiagnostic::Ownership("formatted procedural expression is empty".into())
    })?;
    Ok(std::iter::once(format!("{prefix} {first}"))
        .chain(lines.map(str::to_owned))
        .collect::<Vec<_>>()
        .join("\n"))
}

fn procedural_needs_space(
    tokens: &[super::tokens::SqlToken<'_>],
    previous: Option<usize>,
    current: usize,
) -> bool {
    let Some(previous) = previous else {
        return false;
    };
    if tokens[current].text == ".." || tokens[previous].text == ".." {
        return false;
    }
    if matches!(
        tokens[previous].kind,
        pg_query::protobuf::Token::Ascii43 | pg_query::protobuf::Token::Ascii45
    ) && (previous == 0
        || matches!(
            tokens[previous - 1].text.to_ascii_uppercase().as_str(),
            "RETURN" | "NEXT" | "WHEN" | "THEN" | "ELSE" | "BY" | "IN" | ":="
        )
        || matches!(
            tokens[previous - 1].kind,
            pg_query::protobuf::Token::Ascii40
                | pg_query::protobuf::Token::Ascii44
                | pg_query::protobuf::Token::Ascii61
        ))
    {
        return false;
    }
    super::semantic_block::needs_space(tokens, Some(previous), current)
}

fn format_return_query(
    code: &str,
    upper: &str,
    options: &FormatOptions,
) -> Result<String, FormatDiagnostic> {
    let body = code.trim_end_matches(';').trim();
    if upper.starts_with("RETURN QUERY EXECUTE ") {
        return Ok(format!("{};", uppercase_procedural_words(body)));
    }
    let prefix = "RETURN QUERY ";
    let query = body
        .get(prefix.len()..)
        .ok_or_else(|| unsupported(code, "malformed RETURN QUERY"))?;
    let formatted = format_query_fragment(query, options)?;
    Ok(format!("RETURN QUERY {formatted};"))
}

fn format_for_query_header(
    code: &str,
    upper: &str,
    options: &FormatOptions,
) -> Result<Option<String>, FormatDiagnostic> {
    if !upper.starts_with("FOR ") || !upper.ends_with(" LOOP") {
        return Ok(None);
    }
    let Some(marker) = [" IN SELECT ", " IN WITH "]
        .into_iter()
        .find_map(|marker| upper.find(marker).map(|index| (index, marker)))
    else {
        return Ok(None);
    };
    let query_start = marker.0 + " IN ".len();
    let query_end = code.len() - " LOOP".len();
    let prefix = uppercase_procedural_words(code[..query_start].trim_end());
    let query = format_query_fragment(&code[query_start..query_end], options)?;
    Ok(Some(format!("{prefix} {query} LOOP")))
}

fn format_cursor_query_declaration(
    code: &str,
    upper: &str,
    options: &FormatOptions,
) -> Result<Option<String>, FormatDiagnostic> {
    if !upper.contains(" CURSOR ") {
        return Ok(None);
    }
    let Some(for_index) = upper.find(" FOR ") else {
        return Ok(None);
    };
    let query_start = for_index + " FOR ".len();
    if !upper[query_start..].starts_with("SELECT ") && !upper[query_start..].starts_with("WITH ") {
        return Ok(None);
    }
    let query_end = code.strip_suffix(';').map_or(code.len(), str::len);
    let prefix = uppercase_procedural_words(code[..query_start].trim_end());
    let query = format_query_fragment(&code[query_start..query_end], options)?;
    Ok(Some(format!("{prefix} {query};")))
}

fn format_open_query(
    code: &str,
    upper: &str,
    options: &FormatOptions,
) -> Result<Option<String>, FormatDiagnostic> {
    if !upper.starts_with("OPEN ") {
        return Ok(None);
    }
    let Some(for_index) = upper.find(" FOR ") else {
        return Ok(None);
    };
    let query_start = for_index + " FOR ".len();
    if !upper[query_start..].starts_with("SELECT ") && !upper[query_start..].starts_with("WITH ") {
        return Ok(None);
    }
    let query_end = code.strip_suffix(';').map_or(code.len(), str::len);
    let prefix = uppercase_procedural_words(code[..query_start].trim_end());
    let query = format_query_fragment(&code[query_start..query_end], options)?;
    Ok(Some(format!("{prefix} {query};")))
}

fn format_query_fragment(
    source: &str,
    options: &FormatOptions,
) -> Result<String, FormatDiagnostic> {
    let statement = format!("{};", source.trim().trim_end_matches(';'));
    let formatted = super::format_sql(&statement, options)?.output;
    Ok(formatted
        .trim_end_matches(';')
        .lines()
        .map(str::trim)
        .collect::<Vec<_>>()
        .join(" "))
}

fn attach_line_comment(mut code: String, comment: &str) -> String {
    if !comment.is_empty() {
        code.push(' ');
        code.push_str(comment);
    }
    code
}

fn uppercase_procedural_words(text: &str) -> String {
    const WORDS: &[&str] = &[
        "array",
        "assert",
        "backward",
        "begin",
        "by",
        "case",
        "close",
        "delete",
        "continue",
        "cursor",
        "declare",
        "diagnostics",
        "else",
        "elsif",
        "end",
        "exception",
        "execute",
        "exit",
        "fetch",
        "first",
        "insert",
        "for",
        "foreach",
        "forward",
        "from",
        "get",
        "if",
        "in",
        "into",
        "last",
        "loop",
        "move",
        "next",
        "notice",
        "open",
        "others",
        "perform",
        "prior",
        "query",
        "raise",
        "relative",
        "select",
        "return",
        "reverse",
        "strict",
        "then",
        "update",
        "using",
        "warning",
        "when",
        "where",
        "while",
    ];
    let (code, comment) = split_line_comment(text);
    let mut result = String::with_capacity(text.len());
    let mut chars = code.char_indices().peekable();
    let mut quote = None;
    while let Some((start, ch)) = chars.next() {
        if matches!(ch, '\'' | '"') {
            if quote == Some(ch) {
                if chars.peek().is_some_and(|(_, next)| *next == ch) {
                    result.push(ch);
                    result.push(ch);
                    chars.next();
                    continue;
                }
                quote = None;
            } else if quote.is_none() {
                quote = Some(ch);
            }
            result.push(ch);
            continue;
        }
        if quote.is_none() && (ch.is_ascii_alphabetic() || ch == '_') {
            let mut end = start + ch.len_utf8();
            while let Some(&(index, next)) = chars.peek() {
                if next.is_ascii_alphanumeric() || next == '_' {
                    chars.next();
                    end = index + next.len_utf8();
                } else {
                    break;
                }
            }
            let word = &code[start..end];
            if WORDS
                .iter()
                .any(|candidate| word.eq_ignore_ascii_case(candidate))
            {
                result.push_str(&word.to_ascii_uppercase());
            } else {
                result.push_str(word);
            }
        } else {
            result.push(ch);
        }
    }
    result.push_str(comment);
    result
}

fn split_line_comment(text: &str) -> (&str, &str) {
    let bytes = text.as_bytes();
    let mut index = 0usize;
    let mut quote = None;
    while index < bytes.len() {
        let byte = bytes[index];
        if matches!(byte, b'\'' | b'"') {
            if quote == Some(byte) {
                if bytes.get(index + 1) == Some(&byte) {
                    index += 2;
                    continue;
                }
                quote = None;
            } else if quote.is_none() {
                quote = Some(byte);
            }
            index += 1;
            continue;
        }
        if quote.is_none() && bytes[index..].starts_with(b"--") {
            return text.split_at(index);
        }
        index += 1;
    }
    (text, "")
}

fn validate_plpgsql_equivalent(before: &Value, after: &Value) -> Result<(), FormatDiagnostic> {
    let mut before = before.clone();
    let mut after = after.clone();
    normalize_plpgsql(&mut before)?;
    normalize_plpgsql(&mut after)?;
    if before == after {
        Ok(())
    } else {
        Err(FormatDiagnostic::SemanticMismatch)
    }
}

fn normalize_plpgsql(value: &mut Value) -> Result<(), FormatDiagnostic> {
    match value {
        Value::Object(fields) => {
            fields.remove("lineno");
            if let Some(Value::String(type_name)) = fields.get_mut("typname") {
                *type_name = canonical_type_name(type_name);
            }
            if let Some(Value::String(query)) = fields.get("query").cloned() {
                let mode = fields.get("parseMode").and_then(Value::as_i64).unwrap_or(0);
                let canonical = match mode {
                    0 => canonical_postgresql(&query)?,
                    2 => canonical_postgresql(&format!("SELECT {query}"))?,
                    3 => {
                        let (target, expression) = query.split_once(":=").ok_or_else(|| {
                            unsupported(&query, "unrecognized PL/pgSQL assignment expression")
                        })?;
                        serde_json::json!({
                            "target": target.trim(),
                            "expression": canonical_postgresql(&format!("SELECT {}", expression.trim()))?,
                        })
                    }
                    _ => {
                        return Err(unsupported(
                            &query,
                            format!("unrecognized PL/pgSQL parse mode {mode}"),
                        ));
                    }
                };
                fields.insert("query".into(), canonical);
            }
            for child in fields.values_mut() {
                normalize_plpgsql(child)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                normalize_plpgsql(item)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn canonical_type_name(type_name: &str) -> String {
    let trimmed = type_name.trim();
    let lower = trimmed.to_ascii_lowercase();
    for (aliases, canonical) in [
        (&["smallint", "int2"][..], "int2"),
        (&["integer", "int", "int4"][..], "int4"),
        (&["bigint", "int8"][..], "int8"),
        (&["boolean", "bool"][..], "bool"),
        (&["character", "char"][..], "character"),
        (&["character varying", "varchar"][..], "varchar"),
        (&["bit varying", "varbit"][..], "varbit"),
        (&["numeric", "decimal"][..], "numeric"),
        (&["real", "float4"][..], "float4"),
        (&["double precision", "float", "float8"][..], "float8"),
        (&["time with time zone", "timetz"][..], "timetz"),
        (
            &["timestamp", "timestamp without time zone"][..],
            "timestamp",
        ),
        (
            &["timestamp with time zone", "timestamptz"][..],
            "timestamptz",
        ),
    ] {
        for alias in aliases {
            if lower == *alias {
                return canonical.to_owned();
            }
            if let Some(suffix) = lower.strip_prefix(alias)
                && (suffix.starts_with('(') || suffix.starts_with('['))
            {
                return format!("{canonical}{}", &trimmed[alias.len()..]);
            }
        }
    }
    trimmed.to_owned()
}

fn canonical_postgresql(source: &str) -> Result<Value, FormatDiagnostic> {
    let parsed = pg_query::parse(source)
        .map_err(|error| FormatDiagnostic::PostgreSqlParse(error.to_string()))?;
    let mut canonical = serde_json::to_value(parsed.protobuf)
        .map_err(|error| FormatDiagnostic::PostgreSqlParse(error.to_string()))?;
    strip_locations(&mut canonical);
    Ok(canonical)
}

fn strip_locations(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for name in ["location", "stmt_location", "stmt_len"] {
                fields.remove(name);
            }
            for child in fields.values_mut() {
                strip_locations(child);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(strip_locations),
        _ => {}
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

fn unsupported(source: &str, feature: impl Into<String>) -> FormatDiagnostic {
    FormatDiagnostic::UnsupportedSyntax {
        feature: feature.into(),
        start: 0,
        end: source.len(),
    }
}
