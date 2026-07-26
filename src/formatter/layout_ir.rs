use pg_query::protobuf::Token;

use super::FormatDiagnostic;
use super::ownership::{StatementSpec, SupportedDocument, bind_token_statements};
use super::structure::TokenStructure;
use super::tokens::SqlToken;

mod query;
mod statement;

use self::query::{bind_predicates, bind_queries, bind_set_operations};
use self::statement::{
    bind_body_start, bind_delete, bind_insert, bind_merge, bind_select, bind_update,
    bind_with_block,
};

/// Generic token span owned by one PostgreSQL construct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TokenSpan {
    pub start: usize,
    pub end: usize,
    pub base_depth: usize,
}

/// Query-clause locations bound once for a SELECT token span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(super) struct QueryClauses {
    pub from: Option<usize>,
    pub where_clause: Option<usize>,
    pub group_by: Option<usize>,
    pub having: Option<usize>,
    pub window: Option<usize>,
    pub order_by: Option<usize>,
    pub limit: Option<usize>,
    pub offset: Option<usize>,
    pub fetch: Option<usize>,
    pub locking: Option<usize>,
}

impl QueryClauses {
    pub fn ordered_boundaries(self, end: usize) -> Vec<usize> {
        let mut result = [
            self.from,
            self.where_clause,
            self.group_by,
            self.having,
            self.window,
            self.order_by,
            self.limit,
            self.offset,
            self.fetch,
            self.locking,
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        result.push(end);
        result.sort_unstable();
        result.dedup();
        result
    }

    pub fn next_after(self, index: usize, end: usize) -> usize {
        self.ordered_boundaries(end)
            .into_iter()
            .find(|candidate| *candidate > index)
            .unwrap_or(end)
    }
}

/// One SELECT query branch, including nested and set-operation branches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct QueryBlock {
    pub select: usize,
    pub list_start: usize,
    pub end: usize,
    pub base_depth: usize,
    pub clauses: QueryClauses,
}

/// UNION / INTERSECT / EXCEPT ownership between two query branches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SetOperationBlock {
    pub operator: usize,
    pub next_branch: usize,
    pub base_depth: usize,
}

/// WITH ownership shared by SELECT and data-modifying statements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct WithBlock {
    pub with_index: usize,
    pub definitions: Vec<(usize, usize)>,
    pub body_start: usize,
    pub base_depth: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PredicateKind {
    Where,
    Having,
    JoinOn,
    ConflictTarget,
    ConflictAction,
    MergeOn,
    MergeWhen,
}

