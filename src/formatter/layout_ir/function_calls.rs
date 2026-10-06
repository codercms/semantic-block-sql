use pg_query::protobuf::Token;

use super::{FormatDiagnostic, SqlToken, TokenStructure};
use crate::formatter::{
    ownership::{FunctionArgumentSpec, FunctionCallSpec},
    tokens::next_non_comment,
};

pub(super) fn bind(
    tokens: &[SqlToken<'_>],
    structure: &TokenStructure,
    specs: &[FunctionCallSpec],
) -> Result<Vec<super::FunctionCallBlock>, FormatDiagnostic> {
    let mut names = Vec::new();
    for spec in specs {
        let (location, name, arguments) = match spec {
            FunctionCallSpec::Named {
                location,
                name,
                arguments,
            } => (*location, name, arguments),
            FunctionCallSpec::OperatorEscape { location, keyword } => {
                let index = tokens
                    .iter()
                    .position(|token| token.start == *location && token.kind == *keyword)
                    .ok_or_else(|| {
                        FormatDiagnostic::Ownership(
                            "operator escape helper has no owned operator".into(),
                        )
                    })?;
                if *keyword == Token::Similar
                    && next_non_comment(tokens, index)
                        .is_none_or(|next| tokens[next].kind != Token::To)
                {
                    return Err(FormatDiagnostic::Ownership(
                        "SIMILAR helper has no owned TO keyword".into(),
                    ));
                }
                continue;
            }
        };
        let mut cursor = tokens
            .iter()
            .position(|token| token.start == location)
            .ok_or_else(|| {
                FormatDiagnostic::Ownership("function call location is missing".into())
            })?;
        if name.is_empty() {
            return Err(FormatDiagnostic::Ownership(
                "function call name is empty".into(),
            ));
        }
        let start = cursor;
        for (part, expected) in name.iter().enumerate() {
            if part > 0 {
                let dot = next_non_comment(tokens, cursor)
                    .filter(|&index| tokens[index].kind == Token::Ascii46)
                    .ok_or_else(|| {
                        FormatDiagnostic::Ownership("qualified function call dot is missing".into())
                    })?;
                cursor = next_non_comment(tokens, dot).ok_or_else(|| {
                    FormatDiagnostic::Ownership(
                        "qualified function call component is missing".into(),
                    )
                })?;
            }
            if !super::statement::token_matches_identifier(&tokens[cursor], expected) {
                return Err(FormatDiagnostic::Ownership(
                    "function call name disagrees with its AST".into(),
                ));
            }
        }
        let open = next_non_comment(tokens, cursor)
            .filter(|&index| tokens[index].kind == Token::Ascii40)
            .ok_or_else(|| {
                FormatDiagnostic::Ownership("function argument list is missing".into())
            })?;
        let close = structure.matching_parenthesis(open).ok_or_else(|| {
            FormatDiagnostic::Ownership("function argument list is unclosed".into())
        })?;
        let arguments = bind_arguments(tokens, structure, open, close, *arguments)?;
        names.push(super::FunctionCallBlock {
            start,
            name: cursor,
            open,
            close,
            arguments,
        });
    }
    names.sort_unstable();
    names.dedup();
    Ok(names)
}

fn bind_arguments(
    tokens: &[SqlToken<'_>],
    structure: &TokenStructure,
    open: usize,
    close: usize,
    spec: FunctionArgumentSpec,
) -> Result<super::FunctionArgumentLayout, FormatDiagnostic> {
    let FunctionArgumentSpec::KeyValuePairs { pairs, order_items } = spec else {
        return Ok(super::FunctionArgumentLayout::Ordinary);
    };
    let depth = structure.depth(open) + 1;
    let order_by = if order_items > 0 {
        Some(
            (open + 1..close)
                .find(|&index| {
                    structure.depth(index) == depth
                        && tokens[index].kind == Token::Order
                        && next_non_comment(tokens, index)
                            .is_some_and(|next| tokens[next].kind == Token::By)
                })
                .ok_or_else(|| {
                    FormatDiagnostic::Ownership("JSON aggregate ORDER BY is missing".into())
                })?,
        )
    } else {
        None
    };
    let end = order_by.unwrap_or(close);
    let count = |start: usize, end: usize| {
        1 + (start..end)
            .filter(|&index| {
                tokens[index].kind == Token::Ascii44 && structure.depth(index) == depth
            })
            .count()
    };
    if count(open + 1, end) != pairs * 2
        || order_by.is_some_and(|order| count(order + 2, close) != order_items)
    {
        return Err(FormatDiagnostic::Ownership(
            "JSON key/value arguments disagree with their AST".into(),
        ));
    }
    Ok(super::FunctionArgumentLayout::KeyValuePairs { end, order_by })
}
