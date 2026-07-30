use serde_json::Value;

use super::{FormatDiagnostic, FormatOptions, FormattedSql};

pub(super) fn format_single_routine(
    source: &str,
    options: &FormatOptions,
) -> Result<FormattedSql, FormatDiagnostic> {
    validate_outer(source)?;
    let parsed = pg_query::parse_plpgsql(source)
        .map_err(|error| FormatDiagnostic::PostgreSqlParse(error.to_string()))?;
    validate_plpgsql_json(&parsed)?;

    let (open_start, open_end, close_start, close_end) = dollar_body_span(source)?;
    let body = &source[open_end..close_start];
    let formatted_body = format_body(body, options)?;
    let mut output = String::with_capacity(source.len() + formatted_body.len());
    output.push_str(&source[..open_start]);
    output.push_str(&source[open_start..open_end]);
    output.push_str(&formatted_body);
    output.push_str(&source[close_start..close_end]);
    output.push_str(&source[close_end..]);
    let output = normalize_outer_tokens(&output, options)?;

    validate_outer(&output)?;
    let reparsed = pg_query::parse_plpgsql(&output)
        .map_err(|error| FormatDiagnostic::PostgreSqlParse(error.to_string()))?;
    validate_plpgsql_json(&reparsed)?;
    validate_plpgsql_equivalent(&parsed, &reparsed)?;

    let second_body = {
        let (_, second_open, second_close, _) = dollar_body_span(&output)?;
        format_body(&output[second_open..second_close], options)?
    };
    if second_body != formatted_body {
        return Err(FormatDiagnostic::NotIdempotent);
    }

    Ok(FormattedSql {
        changed: output != source,
        output,
        warnings: Vec::new(),
        diagnostics: Vec::new(),
    })
}

