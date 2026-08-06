use pg_query::protobuf::Token;

use super::FormatDiagnostic;
use super::ownership::{
    AlterTableActionGroup, CreateTableElementSpec, RelationItemSpec, StatementSpec,
    StatementTokens, SupportedDocument, TokenRange, UtilityStatementKind, bind_token_statements,
};
use super::structure::TokenStructure;
use super::tokens::SqlToken;

mod query;
mod statement;

use self::query::{bind_predicates, bind_queries, bind_set_operations, bind_window_blocks};
use self::statement::{
    bind_alter_table, bind_body_start, bind_create_index, bind_create_table, bind_delete,
    bind_insert, bind_materialized_view, bind_merge, bind_select, bind_update, bind_values,
    bind_view, bind_with_block,
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
    pub into: Option<usize>,
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
            self.into,
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

fn find_from_clause(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    start: usize,
    end: usize,
    base_depth: usize,
) -> Option<usize> {
    (start..end).find(|index| {
        depths[*index] == base_depth
            && tokens[*index].kind == Token::From
            && !is_distinct_from_operator(tokens, depths, start, *index, base_depth)
    })
}

fn is_distinct_from_operator(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    start: usize,
    from: usize,
    base_depth: usize,
) -> bool {
    let previous = |before| {
        (start..before)
            .rev()
            .find(|index| depths[*index] == base_depth && !tokens[*index].is_comment())
    };
    let Some(distinct) = previous(from).filter(|index| tokens[*index].kind == Token::Distinct)
    else {
        return false;
    };
    let Some(before_distinct) = previous(distinct) else {
        return false;
    };
    if tokens[before_distinct].kind == Token::Is {
        return true;
    }
    tokens[before_distinct].kind == Token::Not
        && previous(before_distinct).is_some_and(|index| tokens[index].kind == Token::Is)
}

/// One SELECT query branch, including nested and set-operation branches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct QueryBlock {
    pub select: usize,
    pub list_start: usize,
    pub end: usize,
    pub base_depth: usize,
    pub indent: usize,
    pub wrapper: Option<(usize, usize)>,
    pub clauses: QueryClauses,
}

/// One branch owned by a bounded UNION / INTERSECT / EXCEPT expression.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct SetOperationBranch {
    pub start: usize,
    pub end: usize,
    pub query_start: usize,
    pub wrapper: Option<(usize, usize)>,
}

/// Complete bounded ownership for one set-operation expression.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct SetOperationBlock {
    pub owner_start: usize,
    pub owner_end: usize,
    pub owner_wrapper: Option<(usize, usize)>,
    pub operators: Vec<usize>,
    pub branches: Vec<SetOperationBranch>,
    pub base_depth: usize,
}

/// Parenthesized window or ordered-aggregate specification.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WindowBlock {
    pub open: usize,
    pub close: usize,
    pub partition_by: Option<usize>,
    pub order_by: Option<usize>,
    pub frame: Option<usize>,
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
    IndexWhere,
    Check,
}

/// Predicate content owned by a clause introducer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PredicateBlock {
    pub kind: PredicateKind,
    pub introducer: usize,
    pub start: usize,
    pub end: usize,
    pub base_depth: usize,
    pub indent: usize,
    pub wrapper_close: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct RelationJoinBlock {
    pub start: usize,
    pub depth: usize,
    pub predicate: Option<(usize, usize)>,
}

