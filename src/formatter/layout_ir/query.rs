use std::collections::{BTreeMap, HashSet};

use pg_query::protobuf::Token;

use super::*;
use crate::formatter::ownership::{StatementSpec, StatementTokens, ViewCheckSpec};
use crate::formatter::tokens::is_join_start;

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
            let structural_end = (select + 1..statement.range.end)
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
            let end = statement_query_suffix(tokens, depths, statement, select, base_depth)
                .map_or(structural_end, |suffix| structural_end.min(suffix));
            let list_start = select_list_start(tokens, structure, select, end);
            let wrapper = (statement.range.start..select)
                .rev()
                .find(|open| {
                    tokens[*open].kind == Token::Ascii40
                        && structure
                            .matching_parenthesis(*open)
                            .is_some_and(|close| close >= end && close < statement.range.end)
                })
                .and_then(|open| {
                    structure
                        .matching_parenthesis(open)
                        .map(|close| (open, close))
                });
            queries.push(QueryBlock {
                select,
                list_start,
                end,
                base_depth,
                indent: base_depth + predicate_subquery_nesting(tokens, structure, select),
                wrapper,
                clauses: bind_query_clauses(tokens, depths, select, end, base_depth),
            });
        }
    }
    queries.sort_by_key(|query| query.select);
    queries.dedup_by_key(|query| query.select);
    queries
}

