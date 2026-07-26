use pg_query::protobuf::Token;

use super::*;
use crate::formatter::ownership::{
    ConflictActionSpec, DeleteSpec, InsertSourceSpec, InsertSpec, MergeActionSpec, MergeSpec,
    OverrideSpec, SelectSpec, StatementTokens, UpdateSpec,
};

pub(super) fn bind_body_start(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    statement: &StatementTokens,
) -> Result<usize, FormatDiagnostic> {
    let expected = statement.spec.expected_token();
    (statement.range.start..statement.range.end)
        .find(|index| depths[*index] == statement.base_depth && tokens[*index].kind == expected)
        .ok_or_else(|| {
            FormatDiagnostic::Ownership(format!(
                "{} statement has no top-level {:?} token",
                statement.spec.family_name(),
                expected
            ))
        })
}

pub(super) fn bind_select(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    statement: &StatementTokens,
    body_start: usize,
    spec: &SelectSpec,
) -> Result<TokenSpan, FormatDiagnostic> {
    require_count(
        "SELECT",
        "set-operation count",
        set_operation_count(
            tokens,
            depths,
            body_start,
            statement.range.end,
            statement.base_depth,
        ),
        spec.set_operations,
    )?;
    Ok(TokenSpan {
        start: statement.range.start,
        end: statement.range.end,
        base_depth: statement.base_depth,
    })
}

pub(super) fn bind_with_block(
    tokens: &[SqlToken<'_>],
    structure: &TokenStructure,
    statement: &StatementTokens,
    body_start: usize,
) -> Result<WithBlock, FormatDiagnostic> {
    let base_depth = statement.base_depth;
    let mut definitions = Vec::new();
    for index in statement.range.start + 1..body_start {
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
        with_index: statement.range.start,
        definitions,
        body_start,
        base_depth,
    })
}

