use std::collections::HashSet;

use pg_query::protobuf::Token;

use super::FormatDiagnostic;
use super::ownership::{StatementKind, SupportedDocument, TokenStatement, bind_token_statements};
use super::structure::TokenStructure;
use super::tokens::SqlToken;

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
    pub target_where: Option<usize>,
    pub action: usize,
    pub update: bool,
    pub set: Option<usize>,
    pub action_where: Option<usize>,
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
            let body_start = bind_body_start(tokens, structure.depths(), *statement)?;
            let span = TokenSpan {
                start: statement.start,
                end: statement.end,
                base_depth: statement.base_depth,
            };
            if tokens[statement.start].kind == Token::With {
                with_blocks.push(bind_with_block(tokens, structure, *statement, body_start)?);
            }
            statements.push(match statement.kind {
                StatementKind::Select => StatementLayout::Select(span),
                StatementKind::Insert => {
                    StatementLayout::Insert(bind_insert(tokens, structure, *statement, body_start)?)
                }
                StatementKind::Update => StatementLayout::Update(bind_update(
                    tokens,
                    structure.depths(),
                    *statement,
                    body_start,
                )?),
                StatementKind::Delete => StatementLayout::Delete(bind_delete(
                    tokens,
                    structure.depths(),
                    *statement,
                    body_start,
                )?),
                StatementKind::Merge => {
                    StatementLayout::Merge(bind_merge(tokens, structure, *statement, body_start)?)
                }
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

fn bind_body_start(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    statement: TokenStatement,
) -> Result<usize, FormatDiagnostic> {
    let expected = statement_token(statement.kind);
    (statement.start..statement.end)
        .find(|index| depths[*index] == statement.base_depth && tokens[*index].kind == expected)
        .ok_or_else(|| {
            FormatDiagnostic::Ownership(format!(
                "statement {:?} has no top-level {:?} token",
                statement.kind, expected
            ))
        })
}

fn statement_token(kind: StatementKind) -> Token {
    match kind {
        StatementKind::Select => Token::Select,
        StatementKind::Insert => Token::Insert,
        StatementKind::Update => Token::Update,
        StatementKind::Delete => Token::DeleteP,
        StatementKind::Merge => Token::Merge,
    }
}

fn bind_with_block(
    tokens: &[SqlToken<'_>],
    structure: &TokenStructure,
    statement: TokenStatement,
    body_start: usize,
) -> Result<WithBlock, FormatDiagnostic> {
    let base_depth = statement.base_depth;
    let mut definitions = Vec::new();
    for index in statement.start + 1..body_start {
        if structure.depth(index) != base_depth || tokens[index].kind != Token::As {
            continue;
        }
        let Some(open) = (index + 1..body_start).take(5).find(|candidate| {
            structure.depth(*candidate) == base_depth && tokens[*candidate].kind == Token::Ascii40
        }) else {
            continue;
        };
        let close = structure.matching_parenthesis(open).ok_or_else(|| {
            FormatDiagnostic::Ownership("WITH definition has no closing parenthesis".into())
        })?;
        if close >= body_start {
            return Err(FormatDiagnostic::Ownership(
                "WITH definition overlaps statement body".into(),
            ));
        }
        definitions.push((open, close));
    }
    if definitions.is_empty() {
        return Err(FormatDiagnostic::Ownership(
            "supported WITH clause has no CTE definitions".into(),
        ));
    }
    Ok(WithBlock {
        with_index: statement.start,
        definitions,
        body_start,
        base_depth,
    })
}

fn bind_insert(
    tokens: &[SqlToken<'_>],
    structure: &TokenStructure,
    statement: TokenStatement,
    body_start: usize,
) -> Result<InsertBlock, FormatDiagnostic> {
    let depths = structure.depths();
    let base_depth = statement.base_depth;
    let end = statement.end;
    let returning = find_kind(
        tokens,
        depths,
        body_start + 1,
        end,
        base_depth,
        Token::Returning,
    );
    let conflict_start = (body_start + 1..returning.unwrap_or(end)).find(|index| {
        depths[*index] == base_depth
            && tokens[*index].kind == Token::On
            && tokens
                .get(*index + 1)
                .is_some_and(|next| next.kind == Token::Conflict)
    });
    let source_end = conflict_start.or(returning).unwrap_or(end);
    let overriding = find_kind(
        tokens,
        depths,
        body_start + 1,
        source_end,
        base_depth,
        Token::Overriding,
    );
    let values = find_kind(
        tokens,
        depths,
        body_start + 1,
        source_end,
        base_depth,
        Token::Values,
    );
    let default = find_kind(
        tokens,
        depths,
        body_start + 1,
        source_end,
        base_depth,
        Token::Default,
    );
    let query_start = (body_start + 1..source_end)
        .find(|index| depths[*index] == base_depth && tokens[*index].kind == Token::Select);
    let source = match (default, values, query_start) {
        (Some(default), Some(values), _) if default + 1 == values => {
            InsertSource::DefaultValues { default, values }
        }
        (_, Some(keyword), _) => InsertSource::Values { keyword },
        (_, None, Some(start)) => InsertSource::Query { start },
        _ => {
            return Err(FormatDiagnostic::Ownership(
                "supported INSERT has no bound source".into(),
            ));
        }
    };
    let source_start = match source {
        InsertSource::Values { keyword } => keyword,
        InsertSource::DefaultValues { default, .. } => default,
        InsertSource::Query { start } => start,
    };
    let target_open = (body_start + 1..source_start).find(|index| {
        depths[*index] == base_depth
            && tokens[*index].kind == Token::Ascii40
            && structure
                .matching_parenthesis(*index)
                .is_some_and(|close| close < source_start)
    });
    let rows = values
        .map(|keyword| {
            (keyword + 1..source_end)
                .filter(|index| {
                    depths[*index] == base_depth
                        && tokens[*index].kind == Token::Ascii40
                        && structure
                            .matching_parenthesis(*index)
                            .is_some_and(|close| close < source_end)
                })
                .filter_map(|open| {
                    structure
                        .matching_parenthesis(open)
                        .map(|close| (open, close))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let on_conflict = conflict_start
        .map(|conflict| bind_on_conflict(tokens, structure, conflict, returning.unwrap_or(end)))
        .transpose()?;

    Ok(InsertBlock {
        span: TokenSpan {
            start: statement.start,
            end,
            base_depth,
        },
        body_start,
        target_open,
        overriding,
        source,
        rows,
        on_conflict,
        returning,
    })
}

fn bind_on_conflict(
    tokens: &[SqlToken<'_>],
    structure: &TokenStructure,
    conflict: usize,
    boundary: usize,
) -> Result<OnConflictBlock, FormatDiagnostic> {
    let depths = structure.depths();
    let base_depth = depths[conflict];
    let action = find_kind(
        tokens,
        depths,
        conflict + 2,
        boundary,
        base_depth,
        Token::Do,
    )
    .ok_or_else(|| FormatDiagnostic::Ownership("ON CONFLICT has no DO action".into()))?;
    let target_open = (conflict + 2..action).find(|index| {
        depths[*index] == base_depth
            && tokens[*index].kind == Token::Ascii40
            && structure
                .matching_parenthesis(*index)
                .is_some_and(|close| close < action)
    });
    let target_where = find_kind(
        tokens,
        depths,
        conflict + 2,
        action,
        base_depth,
        Token::Where,
    );
    let update = tokens
        .get(action + 1)
        .is_some_and(|token| token.kind == Token::Update);
    let set = if update {
        Some(
            find_kind(tokens, depths, action + 1, boundary, base_depth, Token::Set).ok_or_else(
                || FormatDiagnostic::Ownership("ON CONFLICT DO UPDATE has no SET clause".into()),
            )?,
        )
    } else {
        None
    };
    let action_where =
        set.and_then(|set| find_kind(tokens, depths, set + 1, boundary, base_depth, Token::Where));
    Ok(OnConflictBlock {
        start: conflict,
        target_open,
        target_where,
        action,
        update,
        set,
        action_where,
    })
}

fn bind_update(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    statement: TokenStatement,
    body_start: usize,
) -> Result<UpdateBlock, FormatDiagnostic> {
    let base_depth = statement.base_depth;
    let end = statement.end;
    let set = find_kind(tokens, depths, body_start + 1, end, base_depth, Token::Set)
        .ok_or_else(|| FormatDiagnostic::Ownership("supported UPDATE has no SET clause".into()))?;
    Ok(UpdateBlock {
        span: TokenSpan {
            start: statement.start,
            end,
            base_depth,
        },
        body_start,
        set,
        from: find_kind(tokens, depths, set + 1, end, base_depth, Token::From),
        where_clause: find_kind(tokens, depths, set + 1, end, base_depth, Token::Where),
        returning: find_kind(tokens, depths, set + 1, end, base_depth, Token::Returning),
    })
}

fn bind_delete(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    statement: TokenStatement,
    body_start: usize,
) -> Result<DeleteBlock, FormatDiagnostic> {
    let base_depth = statement.base_depth;
    let end = statement.end;
    Ok(DeleteBlock {
        span: TokenSpan {
            start: statement.start,
            end,
            base_depth,
        },
        body_start,
        using: find_kind(
            tokens,
            depths,
            body_start + 1,
            end,
            base_depth,
            Token::Using,
        ),
        where_clause: find_kind(
            tokens,
            depths,
            body_start + 1,
            end,
            base_depth,
            Token::Where,
        ),
        returning: find_kind(
            tokens,
            depths,
            body_start + 1,
            end,
            base_depth,
            Token::Returning,
        ),
    })
}

fn bind_merge(
    tokens: &[SqlToken<'_>],
    structure: &TokenStructure,
    statement: TokenStatement,
    body_start: usize,
) -> Result<MergeBlock, FormatDiagnostic> {
    let depths = structure.depths();
    let base_depth = statement.base_depth;
    let end = statement.end;
    let returning = find_kind(
        tokens,
        depths,
        body_start + 1,
        end,
        base_depth,
        Token::Returning,
    );
    let boundary = returning.unwrap_or(end);
    let using = find_kind(
        tokens,
        depths,
        body_start + 1,
        boundary,
        base_depth,
        Token::Using,
    )
    .ok_or_else(|| FormatDiagnostic::Ownership("supported MERGE has no USING clause".into()))?;
    let first_when = find_kind(tokens, depths, using + 1, boundary, base_depth, Token::When)
        .ok_or_else(|| FormatDiagnostic::Ownership("supported MERGE has no WHEN branch".into()))?;
    let on = find_kind(tokens, depths, using + 1, first_when, base_depth, Token::On)
        .ok_or_else(|| FormatDiagnostic::Ownership("supported MERGE has no ON clause".into()))?;

    let branch_starts = (first_when..boundary)
        .filter(|index| depths[*index] == base_depth && tokens[*index].kind == Token::When)
        .collect::<Vec<_>>();
    let mut branches = Vec::with_capacity(branch_starts.len());
    for (position, start) in branch_starts.iter().copied().enumerate() {
        let branch_end = branch_starts.get(position + 1).copied().unwrap_or(boundary);
        let then = find_kind(
            tokens,
            depths,
            start + 1,
            branch_end,
            base_depth,
            Token::Then,
        )
        .ok_or_else(|| FormatDiagnostic::Ownership("MERGE branch has no THEN".into()))?;
        let condition = find_kind(tokens, depths, start + 1, then, base_depth, Token::And);
        let action_start = then + 1;
        let action = match tokens.get(action_start).map(|token| token.kind) {
            Some(Token::DeleteP) => MergeAction::Delete,
            Some(Token::Update) => MergeAction::Update {
                set: find_kind(
                    tokens,
                    depths,
                    action_start + 1,
                    branch_end,
                    base_depth,
                    Token::Set,
                )
                .ok_or_else(|| {
                    FormatDiagnostic::Ownership("MERGE UPDATE action has no SET clause".into())
                })?,
            },
            Some(Token::Insert) => {
                let values = find_kind(
                    tokens,
                    depths,
                    action_start + 1,
                    branch_end,
                    base_depth,
                    Token::Values,
                )
                .ok_or_else(|| {
                    FormatDiagnostic::Ownership("MERGE INSERT action has no VALUES clause".into())
                })?;
                let target_open = (action_start + 1..values).find(|index| {
                    depths[*index] == base_depth
                        && tokens[*index].kind == Token::Ascii40
                        && structure
                            .matching_parenthesis(*index)
                            .is_some_and(|close| close < values)
                });
                let overriding = find_kind(
                    tokens,
                    depths,
                    action_start + 1,
                    values,
                    base_depth,
                    Token::Overriding,
                );
                let values_open = (values + 1..branch_end)
                    .find(|index| {
                        depths[*index] == base_depth && tokens[*index].kind == Token::Ascii40
                    })
                    .ok_or_else(|| {
                        FormatDiagnostic::Ownership(
                            "MERGE INSERT VALUES has no parenthesized row".into(),
                        )
                    })?;
                MergeAction::Insert {
                    target_open,
                    overriding,
                    values,
                    values_open,
                }
            }
            Some(Token::Do)
                if tokens
                    .get(action_start + 1)
                    .is_some_and(|token| token.kind == Token::Nothing) =>
            {
                MergeAction::Nothing
            }
            _ => {
                return Err(FormatDiagnostic::Ownership(
                    "unsupported MERGE action token ownership".into(),
                ));
            }
        };
        branches.push(MergeBranch {
            start,
            condition,
            then,
            action,
            end: branch_end,
        });
    }

    Ok(MergeBlock {
        span: TokenSpan {
            start: statement.start,
            end,
            base_depth,
        },
        body_start,
        using,
        on,
        branches,
        returning,
    })
}

fn bind_queries(
    tokens: &[SqlToken<'_>],
    structure: &TokenStructure,
    statements: &[TokenStatement],
) -> Vec<QueryBlock> {
    let depths = structure.depths();
    let mut queries = Vec::new();
    for statement in statements {
        for select in statement.start..statement.end {
            if tokens[select].kind != Token::Select {
                continue;
            }
            let base_depth = depths[select];
            let end = (select + 1..statement.end)
                .find(|index| {
                    depths[*index] < base_depth
                        || (depths[*index] == base_depth
                            && (matches!(
                                tokens[*index].kind,
                                Token::Ascii59
                                    | Token::Union
                                    | Token::Intersect
                                    | Token::Except
                                    | Token::Returning
                            ) || (tokens[*index].kind == Token::On
                                && tokens
                                    .get(*index + 1)
                                    .is_some_and(|next| next.kind == Token::Conflict))))
                })
                .unwrap_or(statement.end);
            let list_start = select_list_start(tokens, structure, select, end);
            queries.push(QueryBlock {
                select,
                list_start,
                end,
                base_depth,
                clauses: bind_query_clauses(tokens, depths, select, end, base_depth),
            });
        }
    }
    queries.sort_by_key(|query| query.select);
    queries.dedup_by_key(|query| query.select);
    queries
}

fn select_list_start(
    tokens: &[SqlToken<'_>],
    structure: &TokenStructure,
    select: usize,
    end: usize,
) -> usize {
    let mut start = select + 1;
    if start >= end {
        return start;
    }
    if matches!(tokens[start].kind, Token::Distinct | Token::All) {
        start += 1;
        if tokens[start - 1].kind == Token::Distinct
            && tokens
                .get(start)
                .is_some_and(|token| token.kind == Token::On)
            && tokens
                .get(start + 1)
                .is_some_and(|token| token.kind == Token::Ascii40)
        {
            if let Some(close) = structure.matching_parenthesis(start + 1) {
                start = close + 1;
            }
        }
    }
    start.min(end)
}

fn bind_set_operations(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    statements: &[TokenStatement],
) -> Vec<SetOperationBlock> {
    let mut result = Vec::new();
    for statement in statements {
        for operator in statement.start..statement.end {
            if !matches!(
                tokens[operator].kind,
                Token::Union | Token::Intersect | Token::Except
            ) {
                continue;
            }
            let base_depth = depths[operator];
            let next_branch = (operator + 1..statement.end)
                .find(|index| depths[*index] == base_depth && tokens[*index].kind == Token::Select);
            if let Some(next_branch) = next_branch {
                result.push(SetOperationBlock {
                    operator,
                    next_branch,
                    base_depth,
                });
            }
        }
    }
    result
}

fn bind_query_clauses(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    select: usize,
    end: usize,
    base_depth: usize,
) -> QueryClauses {
    let mut clauses = QueryClauses::default();
    for index in select + 1..end {
        if depths[index] != base_depth {
            continue;
        }
        match tokens[index].kind {
            Token::From => {
                clauses.from.get_or_insert(index);
            }
            Token::Where => {
                clauses.where_clause.get_or_insert(index);
            }
            Token::Having => {
                clauses.having.get_or_insert(index);
            }
            Token::Window => {
                clauses.window.get_or_insert(index);
            }
            Token::Limit => {
                clauses.limit.get_or_insert(index);
            }
            Token::Offset => {
                clauses.offset.get_or_insert(index);
            }
            Token::Fetch => {
                clauses.fetch.get_or_insert(index);
            }
            Token::For => {
                clauses.locking.get_or_insert(index);
            }
            Token::GroupP
                if tokens
                    .get(index + 1)
                    .is_some_and(|next| next.kind == Token::By) =>
            {
                clauses.group_by.get_or_insert(index);
            }
            Token::Order
                if tokens
                    .get(index + 1)
                    .is_some_and(|next| next.kind == Token::By) =>
            {
                clauses.order_by.get_or_insert(index);
            }
            _ => {}
        }
    }
    clauses
}

fn bind_predicates(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    queries: &[QueryBlock],
    statements: &[StatementLayout],
) -> Vec<PredicateBlock> {
    let mut result = Vec::new();
    let mut seen = HashSet::new();

    for query in queries {
        if let Some(index) = query.clauses.where_clause {
            push_predicate(
                &mut result,
                &mut seen,
                PredicateKind::Where,
                index,
                query.clauses.next_after(index, query.end),
                query.base_depth,
            );
        }
        if let Some(index) = query.clauses.having {
            push_predicate(
                &mut result,
                &mut seen,
                PredicateKind::Having,
                index,
                query.clauses.next_after(index, query.end),
                query.base_depth,
            );
        }
        for index in query.select + 1..query.end {
            if depths[index] != query.base_depth
                || tokens[index].kind != Token::On
                || tokens
                    .get(index + 1)
                    .is_some_and(|next| next.kind == Token::Conflict)
            {
                continue;
            }
            let end = (index + 1..query.end)
                .find(|candidate| {
                    depths[*candidate] < query.base_depth
                        || (depths[*candidate] == query.base_depth
                            && (is_query_clause_start(tokens, *candidate)
                                || is_join_start(tokens, *candidate)))
                })
                .unwrap_or(query.end);
            push_predicate(
                &mut result,
                &mut seen,
                PredicateKind::JoinOn,
                index,
                end,
                query.base_depth,
            );
        }
    }

    for statement in statements {
        match statement {
            StatementLayout::Insert(insert) => {
                if let Some(conflict) = insert.on_conflict {
                    if let Some(index) = conflict.target_where {
                        push_predicate(
                            &mut result,
                            &mut seen,
                            PredicateKind::ConflictTarget,
                            index,
                            conflict.action,
                            insert.span.base_depth,
                        );
                    }
                    if let Some(index) = conflict.action_where {
                        push_predicate(
                            &mut result,
                            &mut seen,
                            PredicateKind::ConflictAction,
                            index,
                            insert.returning.unwrap_or(insert.span.end),
                            insert.span.base_depth,
                        );
                    }
                }
            }
            StatementLayout::Update(update) => {
                if let Some(index) = update.where_clause {
                    push_predicate(
                        &mut result,
                        &mut seen,
                        PredicateKind::Where,
                        index,
                        update.returning.unwrap_or(update.span.end),
                        update.span.base_depth,
                    );
                }
            }
            StatementLayout::Delete(delete) => {
                if let Some(index) = delete.where_clause {
                    push_predicate(
                        &mut result,
                        &mut seen,
                        PredicateKind::Where,
                        index,
                        delete.returning.unwrap_or(delete.span.end),
                        delete.span.base_depth,
                    );
                }
            }
            StatementLayout::Merge(merge) => {
                push_predicate(
                    &mut result,
                    &mut seen,
                    PredicateKind::MergeOn,
                    merge.on,
                    merge.branches[0].start,
                    merge.span.base_depth,
                );
                for branch in &merge.branches {
                    if let Some(condition) = branch.condition {
                        push_predicate(
                            &mut result,
                            &mut seen,
                            PredicateKind::MergeWhen,
                            condition,
                            branch.then,
                            merge.span.base_depth,
                        );
                    }
                }
            }
            StatementLayout::Select(_) => {}
        }
    }

    result.sort_by_key(|predicate| predicate.introducer);
    result
}

fn push_predicate(
    result: &mut Vec<PredicateBlock>,
    seen: &mut HashSet<usize>,
    kind: PredicateKind,
    introducer: usize,
    end: usize,
    base_depth: usize,
) {
    if introducer + 1 >= end || !seen.insert(introducer) {
        return;
    }
    result.push(PredicateBlock {
        kind,
        introducer,
        start: introducer + 1,
        end,
        base_depth,
    });
}

fn find_kind(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    start: usize,
    end: usize,
    base_depth: usize,
    kind: Token,
) -> Option<usize> {
    (start..end).find(|index| depths[*index] == base_depth && tokens[*index].kind == kind)
}

fn is_query_clause_start(tokens: &[SqlToken<'_>], index: usize) -> bool {
    match tokens[index].kind {
        Token::From
        | Token::Where
        | Token::Having
        | Token::Window
        | Token::Limit
        | Token::Offset
        | Token::Fetch
        | Token::For => true,
        Token::GroupP | Token::Order => tokens
            .get(index + 1)
            .is_some_and(|next| next.kind == Token::By),
        _ => false,
    }
}

fn is_join_start(tokens: &[SqlToken<'_>], index: usize) -> bool {
    let kind = tokens[index].kind;
    if kind == Token::Join {
        return index == 0
            || !matches!(
                tokens[index - 1].kind,
                Token::Left
                    | Token::Right
                    | Token::Full
                    | Token::InnerP
                    | Token::Cross
                    | Token::Natural
                    | Token::OuterP
            );
    }
    matches!(
        kind,
        Token::Left | Token::Right | Token::Full | Token::InnerP | Token::Cross | Token::Natural
    ) && tokens[index + 1..]
        .iter()
        .take(2)
        .any(|next| next.kind == Token::Join)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formatter::tokens::tokenize;
    use crate::formatter::validation::parse_supported_postgresql;

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