fn predicate_subquery_nesting(
    tokens: &[SqlToken<'_>],
    structure: &TokenStructure,
    select: usize,
) -> usize {
    (0..select)
        .filter(|open| {
            tokens[*open].kind == Token::Ascii40
                && structure
                    .matching_parenthesis(*open)
                    .is_some_and(|close| close > select)
                && (0..*open)
                    .rev()
                    .find(|previous| !tokens[*previous].is_comment())
                    .is_some_and(|previous| {
                        matches!(
                            tokens[previous].kind,
                            Token::InP | Token::Exists | Token::Any | Token::All
                        )
                    })
        })
        .count()
}

fn statement_query_suffix(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    statement: &StatementTokens,
    select: usize,
    base_depth: usize,
) -> Option<usize> {
    if base_depth != statement.base_depth {
        return None;
    }
    match &statement.spec {
        StatementSpec::View(spec) if spec.check != ViewCheckSpec::None => {
            (select + 1..statement.range.end).rev().find(|index| {
                depths[*index] == base_depth
                    && tokens[*index].kind == Token::With
                    && tokens.get(*index + 1).is_some_and(|next| {
                        matches!(next.kind, Token::Local | Token::Cascaded | Token::Check)
                    })
            })
        }
        StatementSpec::MaterializedView(_) => {
            (select + 1..statement.range.end).rev().find(|index| {
                depths[*index] == base_depth
                    && tokens[*index].kind == Token::With
                    && tokens
                        .get(*index + 1)
                        .is_some_and(|next| matches!(next.kind, Token::No | Token::DataP))
            })
        }
        _ => None,
    }
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
    structure: &TokenStructure,
    statements: &[StatementTokens],
) -> Result<Vec<SetOperationBlock>, FormatDiagnostic> {
    let depths = structure.depths();
    let mut owners = BTreeMap::<(usize, usize, usize), (Option<(usize, usize)>, Vec<usize>)>::new();

    for statement in statements {
        for operator in statement.range.start..statement.range.end {
            if !matches!(
                tokens[operator].kind,
                Token::Union | Token::Intersect | Token::Except
            ) {
                continue;
            }
            let base_depth = depths[operator];
            let owner_wrapper = structure
                .parenthesis_pairs()
                .iter()
                .filter(|(open, close)| {
                    **open < operator && operator < **close && depths[**open] + 1 == base_depth
                })
                .max_by_key(|(open, _)| **open)
                .map(|(open, close)| (*open, *close));
            let (owner_start, raw_owner_end) = owner_wrapper
                .map(|(open, close)| (open + 1, close))
                .unwrap_or((statement.range.start, statement.range.end));
            let owner_end =
                set_operation_owner_end(tokens, depths, operator, raw_owner_end, base_depth);
            owners
                .entry((owner_start, owner_end, base_depth))
                .or_insert_with(|| (owner_wrapper, Vec::new()))
                .1
                .push(operator);
        }
    }

    let mut result = Vec::with_capacity(owners.len());
    for ((owner_start, owner_end, base_depth), (owner_wrapper, mut operators)) in owners {
        operators.sort_unstable();
        operators.dedup();
        operators.retain(|operator| depths[*operator] == base_depth);
        if operators.is_empty() {
            continue;
        }

        let mut branches = Vec::with_capacity(operators.len() + 1);
        let mut branch_start = owner_start;
        for &operator in &operators {
            branches.push(bind_set_operation_branch(
                tokens,
                structure,
                branch_start,
                operator,
                base_depth,
            )?);
            branch_start = operator + 1;
            if let Some(modifier) = (branch_start..owner_end)
                .find(|index| !tokens[*index].is_comment())
                .filter(|index| matches!(tokens[*index].kind, Token::All | Token::Distinct))
            {
                branch_start = modifier + 1;
            }
        }
        branches.push(bind_set_operation_branch(
            tokens,
            structure,
            branch_start,
            owner_end,
            base_depth,
        )?);

        if branches.len() != operators.len() + 1 {
            return Err(FormatDiagnostic::Ownership(
                "set-operation branch cardinality disagrees with its bounded owner".into(),
            ));
        }
        result.push(SetOperationBlock {
            owner_start,
            owner_end,
            owner_wrapper,
            operators,
            branches,
            base_depth,
        });
    }
    result.sort_by_key(|operation| operation.owner_start);
    Ok(result)
}

fn set_operation_owner_end(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    last_known_operator: usize,
    end: usize,
    base_depth: usize,
) -> usize {
    (last_known_operator + 1..end)
        .find(|index| {
            depths[*index] == base_depth
                && matches!(
                    tokens[*index].kind,
                    Token::Order
                        | Token::Limit
                        | Token::Offset
                        | Token::Fetch
                        | Token::For
                        | Token::Ascii59
                )
        })
        .unwrap_or(end)
}

fn bind_set_operation_branch(
    tokens: &[SqlToken<'_>],
    structure: &TokenStructure,
    start: usize,
    mut end: usize,
    base_depth: usize,
) -> Result<SetOperationBranch, FormatDiagnostic> {
    let owned_start = start;
    let mut syntax_start = start;
    while syntax_start < end && tokens[syntax_start].is_comment() {
        syntax_start += 1;
    }
    while syntax_start < end && tokens[end - 1].is_comment() {
        end -= 1;
    }
    if syntax_start >= end {
        return Err(FormatDiagnostic::Ownership(
            "set operation contains an empty branch".into(),
        ));
    }

    let wrapper = if tokens[syntax_start].kind == Token::Ascii40 {
        structure
            .matching_parenthesis(syntax_start)
            .filter(|close| *close < end)
            .filter(|close| {
                (*close + 1..end).all(|index| tokens[index].is_comment()) || *close + 1 == end
            })
            .map(|close| (syntax_start, close))
    } else {
        None
    };
    let search_start = wrapper.map_or(syntax_start, |(open, _)| open + 1);
    let search_end = wrapper.map_or(end, |(_, close)| close);
    let query_start = (search_start..search_end)
        .find(|index| {
            !tokens[*index].is_comment()
                && matches!(
                    tokens[*index].kind,
                    Token::Select | Token::With | Token::Values
                )
                && depths_match_branch(structure.depths()[*index], base_depth, wrapper.is_some())
        })
        .ok_or_else(|| {
            FormatDiagnostic::Ownership("set-operation branch has no bounded query start".into())
        })?;

    Ok(SetOperationBranch {
        start: owned_start,
        end,
        query_start,
        wrapper,
    })
}

fn depths_match_branch(depth: usize, base_depth: usize, wrapped: bool) -> bool {
    depth == base_depth + usize::from(wrapped)
}

pub(super) fn bind_window_blocks(
    tokens: &[SqlToken<'_>],
    structure: &TokenStructure,
    queries: &[QueryBlock],
) -> Vec<WindowBlock> {
    let depths = structure.depths();
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for query in queries {
        for index in query.select..query.end {
            let candidate = match tokens[index].kind {
                Token::Over => tokens
                    .get(index + 1)
                    .and_then(|next| (next.kind == Token::Ascii40).then_some(index + 1)),
                Token::GroupP
                    if index > query.select && tokens[index - 1].kind == Token::Within =>
                {
                    tokens
                        .get(index + 1)
                        .and_then(|next| (next.kind == Token::Ascii40).then_some(index + 1))
                }
                Token::As
                    if depths[index] == query.base_depth
                        && query.clauses.window.is_some_and(|window| index > window) =>
                {
                    tokens
                        .get(index + 1)
                        .and_then(|next| (next.kind == Token::Ascii40).then_some(index + 1))
                }
                _ => None,
            };
            let Some(open) = candidate else {
                continue;
            };
            if !seen.insert(open) {
                continue;
            }
            let Some(close) = structure.matching_parenthesis(open) else {
                continue;
            };
            if close >= query.end {
                continue;
            }
            let inner_depth = depths[open] + 1;
            let mut partition_by = None;
            let mut order_by = None;
            let mut frame = None;
            for token_index in open + 1..close {
                if depths[token_index] != inner_depth {
                    continue;
                }
                match tokens[token_index].kind {
                    Token::Partition
                        if tokens
                            .get(token_index + 1)
                            .is_some_and(|next| next.kind == Token::By) =>
                    {
                        partition_by.get_or_insert(token_index);
                    }
                    Token::Order
                        if tokens
                            .get(token_index + 1)
                            .is_some_and(|next| next.kind == Token::By) =>
                    {
                        order_by.get_or_insert(token_index);
                    }
                    Token::Rows | Token::Range | Token::Groups => {
                        frame.get_or_insert(token_index);
                    }
                    _ => {}
                }
            }
            result.push(WindowBlock {
                query_start: query.select,
                open,
                close,
                partition_by,
                order_by,
                frame,
                base_depth: depths[open],
            });
        }
    }
    result.sort_by_key(|block| block.open);
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
            Token::Into => {
                clauses.into.get_or_insert(index);
            }
            Token::From
                if !is_distinct_from_operator(tokens, depths, select + 1, index, base_depth) =>
            {
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
                query.indent,
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
                query.indent,
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
                query.indent,
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
                            insert.span.base_depth,
                        );
                    }
                }
            }
            StatementLayout::Update(update) => {
                if let Some(source) = &update.from {
                    push_relation_join_predicates(&mut result, &mut seen, source);
                }
                if let Some(index) = update.where_clause {
                    push_predicate(
                        &mut result,
                        &mut seen,
                        PredicateKind::Where,
                        index,
                        update.returning.unwrap_or(update.span.end),
                        update.span.base_depth,
                        update.span.base_depth,
                    );
                }
            }
            StatementLayout::Delete(delete) => {
                if let Some(source) = &delete.using {
                    push_relation_join_predicates(&mut result, &mut seen, source);
                }
                if let Some(index) = delete.where_clause {
                    push_predicate(
                        &mut result,
                        &mut seen,
                        PredicateKind::Where,
                        index,
                        delete.returning.unwrap_or(delete.span.end),
                        delete.span.base_depth,
                        delete.span.base_depth,
                    );
                }
            }
            StatementLayout::Merge(merge) => {
                push_relation_join_predicates(&mut result, &mut seen, &merge.source);
                push_predicate(
                    &mut result,
                    &mut seen,
                    PredicateKind::MergeOn,
                    merge.on,
                    merge.branches[0].start,
                    merge.span.base_depth,
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
                            merge.span.base_depth,
                        );
                    }
                }
            }
            StatementLayout::CreateIndex(index) => {
                if let Some(where_clause) = index.where_clause {
                    push_predicate(
                        &mut result,
                        &mut seen,
                        PredicateKind::IndexWhere,
                        where_clause,
                        index.span.end,
                        index.span.base_depth,
                        index.span.base_depth,
                    );
                }
            }
            StatementLayout::CreateTable(table) => {
                for item in &table.items {
                    push_check_predicates(&mut result, &mut seen, depths, &item.checks);
                }
            }
            StatementLayout::AlterTable(table) => {
                for action in &table.actions {
                    push_check_predicates(&mut result, &mut seen, depths, &action.checks);
                }
            }
            StatementLayout::Select(_)
            | StatementLayout::Values(_)
            | StatementLayout::View(_)
            | StatementLayout::MaterializedView(_)
            | StatementLayout::Utility(_) => {}
        }
    }

    for predicate in &mut result {
        while predicate.end > predicate.start
            && tokens[predicate.end - 1].is_comment()
            && tokens[predicate.end - 1].line_breaks_before > 0
        {
            predicate.end -= 1;
        }
    }
    result.retain(|predicate| predicate.start < predicate.end);
    result.sort_by_key(|predicate| predicate.introducer);
    result
}