fn validate_outer(source: &str) -> Result<(), FormatDiagnostic> {
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
    let options = match node {
        Node::DoStmt(statement) => &statement.args,
        Node::CreateFunctionStmt(statement) => {
            if statement.sql_body.is_some() {
                return Err(unsupported(source, "SQL-standard routine body"));
            }
            &statement.options
        }
        _ => return Err(unsupported(source, "non-routine statement")),
    };

    let mut language = None;
    let mut body_count = 0usize;
    for option in options {
        let Some(Node::DefElem(option)) = option.node.as_ref() else {
            return Err(unsupported(source, "unrecognized routine option"));
        };
        match option.defname.as_str() {
            "language" => language = option_string(option),
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
    Ok(())
}

fn option_string(option: &pg_query::protobuf::DefElem) -> Option<String> {
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

fn validate_plpgsql_json(value: &Value) -> Result<(), FormatDiagnostic> {
    const ALLOWED: &[&str] = &[
        "PLpgSQL_function",
        "PLpgSQL_stmt_block",
        "PLpgSQL_stmt_execsql",
        "PLpgSQL_stmt_perform",
        "PLpgSQL_stmt_return",
        "PLpgSQL_stmt_if",
        "PLpgSQL_stmt_assign",
        "PLpgSQL_stmt_raise",
        "PLpgSQL_stmt_getdiag",
        "PLpgSQL_stmt_loop",
        "PLpgSQL_stmt_while",
        "PLpgSQL_stmt_fori",
        "PLpgSQL_stmt_fors",
        "PLpgSQL_stmt_forc",
        "PLpgSQL_stmt_foreach_a",
        "PLpgSQL_stmt_exit",
        "PLpgSQL_stmt_case",
        "PLpgSQL_case_when",
        "PLpgSQL_stmt_dynexecute",
        "PLpgSQL_stmt_open",
        "PLpgSQL_stmt_fetch",
        "PLpgSQL_stmt_close",
        "PLpgSQL_rec",
        "PLpgSQL_recfield",
        "PLpgSQL_row",
        "PLpgSQL_exception_block",
        "PLpgSQL_exception",
        "PLpgSQL_condition",
        "PLpgSQL_expr",
        "PLpgSQL_var",
        "PLpgSQL_type",
        "PLpgSQL_diag_item",
    ];
    match value {
        Value::Object(fields) => {
            for (name, child) in fields {
                if name.starts_with("PLpgSQL_") && !ALLOWED.contains(&name.as_str()) {
                    return Err(FormatDiagnostic::UnsupportedSyntax {
                        feature: format!("PL/pgSQL node {name}"),
                        start: 0,
                        end: 0,
                    });
                }
                validate_plpgsql_json(child)?;
            }
        }
        Value::Array(items) => {
            for item in items {
                validate_plpgsql_json(item)?;
            }
        }
        _ => {}
    }
    Ok(())
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BodyFrame {
    Begin,
    If,
    Loop,
    Case,
    CaseBranch,
    Exception,
    ExceptionBranch,
}

fn format_body(body: &str, options: &FormatOptions) -> Result<String, FormatDiagnostic> {
    let newline = if body.contains("\r\n") { "\r\n" } else { "\n" };
    let normalized = body.replace("\r\n", "\n");
    if !normalized.contains('\n') {
        return Err(unsupported(body, "compact single-line PL/pgSQL body"));
    }
    let mut lines = Vec::new();
    let mut frames = Vec::new();
    let mut in_declare = false;

    for raw in normalized.lines() {
        let text = raw.trim();
        if text.is_empty() {
            lines.push(String::new());
            continue;
        }
        let code = line_code(text).trim();
        let upper = code.to_ascii_uppercase();
        let mut render_indent = frames.len() + usize::from(in_declare);
        let mut push_after = None;

        if upper == "DECLARE" {
            render_indent = frames.len();
            in_declare = true;
        } else if starts_control(&upper, "BEGIN") {
            render_indent = frames.len();
            in_declare = false;
            push_after = Some(BodyFrame::Begin);
        } else if starts_control(&upper, "END IF") {
            pop_expected(&mut frames, BodyFrame::If, body)?;
            render_indent = frames.len();
        } else if starts_control(&upper, "END LOOP") {
            pop_expected(&mut frames, BodyFrame::Loop, body)?;
            render_indent = frames.len();
        } else if starts_control(&upper, "END CASE") {
            pop_optional_branch(&mut frames, BodyFrame::CaseBranch);
            pop_expected(&mut frames, BodyFrame::Case, body)?;
            render_indent = frames.len();
        } else if starts_control(&upper, "END") {
            pop_optional_branch(&mut frames, BodyFrame::ExceptionBranch);
            match frames.pop() {
                Some(BodyFrame::Begin | BodyFrame::Exception) => {}
                _ => return Err(unsupported(body, "unbalanced PL/pgSQL END")),
            }
            render_indent = frames.len();
        } else if upper == "EXCEPTION" {
            pop_optional_branch(&mut frames, BodyFrame::ExceptionBranch);
            match frames.last_mut() {
                Some(frame @ BodyFrame::Begin) => *frame = BodyFrame::Exception,
                _ => return Err(unsupported(body, "EXCEPTION outside a block")),
            }
            render_indent = frames.len().saturating_sub(1);
        } else if starts_control(&upper, "ELSIF") {
            if frames.last() != Some(&BodyFrame::If) {
                return Err(unsupported(body, "ELSIF outside IF"));
            }
            render_indent = frames.len().saturating_sub(1);
        } else if starts_control(&upper, "WHEN") {
            pop_optional_branch(&mut frames, BodyFrame::CaseBranch);
            pop_optional_branch(&mut frames, BodyFrame::ExceptionBranch);
            render_indent = frames.len();
            push_after = match frames.last() {
                Some(BodyFrame::Case) => Some(BodyFrame::CaseBranch),
                Some(BodyFrame::Exception) => Some(BodyFrame::ExceptionBranch),
                _ => return Err(unsupported(body, "WHEN outside CASE or EXCEPTION")),
            };
            if matches!(push_after, Some(BodyFrame::ExceptionBranch))
                && lines.last().is_some_and(|line| !line.is_empty())
                && frames.last() == Some(&BodyFrame::Exception)
                && lines
                    .iter()
                    .rev()
                    .any(|line| line.trim_start().starts_with("WHEN "))
            {
                lines.push(String::new());
            }
        } else if upper == "ELSE" {
            if frames.last() == Some(&BodyFrame::CaseBranch) {
                frames.pop();
                render_indent = frames.len();
                push_after = Some(BodyFrame::CaseBranch);
            } else if frames.last() == Some(&BodyFrame::If) {
                render_indent = frames.len().saturating_sub(1);
            } else {
                return Err(unsupported(body, "ELSE outside IF or CASE"));
            }
        } else if starts_control(&upper, "IF") && upper.ends_with(" THEN") {
            push_after = Some(BodyFrame::If);
        } else if is_loop_header(&upper) {
            push_after = Some(BodyFrame::Loop);
        } else if upper == "CASE" || upper.starts_with("CASE ") {
            push_after = Some(BodyFrame::Case);
        }

        let rendered = format_body_line(text, options)?;
        for part in rendered.lines() {
            lines.push(format!("{}{}", " ".repeat(render_indent * 4), part));
        }
        if let Some(frame) = push_after {
            frames.push(frame);
        }
    }

    while lines.first().is_some_and(|line| line.is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    if !frames.is_empty() || in_declare {
        return Err(unsupported(body, "unbalanced PL/pgSQL block"));
    }
    Ok(format!("{newline}{}{newline}", lines.join(newline)))
}

fn starts_control(upper: &str, keyword: &str) -> bool {
    upper == keyword
        || upper
            .strip_prefix(keyword)
            .is_some_and(|rest| matches!(rest.as_bytes().first(), Some(b' ' | b';')))
}

fn is_loop_header(upper: &str) -> bool {
    upper == "LOOP"
        || ((upper.starts_with("WHILE ")
            || upper.starts_with("FOR ")
            || upper.starts_with("FOREACH "))
            && upper.ends_with(" LOOP"))
}

fn pop_optional_branch(frames: &mut Vec<BodyFrame>, branch: BodyFrame) {
    if frames.last() == Some(&branch) {
        frames.pop();
    }
}

fn pop_expected(
    frames: &mut Vec<BodyFrame>,
    expected: BodyFrame,
    source: &str,
) -> Result<(), FormatDiagnostic> {
    if frames.pop() == Some(expected) {
        Ok(())
    } else {
        Err(unsupported(source, "unbalanced PL/pgSQL control flow"))
    }
}

fn format_body_line(text: &str, options: &FormatOptions) -> Result<String, FormatDiagnostic> {
    let (raw_code, comment) = split_line_comment(text);
    let code = raw_code.trim();
    if code.is_empty() && !comment.is_empty() {
        return Ok(comment.to_owned());
    }
    let upper = code.to_ascii_uppercase();
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
    let normalized = if code.starts_with("<<") && code.ends_with(">>") {
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

fn line_code(text: &str) -> &str {
    split_line_comment(text).0
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
                *type_name = type_name.trim().to_owned();
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

fn normalize_outer_tokens(
    source: &str,
    options: &FormatOptions,
) -> Result<String, FormatDiagnostic> {
    let tokens = super::tokens::tokenize(source)?;
    let mut output = String::with_capacity(source.len());
    let mut cursor = 0usize;
    for (index, token) in tokens.iter().enumerate() {
        output.push_str(&source[cursor..token.start]);
        if ["function", "procedure", "language", "returns"]
            .iter()
            .any(|keyword| token.text.eq_ignore_ascii_case(keyword))
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
