use std::collections::{HashMap, HashSet};

use pg_query::protobuf::{KeywordKind, Token};

use super::tokens::{SqlToken, tokenize};
use super::{FormatDiagnostic, FormatOptions};

#[derive(Debug, Clone, Copy)]
struct BooleanRange {
    start: usize,
    end: usize,
    base_depth: usize,
}

pub(super) fn format(source: &str, options: &FormatOptions) -> Result<String, FormatDiagnostic> {
    let tokens = tokenize(source)?;
    if tokens.is_empty() {
        return Ok(String::new());
    }

    let depths = token_depths(&tokens);
    let boolean_ranges = boolean_ranges(&tokens, &depths);
    let (boolean_opens, boolean_closes) = boolean_groups(&tokens, &depths);
    let expanded = !boolean_ranges.is_empty() || has_top_level_join(&tokens, &depths);

    let mut writer = Writer::new(options.indent_width);
    let mut previous_index = None;

    for (index, token) in tokens.iter().enumerate() {
        let previous = previous_index.map(|previous| &tokens[previous]);
        let boolean = boolean_ranges
            .iter()
            .find(|range| index >= range.start && index < range.end);

        if token.is_comment() {
            if token.newline_before {
                writer.newline(0);
            } else if !writer.at_line_start() {
                writer.space();
            }
            writer.write(token.text);
            writer.newline(boolean.map_or(0, |range| 1 + depths[index] - range.base_depth));
            previous_index = Some(index);
            continue;
        }

        if expanded && depths[index] == 0 && is_major_clause_start(&tokens, index) {
            writer.newline(0);
        }
        if depths[index] == 0 && is_join_start(&tokens, index) {
            writer.newline(0);
        }

        if let Some(range) = boolean {
            if matches!(token.kind, Token::And | Token::Or) {
                writer.newline(1 + depths[index] - range.base_depth);
            } else if boolean_closes.contains(&index) {
                writer.newline(depths[index].saturating_sub(range.base_depth).max(1));
            }
        }

        let rendered = render_token(&tokens, index);
        if needs_space(previous, token) {
            writer.space();
        }
        writer.write(&rendered);

        if let Some(range) = boolean {
            if boolean_opens.contains(&index) {
                writer.newline(1 + depths[index] + 1 - range.base_depth);
            }
        }

        if starts_expanded_boolean(&boolean_ranges, index) {
            let range = boolean_ranges
                .iter()
                .find(|range| range.start == index + 1)
                .expect("range checked above");
            writer.newline(1 + depths[index] - range.base_depth);
        }

        previous_index = Some(index);
    }

    Ok(writer.finish())
}