fn push_check_predicates(
    result: &mut Vec<PredicateBlock>,
    seen: &mut HashSet<usize>,
    depths: &[usize],
    checks: &[super::CheckPredicateBlock],
) {
    for check in checks {
        if check.open + 1 >= check.close || !seen.insert(check.introducer) {
            continue;
        }
        result.push(PredicateBlock {
            kind: PredicateKind::Check,
            introducer: check.introducer,
            start: check.open + 1,
            end: check.close,
            base_depth: depths[check.open] + 1,
            indent: check.indent,
            wrapper_close: Some(check.close),
        });
    }
}

fn push_relation_join_predicates(
    result: &mut Vec<PredicateBlock>,
    seen: &mut HashSet<usize>,
    source: &RelationSourceBlock,
) {
    for join in &source.joins {
        if let Some((introducer, end)) = join.predicate {
            push_predicate(
                result,
                seen,
                PredicateKind::JoinOn,
                introducer,
                end,
                source.base_depth,
                source.base_depth,
            );
        }
    }
}

fn push_predicate(
    result: &mut Vec<PredicateBlock>,
    seen: &mut HashSet<usize>,
    kind: PredicateKind,
    introducer: usize,
    end: usize,
    base_depth: usize,
    indent: usize,
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
        indent,
        wrapper_close: None,
    });
}

fn is_query_clause_start(tokens: &[SqlToken<'_>], index: usize) -> bool {
    match tokens[index].kind {
        Token::Into
        | Token::From
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
