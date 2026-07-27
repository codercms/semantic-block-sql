use std::collections::HashMap;

use pg_query::protobuf::Token;

use super::tokens::SqlToken;

/// Reusable structural index derived from PostgreSQL scanner tokens.
///
/// This is deliberately syntax-neutral. Statement and expression ownership
/// comes from the PostgreSQL AST; this index only answers delimiter-depth and
/// matching-parenthesis questions for already-owned token spans.
#[derive(Debug)]
pub(super) struct TokenStructure {
    depths: Vec<usize>,
    parenthesis_pairs: HashMap<usize, usize>,
}

impl TokenStructure {
    pub fn new(tokens: &[SqlToken<'_>]) -> Self {
        let mut depths = Vec::with_capacity(tokens.len());
        let mut parenthesis_pairs = HashMap::new();
        let mut stack = Vec::new();
        let mut depth = 0usize;

        for (index, token) in tokens.iter().enumerate() {
            if matches!(token.kind, Token::Ascii41 | Token::Ascii93) {
                depth = depth.saturating_sub(1);
            }
            depths.push(depth);
            match token.kind {
                Token::Ascii40 => {
                    stack.push(index);
                    depth += 1;
                }
                Token::Ascii91 => depth += 1,
                Token::Ascii41 => {
                    if let Some(open) = stack.pop() {
                        parenthesis_pairs.insert(open, index);
                    }
                }
                _ => {}
            }
        }

        Self {
            depths,
            parenthesis_pairs,
        }
    }

    pub fn depths(&self) -> &[usize] {
        &self.depths
    }

    pub fn depth(&self, index: usize) -> usize {
        self.depths[index]
    }

    pub fn parenthesis_pairs(&self) -> &HashMap<usize, usize> {
        &self.parenthesis_pairs
    }

    pub fn matching_parenthesis(&self, open: usize) -> Option<usize> {
        self.parenthesis_pairs.get(&open).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formatter::tokens::tokenize;

    #[test]
    fn indexes_depths_and_matching_parentheses_once() {
        let tokens = tokenize("SELECT fn((1 + 2), ARRAY[3]);").expect("scan succeeds");
        let structure = TokenStructure::new(&tokens);
        let opens = tokens
            .iter()
            .enumerate()
            .filter_map(|(index, token)| (token.kind == Token::Ascii40).then_some(index))
            .collect::<Vec<_>>();

        assert_eq!(opens.len(), 2);
        assert!(structure.matching_parenthesis(opens[0]).is_some());
        assert!(structure.matching_parenthesis(opens[1]).is_some());
        assert!(structure.depths().iter().copied().max().unwrap_or(0) >= 2);
    }
}