fn token_depths(tokens: &[SqlToken<'_>]) -> Vec<usize> {
    let mut depth = 0usize;
    tokens
        .iter()
        .map(|token| {
            if token.kind == Token::Ascii41 {
                depth = depth.saturating_sub(1);
            }
            let current = depth;
            if token.kind == Token::Ascii40 {
                depth += 1;
            }
            current
        })
        .collect()
}

fn boolean_ranges(tokens: &[SqlToken<'_>], depths: &[usize]) -> Vec<BooleanRange> {
    let mut result = Vec::new();

    for (index, token) in tokens.iter().enumerate() {
        if depths[index] != 0 || !matches!(token.kind, Token::Where | Token::On) {
            continue;
        }

        let end = (index + 1..tokens.len())
            .find(|&candidate| {
                depths[candidate] == 0
                    && (is_major_clause_start(tokens, candidate)
                        || is_join_start(tokens, candidate)
                        || tokens[candidate].kind == Token::Ascii59)
            })
            .unwrap_or(tokens.len());
        let has_connector = tokens[index + 1..end]
            .iter()
            .enumerate()
            .any(|(offset, item)| {
                let candidate = index + 1 + offset;
                depths[candidate] >= depths[index] && matches!(item.kind, Token::And | Token::Or)
            });

        if has_connector {
            result.push(BooleanRange {
                start: index + 1,
                end,
                base_depth: depths[index],
            });
        }
    }

    result
}

fn boolean_groups(tokens: &[SqlToken<'_>], depths: &[usize]) -> (HashSet<usize>, HashSet<usize>) {
    let mut stack = Vec::new();
    let mut pairs = HashMap::new();
    for (index, token) in tokens.iter().enumerate() {
        match token.kind {
            Token::Ascii40 => stack.push(index),
            Token::Ascii41 => {
                if let Some(open) = stack.pop() {
                    pairs.insert(open, index);
                }
            }
            _ => {}
        }
    }

    let mut opens = HashSet::new();
    let mut closes = HashSet::new();
    for (open, close) in pairs {
        let inner_depth = depths[open] + 1;
        let contains_boolean = (open + 1..close).any(|index| {
            depths[index] == inner_depth && matches!(tokens[index].kind, Token::And | Token::Or)
        });
        if contains_boolean {
            opens.insert(open);
            closes.insert(close);
        }
    }
    (opens, closes)
}

fn starts_expanded_boolean(ranges: &[BooleanRange], index: usize) -> bool {
    ranges.iter().any(|range| range.start == index + 1)
}

fn has_top_level_join(tokens: &[SqlToken<'_>], depths: &[usize]) -> bool {
    tokens
        .iter()
        .enumerate()
        .any(|(index, _)| depths[index] == 0 && is_join_start(tokens, index))
}

fn is_major_clause_start(tokens: &[SqlToken<'_>], index: usize) -> bool {
    match tokens[index].kind {
        Token::From
        | Token::Where
        | Token::Having
        | Token::Limit
        | Token::Offset
        | Token::Returning
        | Token::Union
        | Token::Intersect
        | Token::Except => true,
        Token::GroupP | Token::Order => tokens
            .get(index + 1)
            .is_some_and(|next| next.kind == Token::By),
        _ => false,
    }
}

fn is_join_start(tokens: &[SqlToken<'_>], index: usize) -> bool {
    let kind = tokens[index].kind;
    if kind == Token::Join {
        return index == 0
            || !matches!(
                tokens[index - 1].kind,
                Token::Left
                    | Token::Right
                    | Token::Full
                    | Token::InnerP
                    | Token::Cross
                    | Token::Natural
                    | Token::OuterP
            );
    }
    matches!(
        kind,
        Token::Left | Token::Right | Token::Full | Token::InnerP | Token::Cross | Token::Natural
    ) && tokens[index + 1..]
        .iter()
        .take(2)
        .any(|next| next.kind == Token::Join)
}

fn render_token(tokens: &[SqlToken<'_>], index: usize) -> String {
    let token = &tokens[index];
    let previous = index.checked_sub(1).map(|previous| &tokens[previous]);
    let next = tokens.get(index + 1);

    if token.kind == Token::NotEquals {
        return "!=".into();
    }
    if token.kind == Token::Ident
        && (next.is_some_and(|next| next.kind == Token::Ascii40)
            || previous.is_some_and(|previous| previous.kind == Token::Typecast))
    {
        return token.text.to_lowercase();
    }
    if is_type_keyword(token.kind)
        || (is_ordinary_function(token.kind)
            && next.is_some_and(|next| next.kind == Token::Ascii40))
    {
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

fn is_ordinary_function(kind: Token) -> bool {
    kind == Token::Ident
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
            | Token::Cross
            | Token::Distinct
            | Token::Else
            | Token::EndP
            | Token::Except
            | Token::FalseP
            | Token::From
            | Token::Full
            | Token::GroupP
            | Token::Having
            | Token::InnerP
            | Token::Intersect
            | Token::Is
            | Token::Join
            | Token::Left
            | Token::Limit
            | Token::Natural
            | Token::Not
            | Token::NullP
            | Token::Nullif
            | Token::Offset
            | Token::On
            | Token::Or
            | Token::Order
            | Token::OuterP
            | Token::Returning
            | Token::Right
            | Token::Select
            | Token::Then
            | Token::TrueP
            | Token::Union
            | Token::When
            | Token::Where
    )
}

fn is_type_keyword(kind: Token) -> bool {
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
            | Token::Interval
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

fn needs_space(previous: Option<&SqlToken<'_>>, current: &SqlToken<'_>) -> bool {
    let Some(previous) = previous else {
        return false;
    };
    if previous.is_comment() {
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
    if current.kind == Token::Ascii40
        && (previous.kind == Token::Ident
            || is_ordinary_function(previous.kind)
            || matches!(
                previous.kind,
                Token::Coalesce | Token::Nullif | Token::Greatest | Token::Least
            ))
    {
        return false;
    }
    true
}

struct Writer {
    output: String,
    indent_width: usize,
    at_line_start: bool,
}

impl Writer {
    fn new(indent_width: usize) -> Self {
        Self {
            output: String::new(),
            indent_width,
            at_line_start: true,
        }
    }

    fn at_line_start(&self) -> bool {
        self.at_line_start
    }

    fn write(&mut self, text: &str) {
        self.output.push_str(text);
        self.at_line_start = false;
    }

    fn space(&mut self) {
        if !self.at_line_start && !self.output.ends_with(' ') {
            self.output.push(' ');
        }
    }

    fn newline(&mut self, indent: usize) {
        while self.output.ends_with(' ') {
            self.output.pop();
        }
        if !self.output.is_empty() && !self.output.ends_with('\n') {
            self.output.push('\n');
        }
        if !self.output.is_empty() {
            self.output
                .extend(std::iter::repeat_n(' ', indent * self.indent_width));
        }
        self.at_line_start = true;
    }

    fn finish(mut self) -> String {
        while self.output.ends_with([' ', '\n']) {
            self.output.pop();
        }
        if !self.output.is_empty() {
            self.output.push('\n');
        }
        self.output
    }
}
