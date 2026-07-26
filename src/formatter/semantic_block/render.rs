use pg_query::protobuf::{KeywordKind, Token};

use super::*;

pub(in crate::formatter) fn render_token(
    tokens: &[SqlToken<'_>],
    index: usize,
    options: &FormatOptions,
) -> String {
    let token = &tokens[index];
    let previous = index.checked_sub(1).map(|previous| &tokens[previous]);
    let next = tokens.get(index + 1);

    if token.kind == Token::NotEquals {
        return match options.not_equal_policy {
            NotEqualPolicy::Preserve => token.text.to_owned(),
            NotEqualPolicy::PreferBang => "!=".into(),
        };
    }
    if is_on_conflict_excluded(tokens, index)
        || is_overriding_value_keyword(tokens, index)
        || is_merge_match_side_keyword(tokens, index)
    {
        return token.text.to_uppercase();
    }
    if token.kind == Token::Interval {
        return if next.is_some_and(|next| is_string_literal(next.kind)) {
            token.text.to_uppercase()
        } else {
            token.text.to_lowercase()
        };
    }
    if is_function_call_name(tokens, index) {
        return if is_uppercase_builtin(token.text) {
            token.text.to_uppercase()
        } else {
            token.text.to_lowercase()
        };
    }
    if token.kind == Token::Ident
        && !token.text.starts_with('"')
        && previous.is_some_and(|previous| previous.kind == Token::Typecast)
    {
        return token.text.to_lowercase();
    }
    if is_type_keyword(token.kind) {
        return token.text.to_lowercase();
    }
    if (token.keyword_kind == KeywordKind::ReservedKeyword || is_keyword_like(token.kind))
        && previous.is_none_or(|previous| previous.kind != Token::Ascii46)
        && next.is_none_or(|next| next.kind != Token::Ascii46)
    {
        return token.text.to_uppercase();
    }
    token.text.to_owned()
}

pub(in crate::formatter) fn is_function_call_name(tokens: &[SqlToken<'_>], index: usize) -> bool {
    !tokens[index].text.starts_with('"') && is_function_call_syntax(tokens, index)
}

pub(in crate::formatter) fn is_function_call_syntax(tokens: &[SqlToken<'_>], index: usize) -> bool {
    tokens
        .get(index + 1)
        .is_some_and(|next| next.kind == Token::Ascii40)
        && matches!(
            tokens[index].kind,
            Token::Ident
                | Token::Coalesce
                | Token::Extract
                | Token::Greatest
                | Token::JsonArray
                | Token::JsonArrayagg
                | Token::JsonExists
                | Token::JsonObject
                | Token::JsonObjectagg
                | Token::JsonQuery
                | Token::JsonScalar
                | Token::JsonSerialize
                | Token::JsonTable
                | Token::JsonValue
                | Token::Least
                | Token::MergeAction
                | Token::Normalize
                | Token::Nullif
                | Token::Overlay
                | Token::Position
                | Token::Substring
                | Token::Trim
                | Token::Xmlattributes
                | Token::Xmlconcat
                | Token::Xmlelement
                | Token::Xmlexists
                | Token::Xmlforest
                | Token::Xmlnamespaces
                | Token::Xmlparse
                | Token::Xmlpi
                | Token::Xmlroot
                | Token::Xmlserialize
                | Token::Xmltable
        )
}

pub(in crate::formatter) fn is_compact_grammar_parenthesis(
    tokens: &[SqlToken<'_>],
    index: usize,
) -> bool {
    tokens
        .get(index + 1)
        .is_some_and(|next| next.kind == Token::Ascii40)
        && matches!(tokens[index].kind, Token::Cast | Token::Treat)
}

pub(in crate::formatter) fn is_type_modifier_syntax(tokens: &[SqlToken<'_>], index: usize) -> bool {
    tokens
        .get(index + 1)
        .is_some_and(|next| next.kind == Token::Ascii40)
        && (tokens[index].kind == Token::Interval || is_type_keyword(tokens[index].kind))
}

pub(in crate::formatter) fn is_uppercase_builtin(name: &str) -> bool {
    matches!(
        name.to_ascii_uppercase().as_str(),
        "COUNT"
            | "SUM"
            | "AVG"
            | "MIN"
            | "MAX"
            | "COALESCE"
            | "NULLIF"
            | "GREATEST"
            | "LEAST"
            | "NOW"
            | "EXTRACT"
    )
}

fn is_on_conflict_excluded(tokens: &[SqlToken<'_>], index: usize) -> bool {
    if tokens[index].kind != Token::Ident
        || !tokens[index].text.eq_ignore_ascii_case("excluded")
        || tokens
            .get(index + 1)
            .is_none_or(|next| next.kind != Token::Ascii46)
    {
        return false;
    }

    let statement_start = tokens[..index]
        .iter()
        .rposition(|token| token.kind == Token::Ascii59)
        .map_or(0, |semicolon| semicolon + 1);
    let statement = &tokens[statement_start..index];
    let Some(conflict) = statement
        .iter()
        .rposition(|token| token.kind == Token::Conflict)
    else {
        return false;
    };
    let Some(action) = statement[conflict + 1..]
        .iter()
        .position(|token| token.kind == Token::Do)
        .map(|offset| conflict + 1 + offset)
    else {
        return false;
    };
    let Some(update) = statement[action + 1..]
        .iter()
        .position(|token| token.kind == Token::Update)
        .map(|offset| action + 1 + offset)
    else {
        return false;
    };
    let Some(set) = statement[update + 1..]
        .iter()
        .position(|token| token.kind == Token::Set)
        .map(|offset| update + 1 + offset)
    else {
        return false;
    };

    !statement[set + 1..]
        .iter()
        .any(|token| token.kind == Token::Returning)
}

fn is_overriding_value_keyword(tokens: &[SqlToken<'_>], index: usize) -> bool {
    match tokens[index].kind {
        Token::SystemP => tokens[..index]
            .iter()
            .rev()
            .take(2)
            .any(|token| token.kind == Token::Overriding),
        Token::ValueP => tokens.get(index.wrapping_sub(1)).is_some_and(|previous| {
            matches!(previous.kind, Token::SystemP | Token::User)
                && tokens[..index.saturating_sub(1)]
                    .iter()
                    .rev()
                    .take(2)
                    .any(|token| token.kind == Token::Overriding)
        }),
        _ => false,
    }
}

fn is_merge_match_side_keyword(tokens: &[SqlToken<'_>], index: usize) -> bool {
    if !matches!(tokens[index].kind, Token::Source | Token::Target)
        || tokens
            .get(index.wrapping_sub(1))
            .is_none_or(|previous| previous.kind != Token::By)
    {
        return false;
    }
    tokens[..index.saturating_sub(1)]
        .iter()
        .rev()
        .take(4)
        .any(|token| token.kind == Token::Matched)
}

fn is_string_literal(kind: Token) -> bool {
    matches!(kind, Token::Sconst | Token::Usconst)
}

fn is_keyword_like(kind: Token) -> bool {
    matches!(
        kind,
        Token::All
            | Token::And
            | Token::As
            | Token::By
            | Token::Case
            | Token::Coalesce
            | Token::Conflict
            | Token::Cross
            | Token::CurrentDate
            | Token::CurrentRole
            | Token::CurrentSchema
            | Token::CurrentTime
            | Token::CurrentTimestamp
            | Token::CurrentUser
            | Token::DayP
            | Token::Distinct
            | Token::DeleteP
            | Token::Do
            | Token::Else
            | Token::EndP
            | Token::Except
            | Token::FalseP
            | Token::Fetch
            | Token::FirstP
            | Token::From
            | Token::Full
            | Token::GroupP
            | Token::Having
            | Token::HourP
            | Token::InnerP
            | Token::Insert
            | Token::Intersect
            | Token::Is
            | Token::Join
            | Token::Left
            | Token::Limit
            | Token::Localtime
            | Token::Localtimestamp
            | Token::MinuteP
            | Token::Matched
            | Token::Merge
            | Token::MonthP
            | Token::Natural
            | Token::Nothing
            | Token::Not
            | Token::Only
            | Token::NullP
            | Token::Nullif
            | Token::Offset
            | Token::On
            | Token::Or
            | Token::Order
            | Token::OuterP
            | Token::Overriding
            | Token::Recursive
            | Token::Returning
            | Token::Right
            | Token::Rows
            | Token::SecondP
            | Token::Select
            | Token::Set
            | Token::SessionUser
            | Token::Then
            | Token::TrueP
            | Token::Union
            | Token::Update
            | Token::User
            | Token::Values
            | Token::When
            | Token::Where
            | Token::With
            | Token::YearP
    )
}

pub(in crate::formatter) fn is_type_keyword(kind: Token) -> bool {
    matches!(
        kind,
        Token::Bigint
            | Token::Bit
            | Token::BooleanP
            | Token::CharP
            | Token::Character
            | Token::DecimalP
            | Token::FloatP
            | Token::IntP
            | Token::Integer
            | Token::Json
            | Token::Numeric
            | Token::Real
            | Token::Smallint
            | Token::TextP
            | Token::Time
            | Token::Timestamp
            | Token::Varchar
    )
}

fn is_insert_target_list_open(tokens: &[SqlToken<'_>], open: usize) -> bool {
    if tokens
        .get(open)
        .is_none_or(|token| token.kind != Token::Ascii40)
    {
        return false;
    }
    for token in tokens[..open].iter().rev() {
        match token.kind {
            Token::Ascii59 | Token::Values => return false,
            Token::Insert => return true,
            _ => {}
        }
    }
    false
}

pub(super) fn needs_space(
    tokens: &[SqlToken<'_>],
    previous: Option<usize>,
    current: usize,
) -> bool {
    let Some(previous_index) = previous else {
        return false;
    };
    let current_index = current;
    let previous = &tokens[previous_index];
    let current = &tokens[current_index];
    if previous.kind == Token::SqlComment
        || (previous.kind == Token::CComment && current.line_breaks_before > 0)
    {
        return false;
    }
    if matches!(
        current.kind,
        Token::Ascii44
            | Token::Ascii59
            | Token::Ascii41
            | Token::Ascii93
            | Token::Ascii46
            | Token::Typecast
    ) || matches!(
        previous.kind,
        Token::Ascii40 | Token::Ascii91 | Token::Ascii46 | Token::Typecast
    ) {
        return false;
    }
    if current.kind == Token::Ascii40 && is_insert_target_list_open(tokens, current_index) {
        return true;
    }
    if current.kind == Token::Ascii40
        && (is_function_call_syntax(tokens, previous_index)
            || is_type_modifier_syntax(tokens, previous_index)
            || is_compact_grammar_parenthesis(tokens, previous_index))
    {
        return false;
    }
    if matches!(previous.kind, Token::Ascii43 | Token::Ascii45)
        && is_unary_sign(tokens, previous_index)
    {
        return false;
    }
    true
}

fn is_unary_sign(tokens: &[SqlToken<'_>], index: usize) -> bool {
    if !matches!(tokens[index].kind, Token::Ascii43 | Token::Ascii45) {
        return false;
    }
    let Some(previous) = index.checked_sub(1).map(|previous| tokens[previous].kind) else {
        return true;
    };
    matches!(
        previous,
        Token::Ascii40
            | Token::Ascii43
            | Token::Ascii44
            | Token::Ascii45
            | Token::Ascii47
            | Token::Ascii61
            | Token::And
            | Token::Else
            | Token::Op
            | Token::Or
            | Token::Select
            | Token::Then
            | Token::When
    )
}