/// FROM/USING relation-list ownership shared by UPDATE, DELETE, and MERGE.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct RelationSourceBlock {
    pub introducer: usize,
    pub range: TokenRange,
    pub items: Vec<TokenRange>,
    pub item_kinds: Vec<RelationItemSpec>,
    pub joins: Vec<RelationJoinBlock>,
    pub wrappers: Vec<(usize, usize, usize)>,
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
pub(super) struct SelectBlock {
    pub span: TokenSpan,
    pub query_start: usize,
    pub from: Option<RelationSourceBlock>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct UpdateBlock {
    pub span: TokenSpan,
    pub body_start: usize,
    pub set: usize,
    pub from: Option<RelationSourceBlock>,
    pub where_clause: Option<usize>,
    pub returning: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DeleteBlock {
    pub span: TokenSpan,
    pub body_start: usize,
    pub using: Option<RelationSourceBlock>,
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
    pub source: RelationSourceBlock,
    pub on: usize,
    pub branches: Vec<MergeBranch>,
    pub returning: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ViewBlock {
    pub span: TokenSpan,
    pub aliases: Option<(usize, usize)>,
    pub options: Option<(usize, usize)>,
    pub as_index: usize,
    pub query_start: usize,
    pub check_option: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MaterializedViewBlock {
    pub span: TokenSpan,
    pub aliases: Option<(usize, usize)>,
    pub using: Option<usize>,
    pub options: Option<(usize, usize)>,
    pub tablespace: Option<usize>,
    pub as_index: usize,
    pub query_start: usize,
    pub data_clause: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ValuesBlock {
    pub span: TokenSpan,
    pub keyword: usize,
    pub rows: Vec<(usize, usize)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CheckPredicateBlock {
    pub introducer: usize,
    pub open: usize,
    pub close: usize,
    pub indent: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CreateTableItem {
    pub range: TokenRange,
    pub kind: CreateTableElementSpec,
    pub checks: Vec<CheckPredicateBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CreateTableBlock {
    pub span: TokenSpan,
    pub open: Option<usize>,
    pub close: Option<usize>,
    pub items: Vec<CreateTableItem>,
    pub clauses: Vec<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CreateIndexBlock {
    pub span: TokenSpan,
    pub key_open: usize,
    pub key_close: usize,
    pub key_items: Vec<TokenRange>,
    pub include: Option<(usize, usize, usize, Vec<TokenRange>)>,
    pub with_options: Option<(usize, usize, usize, Vec<TokenRange>)>,
    pub tablespace: Option<usize>,
    pub where_clause: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AlterTableOptionList {
    pub close: usize,
    pub items: Vec<TokenRange>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AlterTableAction {
    pub range: TokenRange,
    pub group: AlterTableActionGroup,
    pub relation_options: Option<AlterTableOptionList>,
    pub checks: Vec<CheckPredicateBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct AlterTableBlock {
    pub span: TokenSpan,
    pub actions: Vec<AlterTableAction>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct UtilityBlock {
    pub span: TokenSpan,
    pub kind: UtilityStatementKind,
}

/// Exhaustive top-level layout dispatcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum StatementLayout {
    Select(SelectBlock),
    Values(ValuesBlock),
    Insert(InsertBlock),
    Update(UpdateBlock),
    Delete(DeleteBlock),
    Merge(MergeBlock),
    View(ViewBlock),
    MaterializedView(MaterializedViewBlock),
    CreateTable(CreateTableBlock),
    CreateIndex(CreateIndexBlock),
    AlterTable(AlterTableBlock),
    Utility(UtilityBlock),
}

/// Token-bound ownership IR consumed by all layout planners.
#[derive(Debug, Default)]
pub(super) struct LayoutDocument {
    statements: Vec<StatementLayout>,
    queries: Vec<QueryBlock>,
    with_blocks: Vec<WithBlock>,
    predicates: Vec<PredicateBlock>,
    set_operations: Vec<SetOperationBlock>,
    window_blocks: Vec<WindowBlock>,
}

impl LayoutDocument {
    pub fn bind(
        document: &SupportedDocument,
        tokens: &[SqlToken<'_>],
        structure: &TokenStructure,
    ) -> Result<Self, FormatDiagnostic> {
        let top_level_statements = bind_token_statements(document, tokens, structure.depths())?;
        let top_level_count = top_level_statements.len();
        let mut token_statements = top_level_statements.clone();
        let mut statements = Vec::with_capacity(token_statements.len());
        let mut with_blocks = Vec::new();

        let mut statement_index = 0;
        while statement_index < token_statements.len() {
            let statement = token_statements[statement_index].clone();
            let body_start = bind_body_start(tokens, structure.depths(), &statement)?;
            let authored_with = (statement.range.start..body_start)
                .find(|index| {
                    structure.depth(*index) == statement.base_depth && !tokens[*index].is_comment()
                })
                .is_some_and(|index| tokens[index].kind == Token::With);
            if authored_with != statement.spec.has_with() {
                return Err(FormatDiagnostic::Ownership(format!(
                    "{} WITH ownership disagrees with the validated AST shape",
                    statement.spec.family_name()
                )));
            }
            if authored_with {
                let with_block = bind_with_block(tokens, structure, &statement, body_start)?;
                if with_block.definitions.len() != statement.ctes.len() {
                    return Err(FormatDiagnostic::Ownership(format!(
                        "{} CTE ownership disagrees with the validated AST shape: expected {}, found {}",
                        statement.spec.family_name(),
                        statement.ctes.len(),
                        with_block.definitions.len()
                    )));
                }
                for (cte, &(open, close)) in statement.ctes.iter().zip(&with_block.definitions) {
                    token_statements.push(StatementTokens {
                        spec: cte.spec.clone(),
                        ctes: cte.ctes.clone(),
                        range: TokenRange::new(open + 1, close)?,
                        semicolon: None,
                        base_depth: structure.depth(open) + 1,
                    });
                }
                with_blocks.push(with_block);
            }
            let nested_select = statement_index >= top_level_count
                && matches!(&statement.spec, StatementSpec::Select(_));
            if nested_select {
                statement_index += 1;
                continue;
            }
            statements.push(match &statement.spec {
                StatementSpec::Select(spec) => StatementLayout::Select(bind_select(
                    tokens, structure, &statement, body_start, spec,
                )?),
                StatementSpec::Values(spec) => StatementLayout::Values(bind_values(
                    tokens, structure, &statement, body_start, spec,
                )?),
                StatementSpec::Insert(spec) => StatementLayout::Insert(bind_insert(
                    tokens, structure, &statement, body_start, spec,
                )?),
                StatementSpec::Update(spec) => StatementLayout::Update(bind_update(
                    tokens, structure, &statement, body_start, spec,
                )?),
                StatementSpec::Delete(spec) => StatementLayout::Delete(bind_delete(
                    tokens, structure, &statement, body_start, spec,
                )?),
                StatementSpec::Merge(spec) => StatementLayout::Merge(bind_merge(
                    tokens, structure, &statement, body_start, spec,
                )?),
                StatementSpec::View(spec) => StatementLayout::View(bind_view(
                    tokens, structure, &statement, body_start, spec,
                )?),
                StatementSpec::MaterializedView(spec) => StatementLayout::MaterializedView(
                    bind_materialized_view(tokens, structure, &statement, body_start, spec)?,
                ),
                StatementSpec::CreateTable(spec) => StatementLayout::CreateTable(
                    bind_create_table(tokens, structure, &statement, body_start, spec)?,
                ),
                StatementSpec::CreateIndex(spec) => StatementLayout::CreateIndex(
                    bind_create_index(tokens, structure, &statement, body_start, spec)?,
                ),
                StatementSpec::AlterTable(spec) => StatementLayout::AlterTable(bind_alter_table(
                    tokens, structure, &statement, body_start, spec,
                )?),
                StatementSpec::Utility(kind) => StatementLayout::Utility(UtilityBlock {
                    span: TokenSpan {
                        start: statement.range.start,
                        end: statement.range.end,
                        base_depth: statement.base_depth,
                    },
                    kind: *kind,
                }),
            });
            statement_index += 1;
        }

        let queries = bind_queries(tokens, structure, &top_level_statements);
        let predicates = bind_predicates(tokens, structure.depths(), &queries, &statements);
        let set_operations = bind_set_operations(tokens, structure, &top_level_statements)?;
        let window_blocks = bind_window_blocks(tokens, structure, &queries);

        Ok(Self {
            statements,
            queries,
            with_blocks,
            predicates,
            set_operations,
            window_blocks,
        })
    }

    pub fn queries(&self) -> &[QueryBlock] {
        &self.queries
    }

    pub fn selects(&self) -> impl Iterator<Item = &SelectBlock> {
        self.statements
            .iter()
            .filter_map(|statement| match statement {
                StatementLayout::Select(block) => Some(block),
                _ => None,
            })
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

    pub fn window_blocks(&self) -> &[WindowBlock] {
        &self.window_blocks
    }

    pub fn statement_spans(&self) -> impl Iterator<Item = TokenSpan> + '_ {
        self.statements.iter().map(|statement| match statement {
            StatementLayout::Select(block) => block.span,
            StatementLayout::Values(block) => block.span,
            StatementLayout::Insert(block) => block.span,
            StatementLayout::Update(block) => block.span,
            StatementLayout::Delete(block) => block.span,
            StatementLayout::Merge(block) => block.span,
            StatementLayout::View(block) => block.span,
            StatementLayout::MaterializedView(block) => block.span,
            StatementLayout::CreateTable(block) => block.span,
            StatementLayout::CreateIndex(block) => block.span,
            StatementLayout::AlterTable(block) => block.span,
            StatementLayout::Utility(block) => block.span,
        })
    }

    pub fn utilities(&self) -> impl Iterator<Item = &UtilityBlock> {
        self.statements
            .iter()
            .filter_map(|statement| match statement {
                StatementLayout::Utility(block) => Some(block),
                _ => None,
            })
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

    pub fn views(&self) -> impl Iterator<Item = &ViewBlock> {
        self.statements
            .iter()
            .filter_map(|statement| match statement {
                StatementLayout::View(block) => Some(block),
                _ => None,
            })
    }

    pub fn materialized_views(&self) -> impl Iterator<Item = &MaterializedViewBlock> {
        self.statements
            .iter()
            .filter_map(|statement| match statement {
                StatementLayout::MaterializedView(block) => Some(block),
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

    pub fn values(&self) -> impl Iterator<Item = &ValuesBlock> {
        self.statements
            .iter()
            .filter_map(|statement| match statement {
                StatementLayout::Values(block) => Some(block),
                _ => None,
            })
    }

    pub fn create_tables(&self) -> impl Iterator<Item = &CreateTableBlock> {
        self.statements
            .iter()
            .filter_map(|statement| match statement {
                StatementLayout::CreateTable(block) => Some(block),
                _ => None,
            })
    }

    pub fn create_indexes(&self) -> impl Iterator<Item = &CreateIndexBlock> {
        self.statements
            .iter()
            .filter_map(|statement| match statement {
                StatementLayout::CreateIndex(block) => Some(block),
                _ => None,
            })
    }

    pub fn alter_tables(&self) -> impl Iterator<Item = &AlterTableBlock> {
        self.statements
            .iter()
            .filter_map(|statement| match statement {
                StatementLayout::AlterTable(block) => Some(block),
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
            ctes: Vec::new(),
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

    #[test]
    fn query_blocks_exclude_distinctness_operators_from_clause_ownership() {
        let source = "SELECT old_value IS DISTINCT FROM new_value; SELECT item.old_value IS NOT DISTINCT FROM item.new_value FROM items item;";
        let document = parse_supported_postgresql(source).expect("supported parse");
        let tokens = tokenize(source).expect("scan succeeds");
        let structure = TokenStructure::new(&tokens);
        let layout = LayoutDocument::bind(&document, &tokens, &structure).expect("bind succeeds");

        assert_eq!(layout.queries().len(), 2);
        assert_eq!(layout.queries()[0].clauses.from, None);
        let actual_from = layout.queries()[1]
            .clauses
            .from
            .expect("the relation FROM remains owned");
        assert_eq!(tokens[actual_from].text.to_ascii_uppercase(), "FROM");
        assert_eq!(tokens[actual_from + 1].text, "items");
    }
}
