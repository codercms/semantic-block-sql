use pg_query::protobuf::Token;

use super::{FormatDiagnostic, SqlToken, TokenStructure};
use crate::formatter::{ownership::ArrayListSpec, tokens::next_non_comment};

pub(super) fn bind(
    tokens: &[SqlToken<'_>],
    structure: &TokenStructure,
    specs: &[ArrayListSpec],
) -> Result<Vec<(usize, usize)>, FormatDiagnostic> {
    let mut arrays = Vec::new();
    for spec in specs {
        let anchor = tokens
            .iter()
            .position(|token| token.start == spec.location)
            .ok_or_else(|| {
                FormatDiagnostic::Ownership("array constructor location is missing".into())
            })?;
        let open = match tokens[anchor].kind {
            Token::Array => next_non_comment(tokens, anchor)
                .filter(|&index| tokens[index].kind == Token::Ascii91),
            Token::Ascii91 => Some(anchor),
            _ => None,
        }
        .ok_or_else(|| {
            FormatDiagnostic::Ownership("array constructor has no owned bracket".into())
        })?;
        let close = (open + 1..tokens.len())
            .find(|&index| {
                tokens[index].kind == Token::Ascii93
                    && structure.depth(index) == structure.depth(open)
            })
            .ok_or_else(|| FormatDiagnostic::Ownership("array constructor is unclosed".into()))?;
        let has_item = tokens[open + 1..close]
            .iter()
            .any(|token| !token.is_comment());
        let count = usize::from(has_item)
            + (open + 1..close)
                .filter(|&index| {
                    tokens[index].kind == Token::Ascii44
                        && structure.depth(index) == structure.depth(open) + 1
                })
                .count();
        if count != spec.elements {
            return Err(FormatDiagnostic::Ownership(
                "array element count disagrees with its AST".into(),
            ));
        }
        arrays.push((open, close));
    }
    arrays.sort_unstable();
    arrays.dedup();
    Ok(arrays)
}
