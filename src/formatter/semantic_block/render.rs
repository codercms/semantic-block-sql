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
        || is_with_ordinality_keyword(tokens, index)
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
        && is_cast_type_identifier(tokens, index)
    {
        return token.text.to_lowercase();
    }
    if is_type_keyword(token.kind) || is_contextual_type_keyword(tokens, index) {
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
        && (tokens[index].kind == Token::Interval
            || is_type_keyword(tokens[index].kind)
            || is_contextual_type_keyword(tokens, index))
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

fn is_with_ordinality_keyword(tokens: &[SqlToken<'_>], index: usize) -> bool {
    tokens[index].kind == Token::Ordinality
        && tokens
            .get(index.wrapping_sub(1))
            .is_some_and(|previous| previous.kind == Token::With)
}

fn is_string_literal(kind: Token) -> bool {
    matches!(kind, Token::Sconst | Token::Usconst)
}

fn is_keyword_like(kind: Token) -> bool {
    matches!(
        kind,
        Token::AddP
            | Token::All
            | Token::Alter
            | Token::Always
            | Token::And
            | Token::As
            | Token::Attach
            | Token::Between
            | Token::By
            | Token::Cascade
            | Token::Cascaded
            | Token::Case
            | Token::Check
            | Token::Coalesce
            | Token::Column
            | Token::Concurrently
            | Token::Conflict
            | Token::Constraint
            | Token::Create
            | Token::Cross
            | Token::CurrentDate
            | Token::CurrentP
            | Token::CurrentRole
            | Token::CurrentSchema
            | Token::CurrentTime
            | Token::CurrentTimestamp
            | Token::CurrentUser
            | Token::DayP
            | Token::DeleteP
            | Token::Distinct
            | Token::Do
            | Token::Drop
            | Token::Else
            | Token::EndP
            | Token::Except
            | Token::Exists
            | Token::Exclude
            | Token::FalseP
            | Token::Fetch
            | Token::Filter
            | Token::FirstP
            | Token::Following
            | Token::Foreign
            | Token::From
            | Token::Full
            | Token::Generated
            | Token::GroupP
            | Token::Groups
            | Token::Having
            | Token::HourP
            | Token::IdentityP
            | Token::IfP
            | Token::Include
            | Token::Index
            | Token::InnerP
            | Token::Insert
            | Token::Intersect
            | Token::Is
            | Token::Join
            | Token::Key
            | Token::LastP
            | Token::Left
            | Token::Limit
            | Token::Local
            | Token::Localtime
            | Token::Localtimestamp
            | Token::Matched
            | Token::Materialized
            | Token::Merge
            | Token::MinuteP
            | Token::MonthP
            | Token::Natural
            | Token::No
            | Token::Not
            | Token::Nothing
            | Token::NullP
            | Token::Nullif
            | Token::NullsP
            | Token::Offset
            | Token::Option
            | Token::Others
            | Token::On
            | Token::Only
            | Token::Or
            | Token::Order
            | Token::OuterP
            | Token::Over
            | Token::Overriding
            | Token::Partition
            | Token::Preceding
            | Token::Primary
            | Token::Recursive
            | Token::Replace
            | Token::References
            | Token::Restrict
            | Token::Returning
            | Token::Right
            | Token::Row
            | Token::Rows
            | Token::SecondP
            | Token::Select
            | Token::SessionUser
            | Token::Set
            | Token::Table
            | Token::Tablespace
            | Token::Then
            | Token::Ties
            | Token::TrueP
            | Token::Unbounded
            | Token::Union
            | Token::Unique
            | Token::Update
            | Token::User
            | Token::Validate
            | Token::Values
            | Token::View
            | Token::When
            | Token::Where
            | Token::Window
            | Token::Within
            | Token::With
            | Token::DataP
            | Token::Admin
            | Token::After
            | Token::Before
            | Token::Cache
            | Token::Comment
            | Token::Cycle
            | Token::DomainP
            | Token::Each
            | Token::EnumP
            | Token::Execute
            | Token::Function
            | Token::Grant
            | Token::Increment
            | Token::Maxvalue
            | Token::Minvalue
            | Token::Owned
            | Token::Policy
            | Token::Restart
            | Token::Revoke
            | Token::Sequence
            | Token::Start
            | Token::Trigger
            | Token::Truncate
            | Token::TypeP
            | Token::YearP
    )
}

fn is_contextual_type_keyword(tokens: &[SqlToken<'_>], index: usize) -> bool {
    let previous = index
        .checked_sub(1)
        .and_then(|previous| tokens.get(previous));
    let before_previous = index
        .checked_sub(2)
        .and_then(|before_previous| tokens.get(before_previous));
    let next = tokens.get(index + 1);

    match tokens[index].kind {
        Token::DoubleP => next.is_some_and(|next| next.kind == Token::Precision),
        Token::Precision => previous.is_some_and(|previous| previous.kind == Token::DoubleP),
        Token::National => {
            next.is_some_and(|next| matches!(next.kind, Token::CharP | Token::Character))
        }
        Token::Varying => previous.is_some_and(|previous| {
            matches!(previous.kind, Token::Bit | Token::CharP | Token::Character)
        }),
        Token::Zone => {
            previous.is_some_and(|previous| previous.kind == Token::Time)
                && before_previous.is_some_and(|before_previous| {
                    matches!(before_previous.kind, Token::With | Token::Without)
                })
        }
        _ => false,
    }
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

fn is_cast_type_identifier(tokens: &[SqlToken<'_>], index: usize) -> bool {
    let Some(previous) = index.checked_sub(1) else {
        return false;
    };

    match tokens[previous].kind {
        Token::Typecast => true,
        Token::As => as_belongs_to_cast(tokens, previous),
        Token::Ascii46 => previous
            .checked_sub(1)
            .is_some_and(|component| is_cast_type_name_component(tokens, component)),
        _ => false,
    }
}

fn is_cast_type_name_component(tokens: &[SqlToken<'_>], index: usize) -> bool {
    if tokens[index].kind != Token::Ident {
        return false;
    }

    let Some(previous) = index.checked_sub(1) else {
        return false;
    };
    match tokens[previous].kind {
        Token::Typecast => true,
        Token::As => as_belongs_to_cast(tokens, previous),
        Token::Ascii46 => previous
            .checked_sub(1)
            .is_some_and(|component| is_cast_type_name_component(tokens, component)),
        _ => false,
    }
}

fn as_belongs_to_cast(tokens: &[SqlToken<'_>], as_index: usize) -> bool {
    let mut parentheses = 0usize;

    for cursor in (0..as_index).rev() {
        match tokens[cursor].kind {
            Token::Ascii41 => parentheses += 1,
            Token::Ascii40 if parentheses > 0 => parentheses -= 1,
            Token::Ascii40 => {
                return cursor
                    .checked_sub(1)
                    .is_some_and(|previous| tokens[previous].kind == Token::Cast);
            }
            Token::Ascii59 if parentheses == 0 => return false,
            _ => {}
        }
    }

    false
}

fn is_array_slice_colon(tokens: &[SqlToken<'_>], index: usize) -> bool {
    if tokens
        .get(index)
        .is_none_or(|token| token.kind != Token::Ascii58)
    {
        return false;
    }

    let mut parentheses = 0usize;
    let mut brackets = 0usize;
    for token in tokens[..index].iter().rev() {
        match token.kind {
            Token::Ascii41 => parentheses += 1,
            Token::Ascii93 => brackets += 1,
            Token::Ascii40 if parentheses > 0 => parentheses -= 1,
            Token::Ascii91 if brackets > 0 => brackets -= 1,
            Token::Ascii40 => return false,
            Token::Ascii91 => return parentheses == 0,
            Token::Ascii59 if parentheses == 0 && brackets == 0 => return false,
            _ => {}
        }
    }

    false
}

fn is_insert_target_list_open(tokens: &[SqlToken<'_>], open: usize) -> bool {
    if tokens
        .get(open)
        .is_none_or(|token| token.kind != Token::Ascii40)
    {
        return false;
    }
    let statement_start = tokens[..open]
        .iter()
        .rposition(|token| token.kind == Token::Ascii59)
        .map_or(0, |semicolon| semicolon + 1);
    let first = tokens[statement_start..open]
        .iter()
        .find(|token| !token.is_comment())
        .map(|token| token.kind);
    if !matches!(first, Some(Token::Insert | Token::With)) {
        return false;
    }
    for token in tokens[statement_start..open].iter().rev() {
        match token.kind {
            Token::Ascii59 | Token::Values | Token::Select => return false,
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
            | Token::Ascii91
            | Token::Ascii46
            | Token::Typecast
    ) || matches!(
        previous.kind,
        Token::Ascii40 | Token::Ascii91 | Token::Ascii46 | Token::Typecast
    ) {
        return false;
    }
    if (current.kind == Token::Ascii58 && is_array_slice_colon(tokens, current_index))
        || (previous.kind == Token::Ascii58 && is_array_slice_colon(tokens, previous_index))
    {
        return false;
    }
    if current.kind == Token::Ascii40 && is_insert_target_list_open(tokens, current_index) {
        return true;
    }
    if current.kind == Token::Ascii40 && is_ddl_list_open(tokens, current_index) {
        return true;
    }
    if current.kind == Token::Ascii40 && is_view_alias_list_open(tokens, current_index) {
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

fn is_view_alias_list_open(tokens: &[SqlToken<'_>], open: usize) -> bool {
    if tokens
        .get(open)
        .is_none_or(|token| token.kind != Token::Ascii40)
    {
        return false;
    }
    let statement_start = tokens[..open]
        .iter()
        .rposition(|token| token.kind == Token::Ascii59)
        .map_or(0, |semicolon| semicolon + 1);
    let mut depth = 0usize;
    let mut create = false;
    let mut view = false;
    for token in &tokens[statement_start..open] {
        match token.kind {
            Token::Ascii40 | Token::Ascii91 => depth += 1,
            Token::Ascii41 | Token::Ascii93 => depth = depth.saturating_sub(1),
            Token::Create if depth == 0 => create = true,
            Token::View if depth == 0 => view = true,
            Token::As if depth == 0 => return false,
            _ => {}
        }
    }
    depth == 0 && create && view
}

fn is_ddl_list_open(tokens: &[SqlToken<'_>], open: usize) -> bool {
    if tokens
        .get(open)
        .is_none_or(|token| token.kind != Token::Ascii40)
    {
        return false;
    }
    let statement_start = tokens[..open]
        .iter()
        .rposition(|token| token.kind == Token::Ascii59)
        .map_or(0, |semicolon| semicolon + 1);
    let mut depth = 0usize;
    let mut create = false;
    let mut table = false;
    let mut index = false;
    let mut on = false;
    for token in &tokens[statement_start..open] {
        match token.kind {
            Token::Ascii40 | Token::Ascii91 => depth += 1,
            Token::Ascii41 | Token::Ascii93 => depth = depth.saturating_sub(1),
            Token::Create if depth == 0 => create = true,
            Token::Table if depth == 0 => table = true,
            Token::Index if depth == 0 => index = true,
            Token::On if depth == 0 => on = true,
            _ => {}
        }
    }
    depth == 0 && create && (table || (index && on))
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
