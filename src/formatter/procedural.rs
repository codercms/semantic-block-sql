use serde_json::Value;

use super::{FormatDiagnostic, FormatOptions, FormattedSql};

pub(super) fn is_routine(source: &str) -> Result<bool, FormatDiagnostic> {
    let parsed = pg_query::parse(source)
        .map_err(|error| FormatDiagnostic::PostgreSqlParse(error.to_string()))?;
    if parsed.protobuf.stmts.len() != 1 {
        return Ok(false);
    }
    let node = parsed.protobuf.stmts[0]
        .stmt
        .as_deref()
        .and_then(|node| node.node.as_ref());
    Ok(matches!(
        node,
        Some(
            pg_query::protobuf::node::Node::DoStmt(_)
                | pg_query::protobuf::node::Node::CreateFunctionStmt(_)
        )
    ))
}

pub(super) fn format_routine(
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

fn format_body(body: &str, options: &FormatOptions) -> Result<String, FormatDiagnostic> {
    let newline = if body.contains("\r\n") { "\r\n" } else { "\n" };
    let normalized = body.replace("\r\n", "\n");
    if !normalized.contains('\n') {
        return Err(unsupported(body, "compact single-line PL/pgSQL body"));
    }
    let mut lines = Vec::new();
    let mut indent = 0usize;
    let mut in_declare = false;
    let mut in_exception = false;
    let mut in_handler = false;

    for raw in normalized.lines() {
        let text = raw.trim();
        if text.is_empty() {
            lines.push(String::new());
            continue;
        }
        let upper = text.to_ascii_uppercase();
        if upper.starts_with("BEGIN") && in_declare {
            indent = indent.saturating_sub(1);
            in_declare = false;
        }
        if upper.starts_with("ELSIF ") || upper == "ELSE" {
            indent = indent.saturating_sub(1);
        }
        if upper == "EXCEPTION" {
            indent = indent.saturating_sub(1);
            in_exception = true;
            in_handler = false;
        }
        if upper.starts_with("WHEN ") && in_handler {
            indent = indent.saturating_sub(1);
            if lines.last().is_some_and(|line| !line.is_empty()) {
                lines.push(String::new());
            }
        }
        if upper.starts_with("END") {
            indent = if in_exception {
                0
            } else {
                indent.saturating_sub(1)
            };
            in_exception = false;
            in_handler = false;
        }

        let rendered = format_body_line(text, options)?;
        for (part_index, part) in rendered.lines().enumerate() {
            let part_indent = if part_index == 0 { indent } else { indent + 1 };
            lines.push(format!("{}{}", " ".repeat(part_indent * 4), part));
        }

        if upper == "DECLARE" {
            indent += 1;
            in_declare = true;
        } else if upper.starts_with("BEGIN") {
            indent += 1;
        } else if (upper.starts_with("IF ") || upper.starts_with("ELSIF "))
            && upper.ends_with(" THEN")
        {
            indent += 1;
        } else if upper == "ELSE" {
            indent += 1;
        } else if upper == "EXCEPTION" {
            indent += 1;
        } else if upper.starts_with("WHEN ") && upper.ends_with(" THEN") {
            indent += 1;
            in_handler = true;
        }
    }

    while lines.first().is_some_and(|line| line.is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    let result = format!("{newline}{}{newline}", lines.join(newline));
    Ok(result)
}

fn format_body_line(text: &str, options: &FormatOptions) -> Result<String, FormatDiagnostic> {
    let upper = text.to_ascii_uppercase();
    for keyword in ["SELECT", "INSERT", "UPDATE", "DELETE", "MERGE"] {
        if upper.starts_with(keyword) && text.ends_with(';') {
            return super::format_sql(text, options).map(|formatted| formatted.output);
        }
    }
    let keyword = ["DECLARE", "BEGIN", "END IF;", "END;", "EXCEPTION", "ELSE"]
        .into_iter()
        .find(|keyword| upper == *keyword);
    if let Some(keyword) = keyword {
        return Ok(keyword.to_string());
    }
    for prefix in [
        "IF ",
        "ELSIF ",
        "WHEN ",
        "RETURN NEXT ",
        "RETURN ",
        "PERFORM ",
        "RAISE ",
        "GET DIAGNOSTICS ",
    ] {
        if upper.starts_with(prefix) {
            return Ok(uppercase_procedural_words(&format!(
                "{}{}",
                prefix,
                text[prefix.len()..].trim_start()
            )));
        }
    }
    Ok(text
        .replace(":=", " := ")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" "))
}

fn uppercase_procedural_words(text: &str) -> String {
    const WORDS: &[&str] = &[
        "if",
        "elsif",
        "then",
        "else",
        "return",
        "next",
        "perform",
        "raise",
        "notice",
        "warning",
        "exception",
        "when",
        "others",
        "get",
        "diagnostics",
    ];
    let mut result = String::with_capacity(text.len());
    let mut chars = text.char_indices().peekable();
    let mut quoted = false;
    while let Some((start, ch)) = chars.next() {
        if ch == '\'' {
            quoted = !quoted;
            result.push(ch);
            continue;
        }
        if !quoted && (ch.is_ascii_alphabetic() || ch == '_') {
            let mut end = start + ch.len_utf8();
            while let Some(&(index, next)) = chars.peek() {
                if next.is_ascii_alphanumeric() || next == '_' {
                    chars.next();
                    end = index + next.len_utf8();
                } else {
                    break;
                }
            }
            let word = &text[start..end];
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
    result
}

fn validate_plpgsql_equivalent(before: &Value, after: &Value) -> Result<(), FormatDiagnostic> {
    let mut before = before.clone();
    let mut after = after.clone();
    normalize_plpgsql(&mut before);
    normalize_plpgsql(&mut after);
    if before == after {
        Ok(())
    } else {
        Err(FormatDiagnostic::SemanticMismatch)
    }
}

fn normalize_plpgsql(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            fields.remove("lineno");
            if fields.contains_key("query") {
                fields.remove("query");
            }
            if let Some(Value::String(type_name)) = fields.get_mut("typname") {
                *type_name = type_name.trim().to_owned();
            }
            for child in fields.values_mut() {
                normalize_plpgsql(child);
            }
        }
        Value::Array(items) => {
            for item in items {
                normalize_plpgsql(item);
            }
        }
        _ => {}
    }
}

fn unsupported(source: &str, feature: impl Into<String>) -> FormatDiagnostic {
    FormatDiagnostic::UnsupportedSyntax {
        feature: feature.into(),
        start: 0,
        end: source.len(),
    }
}