/// Predicate content owned by a clause introducer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PredicateBlock {
    pub kind: PredicateKind,
    pub introducer: usize,
    pub start: usize,
    pub end: usize,
    pub base_depth: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct OnConflictBlock {
    pub start: usize,
    pub target_open: Option<usize>,
    pub target_constraint: bool,
    pub target_where: Option<usize>,
    pub action: usize,
    pub update: bool,
    pub set: Option<usize>,
    pub action_where: Option<usize>,
    pub end: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InsertSource {
    Values { keyword: usize },
    Query { start: usize },
    DefaultValues { default: usize, values: usize },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct InsertBlock {
    pub span: TokenSpan,
    pub body_start: usize,
    pub target_open: Option<usize>,
    pub overriding: Option<usize>,
    pub source: InsertSource,
    pub rows: Vec<(usize, usize)>,
    pub on_conflict: Option<OnConflictBlock>,
    pub returning: Option<usize>,
}

impl InsertBlock {
    pub fn values_keyword(&self) -> Option<usize> {
        match self.source {
            InsertSource::Values { keyword } => Some(keyword),
            InsertSource::DefaultValues { values, .. } => Some(values),
            InsertSource::Query { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct UpdateBlock {
    pub span: TokenSpan,
    pub body_start: usize,
    pub set: usize,
    pub from: Option<usize>,
    pub where_clause: Option<usize>,
    pub returning: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct DeleteBlock {
    pub span: TokenSpan,
    pub body_start: usize,
    pub using: Option<usize>,
    pub where_clause: Option<usize>,
    pub returning: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum MergeAction {
    Delete,
    Nothing,
    Update {
        set: usize,
    },
    Insert {
        target_open: Option<usize>,
        overriding: Option<usize>,
        values: usize,
        values_open: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct MergeBranch {
    pub start: usize,
    pub condition: Option<usize>,
    pub then: usize,
    pub action: MergeAction,
    pub end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MergeBlock {
    pub span: TokenSpan,
    pub body_start: usize,
    pub using: usize,
    pub on: usize,
    pub branches: Vec<MergeBranch>,
    pub returning: Option<usize>,
}

/// Exhaustive top-level layout dispatcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum StatementLayout {
    Select(TokenSpan),
    Insert(InsertBlock),
    Update(UpdateBlock),
    Delete(DeleteBlock),
    Merge(MergeBlock),
}

/// Token-bound ownership IR consumed by all layout planners.
#[derive(Debug, Default)]
pub(super) struct LayoutDocument {
    statements: Vec<StatementLayout>,
    queries: Vec<QueryBlock>,
    with_blocks: Vec<WithBlock>,
    predicates: Vec<PredicateBlock>,
    set_operations: Vec<SetOperationBlock>,
}

impl LayoutDocument {
    pub fn bind(
        document: &SupportedDocument,
        tokens: &[SqlToken<'_>],
        structure: &TokenStructure,
    ) -> Result<Self, FormatDiagnostic> {
        let token_statements = bind_token_statements(document, tokens, structure.depths())?;
        let mut statements = Vec::with_capacity(token_statements.len());
        let mut with_blocks = Vec::new();

        for statement in &token_statements {
            let body_start = bind_body_start(tokens, structure.depths(), statement)?;
            let authored_with = tokens[statement.range.start].kind == Token::With;
            if authored_with != statement.spec.has_with() {
                return Err(FormatDiagnostic::Ownership(format!(
                    "{} WITH ownership disagrees with the validated AST shape",
                    statement.spec.family_name()
                )));
            }
            if authored_with {
                with_blocks.push(bind_with_block(tokens, structure, statement, body_start)?);
            }
            statements.push(match &statement.spec {
                StatementSpec::Select(spec) => StatementLayout::Select(bind_select(
                    tokens,
                    structure.depths(),
                    statement,
                    body_start,
                    spec,
                )?),
                StatementSpec::Insert(spec) => StatementLayout::Insert(bind_insert(
                    tokens, structure, statement, body_start, spec,
                )?),
                StatementSpec::Update(spec) => StatementLayout::Update(bind_update(
                    tokens,
                    structure.depths(),
                    statement,
                    body_start,
                    spec,
                )?),
                StatementSpec::Delete(spec) => StatementLayout::Delete(bind_delete(
                    tokens,
                    structure.depths(),
                    statement,
                    body_start,
                    spec,
                )?),
                StatementSpec::Merge(spec) => StatementLayout::Merge(bind_merge(
                    tokens, structure, statement, body_start, spec,
                )?),
            });
        }

        let queries = bind_queries(tokens, structure, &token_statements);
        let predicates = bind_predicates(tokens, structure.depths(), &queries, &statements);
        let set_operations = bind_set_operations(tokens, structure.depths(), &token_statements);

        Ok(Self {
            statements,
            queries,
            with_blocks,
            predicates,
            set_operations,
        })
    }

    pub fn queries(&self) -> &[QueryBlock] {
        &self.queries
    }

    pub fn with_blocks(&self) -> &[WithBlock] {
        &self.with_blocks
    }

    pub fn predicates(&self) -> &[PredicateBlock] {
        &self.predicates
    }

    pub fn set_operations(&self) -> &[SetOperationBlock] {
        &self.set_operations
    }

    pub fn inserts(&self) -> impl Iterator<Item = &InsertBlock> {
        self.statements
            .iter()
            .filter_map(|statement| match statement {
                StatementLayout::Insert(block) => Some(block),
                _ => None,
            })
    }

    pub fn updates(&self) -> impl Iterator<Item = &UpdateBlock> {
        self.statements
            .iter()
            .filter_map(|statement| match statement {
                StatementLayout::Update(block) => Some(block),
                _ => None,
            })
    }

    pub fn deletes(&self) -> impl Iterator<Item = &DeleteBlock> {
        self.statements
            .iter()
            .filter_map(|statement| match statement {
                StatementLayout::Delete(block) => Some(block),
                _ => None,
            })
    }

    pub fn merges(&self) -> impl Iterator<Item = &MergeBlock> {
        self.statements
            .iter()
            .filter_map(|statement| match statement {
                StatementLayout::Merge(block) => Some(block),
                _ => None,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formatter::tokens::tokenize;
    use crate::formatter::validation::parse_supported_postgresql;

    #[test]
    fn rejects_token_ownership_that_disagrees_with_the_validated_shape() {
        use crate::formatter::SourceRange;
        use crate::formatter::ownership::{
            InsertSourceSpec, InsertSpec, OverrideSpec, SourceStatement, StatementSpec,
        };

        let source = "INSERT INTO items (id) VALUES (1) RETURNING id;";
        let tokens = tokenize(source).expect("scan succeeds");
        let structure = TokenStructure::new(&tokens);
        let document = SupportedDocument::new(vec![SourceStatement {
            spec: StatementSpec::Insert(InsertSpec {
                has_with: false,
                target_columns: 1,
                overriding: OverrideSpec::None,
                source: InsertSourceSpec::Values { rows: 1 },
                conflict: None,
                // Deliberately contradict the source tokens.
                returning_items: 0,
            }),
            range: SourceRange::new(0, source.len()),
        }]);

        let error = LayoutDocument::bind(&document, &tokens, &structure)
            .expect_err("shape mismatch must fail safely");
        assert!(matches!(error, FormatDiagnostic::Ownership(_)));
        assert!(error.to_string().contains("RETURNING item count"));
    }

    #[test]
    fn binds_queries_and_predicates_inside_owned_statement_spans() {
        let source = "SELECT item.id FROM items item JOIN links link ON link.item_id = item.id WHERE item.deleted_at IS NULL;";
        let document = parse_supported_postgresql(source).expect("supported parse");
        let tokens = tokenize(source).expect("scan succeeds");
        let structure = TokenStructure::new(&tokens);
        let layout = LayoutDocument::bind(&document, &tokens, &structure).expect("bind succeeds");

        assert_eq!(layout.queries().len(), 1);
        assert_eq!(layout.predicates().len(), 2);
        assert_eq!(layout.predicates()[0].kind, PredicateKind::JoinOn);
        assert_eq!(layout.predicates()[1].kind, PredicateKind::Where);
    }
}
