use pg_query::protobuf::{RawStmt, Token};

use super::FormatDiagnostic;
use super::tokens::SqlToken;

/// PostgreSQL statement family whose exact AST shape passed the support gate.
///
/// This is deliberately a closed sum type. Adding a statement family requires
/// an explicit validation branch and an explicit layout dispatcher, while
/// unknown future PostgreSQL nodes remain fail-safe unsupported syntax.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StatementKind {
    Select,
    Insert,
    Update,
    Delete,
    Merge,
}

/// UTF-8 byte span owned by one top-level PostgreSQL statement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SourceStatement {
    pub kind: StatementKind,
    pub start: usize,
    pub end: usize,
}

/// AST-validated top-level ownership model shared by validation and layout.
#[derive(Debug, Default)]
pub(super) struct SupportedDocument {
    statements: Vec<SourceStatement>,
}

impl SupportedDocument {
    pub fn new(statements: Vec<SourceStatement>) -> Self {
        Self { statements }
    }

    pub fn statements(&self) -> &[SourceStatement] {
        &self.statements
    }
}

/// Token-indexed form of [`SourceStatement`]. `end` is the terminal semicolon
/// token index when present, otherwise the exclusive token bound.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TokenStatement {
    pub kind: StatementKind,
    pub start: usize,
    pub end: usize,
    pub base_depth: usize,
}

pub(super) fn source_statement(
    source: &str,
    raw: &RawStmt,
    kind: StatementKind,
) -> SourceStatement {
    let start = usize::try_from(raw.stmt_location)
        .unwrap_or(0)
        .min(source.len());
    let length = usize::try_from(raw.stmt_len).unwrap_or(0);
    let mut end = if length == 0 {
        source.len()
    } else {
        start.saturating_add(length).min(source.len())
    };
    if source.as_bytes().get(end) == Some(&b';') {
        end += 1;
    }
    SourceStatement { kind, start, end }
}

pub(super) fn bind_token_statements(
    document: &SupportedDocument,
    tokens: &[SqlToken<'_>],
    depths: &[usize],
) -> Result<Vec<TokenStatement>, FormatDiagnostic> {
    let mut result = Vec::with_capacity(document.statements().len());

    for statement in document.statements() {
        let start = tokens
            .iter()
            .position(|token| token.start >= statement.start && token.start < statement.end)
            .ok_or_else(|| {
                FormatDiagnostic::Ownership(format!(
                    "statement {:?} at bytes {}..{} has no source token",
                    statement.kind, statement.start, statement.end
                ))
            })?;
        let exclusive_end = tokens
            .iter()
            .position(|token| token.start >= statement.end)
            .unwrap_or(tokens.len());
        if exclusive_end <= start {
            return Err(FormatDiagnostic::Ownership(format!(
                "statement {:?} at bytes {}..{} has an empty token span",
                statement.kind, statement.start, statement.end
            )));
        }
        let end = if tokens[exclusive_end - 1].kind == Token::Ascii59 {
            exclusive_end - 1
        } else {
            exclusive_end
        };
        let base_depth = depths[start];
        result.push(TokenStatement {
            kind: statement.kind,
            start,
            end,
            base_depth,
        });
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formatter::tokens::tokenize;

    #[test]
    fn binds_multiple_statements_to_independent_token_spans() {
        let source = "select 1;\nupdate public.items set value = 2;";
        let tokens = tokenize(source).expect("scan succeeds");
        let mut depth = 0usize;
        let depths = tokens
            .iter()
            .map(|token| {
                let current = depth;
                match token.kind {
                    Token::Ascii40 | Token::Ascii91 => depth += 1,
                    Token::Ascii41 | Token::Ascii93 => depth = depth.saturating_sub(1),
                    _ => {}
                }
                current
            })
            .collect::<Vec<_>>();
        let document = SupportedDocument::new(vec![
            SourceStatement {
                kind: StatementKind::Select,
                start: 0,
                end: 9,
            },
            SourceStatement {
                kind: StatementKind::Update,
                start: 10,
                end: source.len(),
            },
        ]);

        let bound = bind_token_statements(&document, &tokens, &depths).expect("bind succeeds");
        assert_eq!(bound.len(), 2);
        assert_eq!(tokens[bound[0].start].kind, Token::Select);
        assert_eq!(tokens[bound[0].end].kind, Token::Ascii59);
        assert_eq!(tokens[bound[1].start].kind, Token::Update);
        assert_eq!(tokens[bound[1].end].kind, Token::Ascii59);
    }
}
