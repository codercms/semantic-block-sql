use pg_query::protobuf::Token;

use super::{FormatDiagnostic, SqlToken, TokenStructure};
use crate::formatter::{ownership::FunctionCallSpec, tokens::next_non_comment};

pub(super) fn bind(
    tokens: &[SqlToken<'_>],
    structure: &TokenStructure,
    specs: &[FunctionCallSpec],
) -> Result<Vec<usize>, FormatDiagnostic> {
    let mut names = Vec::new();
    for spec in specs {
        let (location, name) = match spec {
            FunctionCallSpec::Named { location, name } => (*location, name),
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
        if structure.matching_parenthesis(open).is_none() {
            return Err(FormatDiagnostic::Ownership(
                "function argument list is unclosed".into(),
            ));
        }
        names.push(cursor);
    }
    names.sort_unstable();
    names.dedup();
    Ok(names)
}