pub(super) fn bind_insert(
    tokens: &[SqlToken<'_>],
    structure: &TokenStructure,
    statement: &StatementTokens,
    body_start: usize,
    spec: &InsertSpec,
) -> Result<InsertBlock, FormatDiagnostic> {
    let depths = structure.depths();
    let base_depth = statement.base_depth;
    let end = statement.range.end;
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

    let target_columns = target_open
        .map(|open| parenthesized_item_count(tokens, structure, open))
        .transpose()?
        .unwrap_or(0);
    require_count(
        "INSERT",
        "target-column count",
        target_columns,
        spec.target_columns,
    )?;
    let actual_override = bound_override(tokens, overriding)?;
    if actual_override != spec.overriding {
        return Err(FormatDiagnostic::Ownership(format!(
            "INSERT OVERRIDING ownership disagrees with the validated AST shape: expected {:?}, found {:?}",
            spec.overriding, actual_override
        )));
    }
    match (spec.source, source) {
        (InsertSourceSpec::DefaultValues, InsertSource::DefaultValues { .. }) => {}
        (
            InsertSourceSpec::Query {
                set_operations: expected,
            },
            InsertSource::Query { start },
        ) => {
            require_count(
                "INSERT source query",
                "set-operation count",
                set_operation_count(tokens, depths, start, source_end, base_depth),
                expected,
            )?;
        }
        (InsertSourceSpec::Values { rows: expected }, InsertSource::Values { .. }) => {
            require_count("INSERT", "VALUES row count", rows.len(), expected)?;
        }
        (expected, actual) => {
            return Err(FormatDiagnostic::Ownership(format!(
                "INSERT source ownership disagrees with the validated AST shape: expected {expected:?}, found {actual:?}"
            )));
        }
    }
    require_presence(
        "INSERT",
        "ON CONFLICT clause",
        on_conflict.is_some(),
        spec.conflict.is_some(),
    )?;
    if let (Some(expected), Some(actual)) = (spec.conflict, on_conflict) {
        require_presence(
            "ON CONFLICT",
            "target",
            actual.target_open.is_some() || actual.target_constraint,
            expected.has_target,
        )?;
        require_presence(
            "ON CONFLICT",
            "target WHERE",
            actual.target_where.is_some(),
            expected.has_target_where,
        )?;
        match expected.action {
            ConflictActionSpec::Nothing => {
                require_presence("ON CONFLICT", "DO UPDATE action", actual.update, false)?;
            }
            ConflictActionSpec::Update {
                assignments,
                has_where,
            } => {
                require_presence("ON CONFLICT", "DO UPDATE action", actual.update, true)?;
                let set = actual.set.ok_or_else(|| {
                    FormatDiagnostic::Ownership(
                        "validated ON CONFLICT DO UPDATE has no bound SET clause".into(),
                    )
                })?;
                let assignment_end = actual.action_where.unwrap_or(actual.end);
                require_count(
                    "ON CONFLICT",
                    "assignment count",
                    item_count(tokens, depths, set + 1, assignment_end, base_depth),
                    assignments,
                )?;
                require_presence(
                    "ON CONFLICT",
                    "action WHERE",
                    actual.action_where.is_some(),
                    has_where,
                )?;
            }
        }
    }
    let returning_items = returning
        .map(|index| item_count(tokens, depths, index + 1, end, base_depth))
        .unwrap_or(0);
    require_count(
        "INSERT",
        "RETURNING item count",
        returning_items,
        spec.returning_items,
    )?;

    Ok(InsertBlock {
        span: TokenSpan {
            start: statement.range.start,
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
    let target_constraint = find_kind(
        tokens,
        depths,
        conflict + 2,
        action,
        base_depth,
        Token::Constraint,
    )
    .is_some();
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
        target_constraint,
        target_where,
        action,
        update,
        set,
        action_where,
        end: boundary,
    })
}

pub(super) fn bind_update(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    statement: &StatementTokens,
    body_start: usize,
    spec: &UpdateSpec,
) -> Result<UpdateBlock, FormatDiagnostic> {
    let base_depth = statement.base_depth;
    let end = statement.range.end;
    let set = find_kind(tokens, depths, body_start + 1, end, base_depth, Token::Set)
        .ok_or_else(|| FormatDiagnostic::Ownership("supported UPDATE has no SET clause".into()))?;
    let from = find_kind(tokens, depths, set + 1, end, base_depth, Token::From);
    let where_clause = find_kind(tokens, depths, set + 1, end, base_depth, Token::Where);
    let returning = find_kind(tokens, depths, set + 1, end, base_depth, Token::Returning);
    let assignment_end = [from, where_clause, returning]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(end);

    require_count(
        "UPDATE",
        "assignment count",
        item_count(tokens, depths, set + 1, assignment_end, base_depth),
        spec.assignments,
    )?;
    require_count(
        "UPDATE",
        "FROM relation count",
        usize::from(from.is_some()),
        spec.from_relations,
    )?;
    require_presence(
        "UPDATE",
        "WHERE clause",
        where_clause.is_some(),
        spec.has_where,
    )?;
    require_count(
        "UPDATE",
        "RETURNING item count",
        returning
            .map(|index| item_count(tokens, depths, index + 1, end, base_depth))
            .unwrap_or(0),
        spec.returning_items,
    )?;

    Ok(UpdateBlock {
        span: TokenSpan {
            start: statement.range.start,
            end,
            base_depth,
        },
        body_start,
        set,
        from,
        where_clause,
        returning,
    })
}

pub(super) fn bind_delete(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    statement: &StatementTokens,
    body_start: usize,
    spec: &DeleteSpec,
) -> Result<DeleteBlock, FormatDiagnostic> {
    let base_depth = statement.base_depth;
    let end = statement.range.end;
    let using = find_kind(
        tokens,
        depths,
        body_start + 1,
        end,
        base_depth,
        Token::Using,
    );
    let where_clause = find_kind(
        tokens,
        depths,
        body_start + 1,
        end,
        base_depth,
        Token::Where,
    );
    let returning = find_kind(
        tokens,
        depths,
        body_start + 1,
        end,
        base_depth,
        Token::Returning,
    );

    require_count(
        "DELETE",
        "USING relation count",
        usize::from(using.is_some()),
        spec.using_relations,
    )?;
    require_presence(
        "DELETE",
        "WHERE clause",
        where_clause.is_some(),
        spec.has_where,
    )?;
    require_count(
        "DELETE",
        "RETURNING item count",
        returning
            .map(|index| item_count(tokens, depths, index + 1, end, base_depth))
            .unwrap_or(0),
        spec.returning_items,
    )?;

    Ok(DeleteBlock {
        span: TokenSpan {
            start: statement.range.start,
            end,
            base_depth,
        },
        body_start,
        using,
        where_clause,
        returning,
    })
}

pub(super) fn bind_merge(
    tokens: &[SqlToken<'_>],
    structure: &TokenStructure,
    statement: &StatementTokens,
    body_start: usize,
    spec: &MergeSpec,
) -> Result<MergeBlock, FormatDiagnostic> {
    let depths = structure.depths();
    let base_depth = statement.base_depth;
    let end = statement.range.end;
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

    require_count("MERGE", "branch count", branches.len(), spec.branches.len())?;
    for (position, (actual, expected)) in branches.iter().zip(&spec.branches).enumerate() {
        require_presence(
            &format!("MERGE branch {position}"),
            "condition",
            actual.condition.is_some(),
            expected.has_condition,
        )?;
        match (actual.action, expected.action) {
            (MergeAction::Delete, MergeActionSpec::Delete)
            | (MergeAction::Nothing, MergeActionSpec::Nothing) => {}
            (MergeAction::Update { set }, MergeActionSpec::Update { assignments }) => {
                require_count(
                    &format!("MERGE branch {position}"),
                    "UPDATE assignment count",
                    item_count(tokens, depths, set + 1, actual.end, base_depth),
                    assignments,
                )?;
            }
            (
                MergeAction::Insert {
                    target_open,
                    overriding,
                    values_open,
                    ..
                },
                MergeActionSpec::Insert {
                    target_columns,
                    values,
                    overriding: expected_override,
                },
            ) => {
                require_count(
                    &format!("MERGE branch {position}"),
                    "INSERT target-column count",
                    target_open
                        .map(|open| parenthesized_item_count(tokens, structure, open))
                        .transpose()?
                        .unwrap_or(0),
                    target_columns,
                )?;
                require_count(
                    &format!("MERGE branch {position}"),
                    "INSERT value count",
                    parenthesized_item_count(tokens, structure, values_open)?,
                    values,
                )?;
                let actual_override = bound_override(tokens, overriding)?;
                if actual_override != expected_override {
                    return Err(FormatDiagnostic::Ownership(format!(
                        "MERGE branch {position} OVERRIDING ownership disagrees with the validated AST shape: expected {expected_override:?}, found {actual_override:?}"
                    )));
                }
            }
            (actual, expected) => {
                return Err(FormatDiagnostic::Ownership(format!(
                    "MERGE branch {position} action ownership disagrees with the validated AST shape: expected {expected:?}, found {actual:?}"
                )));
            }
        }
    }
    require_count(
        "MERGE",
        "RETURNING item count",
        returning
            .map(|index| item_count(tokens, depths, index + 1, end, base_depth))
            .unwrap_or(0),
        spec.returning_items,
    )?;

    Ok(MergeBlock {
        span: TokenSpan {
            start: statement.range.start,
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

fn set_operation_count(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    start: usize,
    end: usize,
    depth: usize,
) -> usize {
    (start..end)
        .filter(|index| {
            depths[*index] == depth
                && matches!(
                    tokens[*index].kind,
                    Token::Union | Token::Intersect | Token::Except
                )
        })
        .count()
}

fn require_presence(
    owner: &str,
    field: &str,
    actual: bool,
    expected: bool,
) -> Result<(), FormatDiagnostic> {
    if actual == expected {
        return Ok(());
    }
    Err(FormatDiagnostic::Ownership(format!(
        "{owner} {field} ownership disagrees with the validated AST shape: expected {expected}, found {actual}"
    )))
}

fn require_count(
    owner: &str,
    field: &str,
    actual: usize,
    expected: usize,
) -> Result<(), FormatDiagnostic> {
    if actual == expected {
        return Ok(());
    }
    Err(FormatDiagnostic::Ownership(format!(
        "{owner} {field} ownership disagrees with the validated AST shape: expected {expected}, found {actual}"
    )))
}

fn item_count(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    start: usize,
    end: usize,
    depth: usize,
) -> usize {
    if start >= end {
        return 0;
    }
    let has_item = (start..end).any(|index| {
        depths[index] >= depth && !matches!(tokens[index].kind, Token::SqlComment | Token::CComment)
    });
    if !has_item {
        return 0;
    }
    1 + (start..end)
        .filter(|index| depths[*index] == depth && tokens[*index].kind == Token::Ascii44)
        .count()
}

fn parenthesized_item_count(
    tokens: &[SqlToken<'_>],
    structure: &TokenStructure,
    open: usize,
) -> Result<usize, FormatDiagnostic> {
    let close = structure.matching_parenthesis(open).ok_or_else(|| {
        FormatDiagnostic::Ownership(format!(
            "parenthesized list at token {open} has no closing parenthesis"
        ))
    })?;
    Ok(item_count(
        tokens,
        structure.depths(),
        open + 1,
        close,
        structure.depth(open) + 1,
    ))
}

fn bound_override(
    tokens: &[SqlToken<'_>],
    overriding: Option<usize>,
) -> Result<OverrideSpec, FormatDiagnostic> {
    let Some(index) = overriding else {
        return Ok(OverrideSpec::None);
    };
    match tokens.get(index + 1).map(|token| token.kind) {
        Some(Token::User) => Ok(OverrideSpec::User),
        Some(Token::SystemP) => Ok(OverrideSpec::System),
        _ => Err(FormatDiagnostic::Ownership(
            "OVERRIDING clause has no USER or SYSTEM mode".into(),
        )),
    }
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
