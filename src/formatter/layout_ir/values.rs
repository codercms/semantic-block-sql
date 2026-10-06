use pg_query::protobuf::Token;

use super::*;
use crate::formatter::ownership::ValuesSpec;

/// Bind only the consecutive grammar-owned row groups after VALUES. Later
/// parentheses belong to suffix expressions, not additional rows.
pub(super) fn bind(
    tokens: &[SqlToken<'_>],
    structure: &TokenStructure,
    keyword: usize,
    end: usize,
    wrapper: Option<(usize, usize)>,
) -> Result<ValuesBlock, FormatDiagnostic> {
    let mut rows = Vec::new();
    let mut next = keyword + 1;
    loop {
        let Some(open) = (next..end).find(|&index| !tokens[index].is_comment()) else {
            break;
        };
        if tokens[open].kind != Token::Ascii40 {
            break;
        }
        let close = structure
            .matching_parenthesis(open)
            .filter(|close| *close < end)
            .ok_or_else(|| FormatDiagnostic::Ownership("VALUES row is unclosed".into()))?;
        rows.push((open, close));
        next = close + 1;
        let Some(comma) = (next..end).find(|&index| !tokens[index].is_comment()) else {
            break;
        };
        if tokens[comma].kind != Token::Ascii44 {
            break;
        }
        next = comma + 1;
    }
    let base_depth = structure.depth(keyword);
    let suffix_start = rows.last().map_or(keyword, |row| row.1);
    let clauses =
        super::query::bind_query_clauses(tokens, structure.depths(), suffix_start, end, base_depth);
    Ok(ValuesBlock {
        span: TokenSpan {
            start: keyword,
            end,
            base_depth,
        },
        keyword,
        rows,
        wrapper,
        clauses,
    })
}

pub(super) fn capability(
    values: &ValuesBlock,
    tokens: &[SqlToken<'_>],
    structure: &TokenStructure,
) -> ValuesSpec {
    let order_items = values.clauses.order_by.map_or(0, |order| {
        let end = values.clauses.next_after(order, values.span.end);
        1 + (order + 2..end)
            .filter(|&index| {
                tokens[index].kind == Token::Ascii44
                    && structure.depth(index) == values.span.base_depth
            })
            .count()
    });
    ValuesSpec {
        rows: values.rows.len(),
        order_items,
        has_limit_count: values.clauses.limit.is_some() || values.clauses.fetch.is_some(),
        has_limit_offset: values.clauses.offset.is_some(),
    }
}
