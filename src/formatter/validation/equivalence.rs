use pg_query::protobuf::Token;
use serde_json::Value;

use crate::formatter::tokens::{self, normalize_comment_trailing_whitespace};
use crate::formatter::{FormatDiagnostic, SourceRange};

pub(in crate::formatter) struct EquivalenceError {
    pub(in crate::formatter) diagnostic: FormatDiagnostic,
    pub(in crate::formatter) source_range: Option<SourceRange>,
}

#[derive(Debug)]
struct ProtectedToken {
    kind: Token,
    text: String,
    source_range: SourceRange,
}

/// Validates structural equivalence and exact preservation of protected text.
pub fn validate_equivalent(source: &str, formatted: &str) -> Result<(), FormatDiagnostic> {
    validate_equivalent_located(source, formatted).map_err(|error| error.diagnostic)
}

pub(in crate::formatter) fn validate_equivalent_located(
    source: &str,
    formatted: &str,
) -> Result<(), EquivalenceError> {
    let source_tree = canonical_tree(source).map_err(EquivalenceError::unlocated)?;
    let formatted_tree = canonical_tree(formatted).map_err(EquivalenceError::unlocated)?;
    if source_tree != formatted_tree {
        return Err(EquivalenceError::unlocated(
            FormatDiagnostic::SemanticMismatch,
        ));
    }

    let before = protected_tokens(source).map_err(EquivalenceError::unlocated)?;
    let after = protected_tokens(formatted).map_err(EquivalenceError::unlocated)?;
    if before.len() != after.len()
        || before
            .iter()
            .zip(&after)
            .any(|(left, right)| left.kind != right.kind || left.text != right.text)
    {
        let mismatch = before
            .iter()
            .zip(after.iter())
            .find(|(left, right)| left.kind != right.kind || left.text != right.text);
        let (detail, source_range) = mismatch.map_or_else(
            || {
                (
                    format!("token count differs: {} != {}", before.len(), after.len()),
                    before.get(after.len()).map(|token| token.source_range),
                )
            },
            |(left, right)| {
                (
                    format!(
                        "{:?} != {:?}",
                        (left.kind, &left.text),
                        (right.kind, &right.text)
                    ),
                    Some(left.source_range),
                )
            },
        );
        return Err(EquivalenceError {
            diagnostic: FormatDiagnostic::ProtectedTokenChanged(detail),
            source_range,
        });
    }

    Ok(())
}

impl EquivalenceError {
    fn unlocated(diagnostic: FormatDiagnostic) -> Self {
        Self {
            diagnostic,
            source_range: None,
        }
    }
}

fn canonical_tree(source: &str) -> Result<Value, FormatDiagnostic> {
    let parsed = pg_query::parse(source)
        .map_err(|error| FormatDiagnostic::PostgreSqlParse(error.to_string()))?;
    let mut tree = serde_json::to_value(&parsed.protobuf)
        .map_err(|error| FormatDiagnostic::PostgreSqlParse(error.to_string()))?;
    strip_locations(&mut tree);
    Ok(tree)
}

fn strip_locations(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            for name in ["location", "stmt_location", "stmt_len"] {
                fields.remove(name);
            }
            for child in fields.values_mut() {
                strip_locations(child);
            }
        }
        Value::Array(items) => items.iter_mut().for_each(strip_locations),
        _ => {}
    }
}

fn protected_tokens(source: &str) -> Result<Vec<ProtectedToken>, FormatDiagnostic> {
    Ok(tokens::tokenize(source)?
        .into_iter()
        .filter(|token| {
            matches!(
                token.kind,
                Token::Sconst
                    | Token::Usconst
                    | Token::Bconst
                    | Token::Xconst
                    | Token::SqlComment
                    | Token::CComment
            ) || token.text.starts_with('"')
        })
        .map(|token| {
            let text = if token.is_comment() {
                normalize_comment_trailing_whitespace(token.text).into_owned()
            } else {
                token.text.to_owned()
            };
            ProtectedToken {
                kind: token.kind,
                text,
                source_range: SourceRange::new(token.start, token.end),
            }
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_token_mismatches_retain_the_source_token_range() {
        let source = "SELECT 1; -- before\n";
        let error = validate_equivalent_located(source, "SELECT 1; -- after\n")
            .expect_err("changed comment must fail equivalence");
        let range = error.source_range.expect("protected token source range");

        assert_eq!(&source[range.start..range.end], "-- before");
        assert!(matches!(
            error.diagnostic,
            FormatDiagnostic::ProtectedTokenChanged(_)
        ));
    }
}
