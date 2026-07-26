use std::collections::HashSet;

use pg_query::protobuf::Token;

use super::*;
use crate::formatter::ownership::StatementTokens;

pub(super) fn bind_queries(
    tokens: &[SqlToken<'_>],
    structure: &TokenStructure,
    statements: &[StatementTokens],
) -> Vec<QueryBlock> {
    let depths = structure.depths();
    let mut queries = Vec::new();
    for statement in statements {
        for select in statement.range.start..statement.range.end {
            if tokens[select].kind != Token::Select {
                continue;
            }
            let base_depth = depths[select];
            let end = (select + 1..statement.range.end)
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
                .unwrap_or(statement.range.end);
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

pub(super) fn bind_set_operations(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    statements: &[StatementTokens],
) -> Vec<SetOperationBlock> {
    let mut result = Vec::new();
    for statement in statements {
        for operator in statement.range.start..statement.range.end {
            if !matches!(
                tokens[operator].kind,
                Token::Union | Token::Intersect | Token::Except
            ) {
                continue;
            }
            let base_depth = depths[operator];
            let next_branch = (operator + 1..statement.range.end)
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

pub(super) fn bind_predicates(
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
