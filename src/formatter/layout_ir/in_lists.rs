use pg_query::protobuf::Token;

use super::{FormatDiagnostic, SqlToken, TokenStructure};
use crate::formatter::{ownership::InListSpec, tokens::next_non_comment};

pub(super) fn bind(
    tokens: &[SqlToken<'_>],
    structure: &TokenStructure,
    specs: &[InListSpec],
) -> Result<Vec<(usize, usize)>, FormatDiagnostic> {
    let mut lists = Vec::new();
    for spec in specs {
        let anchor = tokens
            .iter()
            .position(|token| token.start == spec.location)
            .ok_or_else(|| FormatDiagnostic::Ownership("IN list location is missing".into()))?;
        // NOT IN is located at NOT by the PostgreSQL parser.
        let keyword = if tokens[anchor].kind == Token::Not {
            next_non_comment(tokens, anchor)
        } else {
            Some(anchor)
        }
        .filter(|&index| tokens[index].kind == Token::InP)
        .ok_or_else(|| FormatDiagnostic::Ownership("IN list has no keyword".into()))?;
        let open = next_non_comment(tokens, keyword)
            .filter(|&index| tokens[index].kind == Token::Ascii40)
            .ok_or_else(|| FormatDiagnostic::Ownership("IN list has no parenthesis".into()))?;
        let close = structure
            .matching_parenthesis(open)
            .ok_or_else(|| FormatDiagnostic::Ownership("IN list is unclosed".into()))?;
        let count = usize::from(
            tokens[open + 1..close]
                .iter()
                .any(|token| !token.is_comment()),
        ) + (open + 1..close)
            .filter(|&index| {
                tokens[index].kind == Token::Ascii44
                    && structure.depth(index) == structure.depth(open) + 1
            })
            .count();
        if count != spec.elements {
            return Err(FormatDiagnostic::Ownership(
                "IN list item count disagrees with its AST".into(),
            ));
        }
        lists.push((open, close));
    }
    lists.sort_unstable();
    lists.dedup();
    Ok(lists)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_in_list_cardinality_that_disagrees_with_the_ast() {
        let source = "SELECT id IN (1, 2);";
        let tokens = crate::formatter::tokens::tokenize(source).unwrap();
        let structure = TokenStructure::new(&tokens);
        let mut spec = InListSpec {
            location: source.find("IN").unwrap(),
            elements: 2,
        };
        assert_eq!(bind(&tokens, &structure, &[spec]).unwrap().len(), 1);
        spec.elements = 3;
        assert!(matches!(
            bind(&tokens, &structure, &[spec]),
            Err(FormatDiagnostic::Ownership(_))
        ));
    }
}
