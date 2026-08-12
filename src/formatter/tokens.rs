use std::borrow::Cow;
use std::ops::Range;

use pg_query::protobuf::{KeywordKind, Token};

use super::FormatDiagnostic;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) enum TokenRole {
    #[default]
    Unowned,
    Identifier,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SqlToken<'a> {
    pub kind: Token,
    pub keyword_kind: KeywordKind,
    pub text: &'a str,
    pub start: usize,
    pub end: usize,
    /// Number of authored line boundaries between the previous token and this
    /// token. Two or more line boundaries represent a hard blank-line
    /// boundary.
    pub line_breaks_before: usize,
    pub role: TokenRole,
}

impl SqlToken<'_> {
    pub fn is_comment(&self) -> bool {
        matches!(self.kind, Token::SqlComment | Token::CComment)
    }
}

pub(super) fn comment_trailing_whitespace_ranges(text: &str) -> Vec<Range<usize>> {
    let mut ranges = Vec::new();
    let mut line_start = 0;
    let bytes = text.as_bytes();

    while line_start < text.len() {
        let line_end = bytes[line_start..]
            .iter()
            .position(|byte| matches!(byte, b'\r' | b'\n'))
            .map_or(text.len(), |offset| line_start + offset);
        let trimmed_end = line_start
            + text[line_start..line_end]
                .trim_end_matches(is_removable_trailing_whitespace)
                .len();
        if trimmed_end < line_end {
            ranges.push(trimmed_end..line_end);
        }
        if line_end == text.len() {
            break;
        }
        line_start = line_end
            + usize::from(bytes[line_end] == b'\r' && bytes.get(line_end + 1) == Some(&b'\n'))
            + 1;
    }

    ranges
}

pub(super) fn normalize_comment_trailing_whitespace(text: &str) -> Cow<'_, str> {
    let ranges = comment_trailing_whitespace_ranges(text);
    if ranges.is_empty() {
        return Cow::Borrowed(text);
    }

    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;
    for range in ranges {
        output.push_str(&text[cursor..range.start]);
        cursor = range.end;
    }
    output.push_str(&text[cursor..]);
    Cow::Owned(output)
}

pub(super) fn is_removable_trailing_whitespace(character: char) -> bool {
    character.is_whitespace() && !matches!(character, '\r' | '\n')
}

