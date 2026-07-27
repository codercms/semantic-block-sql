use pg_query::protobuf::Token;
use serde_json::Value;

use crate::formatter::FormatDiagnostic;
use crate::formatter::tokens;

/// Validates structural equivalence and exact preservation of protected text.
pub fn validate_equivalent(source: &str, formatted: &str) -> Result<(), FormatDiagnostic> {
    let source_tree = canonical_tree(source)?;
    let formatted_tree = canonical_tree(formatted)?;
    if source_tree != formatted_tree {
        return Err(FormatDiagnostic::SemanticMismatch);
    }

    let before = protected_tokens(source)?;
    let after = protected_tokens(formatted)?;
    if before != after {
        let detail = before
            .iter()
            .zip(after.iter())
            .find(|(left, right)| left != right)
            .map_or_else(
                || format!("token count differs: {} != {}", before.len(), after.len()),
                |(left, right)| format!("{left:?} != {right:?}"),
            );
        return Err(FormatDiagnostic::ProtectedTokenChanged(detail));
    }

    Ok(())
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

fn protected_tokens(source: &str) -> Result<Vec<(Token, String)>, FormatDiagnostic> {
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
        .map(|token| (token.kind, token.text.to_owned()))
        .collect())
}