/// Returns true only for the first significant token of a complete JOIN header.
///
/// PostgreSQL headers may contain comments between `NATURAL`, the join type,
/// `OUTER`, and `JOIN`; callers need one shared definition so query and DML
/// relation planners break before the same token.
/// Returns true only when a token can introduce a query clause at its
/// current lexical position. Qualified identifiers such as `row.limit` or
/// `row.for` are never clause starts. Callers that distinguish `FROM` from
/// `IS DISTINCT FROM` still apply that expression-specific proof separately.
pub(super) fn is_query_clause_start(tokens: &[SqlToken<'_>], index: usize) -> bool {
    if previous_non_comment(tokens, index)
        .is_some_and(|previous| tokens[previous].kind == Token::Ascii46)
    {
        return false;
    }

    match tokens[index].kind {
        Token::Into
        | Token::From
        | Token::Where
        | Token::Having
        | Token::Window
        | Token::Limit
        | Token::Offset
        | Token::Fetch
        | Token::For => true,
        Token::GroupP | Token::Order => {
            next_non_comment(tokens, index).is_some_and(|next| tokens[next].kind == Token::By)
        }
        _ => false,
    }
}

pub(super) fn is_join_start(tokens: &[SqlToken<'_>], index: usize) -> bool {
    if join_keyword(tokens, index).is_none() {
        return false;
    }

    let previous = previous_non_comment(tokens, index).map(|index| tokens[index].kind);
    match tokens[index].kind {
        Token::Natural => true,
        Token::InnerP | Token::Left | Token::Right | Token::Full | Token::Cross => {
            previous != Some(Token::Natural)
        }
        Token::Join => !previous.is_some_and(|kind| {
            matches!(
                kind,
                Token::Natural
                    | Token::InnerP
                    | Token::Left
                    | Token::Right
                    | Token::Full
                    | Token::Cross
                    | Token::OuterP
            )
        }),
        _ => false,
    }
}

fn join_keyword(tokens: &[SqlToken<'_>], start: usize) -> Option<usize> {
    let mut index = start;
    if tokens.get(index)?.kind == Token::Natural {
        index = next_non_comment(tokens, index)?;
    }

    match tokens.get(index)?.kind {
        Token::Join => Some(index),
        Token::InnerP | Token::Cross => {
            let join = next_non_comment(tokens, index)?;
            (tokens[join].kind == Token::Join).then_some(join)
        }
        Token::Left | Token::Right | Token::Full => {
            let mut join = next_non_comment(tokens, index)?;
            if tokens[join].kind == Token::OuterP {
                join = next_non_comment(tokens, join)?;
            }
            (tokens[join].kind == Token::Join).then_some(join)
        }
        _ => None,
    }
}

pub(super) fn previous_non_comment(tokens: &[SqlToken<'_>], index: usize) -> Option<usize> {
    (0..index)
        .rev()
        .find(|candidate| !tokens[*candidate].is_comment())
}

pub(super) fn next_non_comment(tokens: &[SqlToken<'_>], index: usize) -> Option<usize> {
    (index + 1..tokens.len()).find(|candidate| !tokens[*candidate].is_comment())
}

pub(super) fn tokenize(source: &str) -> Result<Vec<SqlToken<'_>>, FormatDiagnostic> {
    let scanned = pg_query::scan(source)
        .map_err(|error| FormatDiagnostic::PostgreSqlScan(error.to_string()))?;
    let mut previous_end = 0usize;
    let mut result = Vec::with_capacity(scanned.tokens.len());

    for scanned in scanned.tokens {
        let start = usize::try_from(scanned.start)
            .map_err(|_| FormatDiagnostic::PostgreSqlScan("negative token start".into()))?;
        let end = usize::try_from(scanned.end)
            .map_err(|_| FormatDiagnostic::PostgreSqlScan("negative token end".into()))?;
        let gap = source.get(previous_end..start).ok_or_else(|| {
            FormatDiagnostic::PostgreSqlScan("token gap was not valid UTF-8".into())
        })?;
        let text = source.get(start..end).ok_or_else(|| {
            FormatDiagnostic::PostgreSqlScan("token byte range was not valid UTF-8".into())
        })?;
        let kind = Token::try_from(scanned.token).map_err(|_| {
            FormatDiagnostic::PostgreSqlScan(format!("unknown PostgreSQL token {}", scanned.token))
        })?;
        let keyword_kind = KeywordKind::try_from(scanned.keyword_kind).map_err(|_| {
            FormatDiagnostic::PostgreSqlScan(format!(
                "unknown PostgreSQL keyword kind {}",
                scanned.keyword_kind
            ))
        })?;
        result.push(SqlToken {
            kind,
            keyword_kind,
            text,
            start,
            end,
            line_breaks_before: gap.bytes().filter(|byte| *byte == b'\n').count(),
            role: TokenRole::Unowned,
        });
        previous_end = end;
    }

    Ok(result)
}

#[cfg(test)]
mod comment_whitespace_tests {
    use super::*;

    #[test]
    fn normalizes_unicode_whitespace_at_physical_comment_line_ends() {
        let line = "-- note\u{a0}\t ";
        assert_eq!(normalize_comment_trailing_whitespace(line), "-- note");
        let line_ranges = comment_trailing_whitespace_ranges(line);
        assert_eq!(line_ranges.len(), 1);
        assert_eq!(line_ranges[0], 7..11);

        let block = "/* first\u{2003}\nsecond\u{3000}\r\nthird\t \r*/";
        assert_eq!(
            normalize_comment_trailing_whitespace(block),
            "/* first\nsecond\r\nthird\r*/"
        );
        assert_eq!(
            comment_trailing_whitespace_ranges(block),
            [8..11, 18..21, 28..30]
        );

        let zero_width = "-- keep\u{200b}";
        assert_eq!(
            normalize_comment_trailing_whitespace(zero_width),
            zero_width
        );
        assert!(comment_trailing_whitespace_ranges(zero_width).is_empty());
    }
}
