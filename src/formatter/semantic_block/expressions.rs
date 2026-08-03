use super::lists::{is_function_call_open, owned_list_item_ranges};
use super::*;
use crate::formatter::layout_ir::{DeleteBlock, UpdateBlock, ValuesBlock};

pub(super) struct ExpressionSources<'a> {
    pub predicates: &'a [PredicateBlock],
    pub queries: &'a [QueryBlock],
    pub inserts: &'a [InsertBlock],
    pub updates: &'a [UpdateBlock],
    pub deletes: &'a [DeleteBlock],
    pub merges: &'a [MergeBlock],
    pub values: &'a [ValuesBlock],
    pub lists: &'a [ParenthesizedList],
    pub cases: &'a [CaseRange],
}

#[derive(Debug, Clone, Copy)]
struct OwnedListSpec {
    start: usize,
    end: usize,
    base_depth: usize,
    root_indent: usize,
    kind: ExpressionOwnerKind,
    assignment: bool,
}

#[derive(Debug, Clone, Copy)]
struct CaseExpressionSpec {
    start: usize,
    end: usize,
    case: CaseRange,
    root_indent: usize,
    kind: ExpressionOwnerKind,
}

pub(super) fn owned_expression_ranges(
    context: &PlanningContext<'_, '_>,
    parens: &HashMap<usize, usize>,
    sources: ExpressionSources<'_>,
) -> Vec<ExpressionRange> {
    let mut ranges = sources
        .predicates
        .iter()
        .map(|predicate| ExpressionRange {
            kind: ExpressionOwnerKind::Predicate,
            start: predicate.start,
            end: predicate.end,
            base_depth: predicate.base_depth,
            root_indent: predicate.indent + 1,
            wrapper_close: predicate.wrapper_close,
        })
        .collect::<Vec<_>>();

    for query in sources.queries {
        let end = query
            .clauses
            .ordered_boundaries(query.end)
            .into_iter()
            .next()
            .unwrap_or(query.end);
        add_list_expressions(
            context,
            parens,
            OwnedListSpec {
                start: query.list_start,
                end,
                base_depth: query.base_depth,
                root_indent: query.indent + 1,
                kind: ExpressionOwnerKind::SelectTarget,
                assignment: false,
            },
            &mut ranges,
        );
    }

    for insert in sources.inserts {
        if let Some(returning) = insert.returning {
            add_list_expressions(
                context,
                parens,
                OwnedListSpec {
                    start: returning + 1,
                    end: insert.span.end,
                    base_depth: insert.span.base_depth,
                    root_indent: insert.span.base_depth + 1,
                    kind: ExpressionOwnerKind::ReturningItem,
                    assignment: false,
                },
                &mut ranges,
            );
        }
        if let Some(conflict) = insert.on_conflict {
            if let Some(set) = conflict.set {
                add_list_expressions(
                    context,
                    parens,
                    OwnedListSpec {
                        start: set + 1,
                        end: conflict
                            .action_where
                            .or(insert.returning)
                            .unwrap_or(insert.span.end),
                        base_depth: insert.span.base_depth,
                        root_indent: insert.span.base_depth + 1,
                        kind: ExpressionOwnerKind::AssignmentValue,
                        assignment: true,
                    },
                    &mut ranges,
                );
            }
        }
        for &(open, close) in &insert.rows {
            add_values_expressions(
                context,
                parens,
                open,
                close,
                insert.span.base_depth,
                &mut ranges,
            );
        }
    }

    for update in sources.updates {
        let set_end = update
            .from
            .as_ref()
            .map(|source| source.introducer)
            .or(update.where_clause)
            .or(update.returning)
            .unwrap_or(update.span.end);
        add_list_expressions(
            context,
            parens,
            OwnedListSpec {
                start: update.set + 1,
                end: set_end,
                base_depth: update.span.base_depth,
                root_indent: update.span.base_depth + 1,
                kind: ExpressionOwnerKind::AssignmentValue,
                assignment: true,
            },
            &mut ranges,
        );
        if let Some(returning) = update.returning {
            add_list_expressions(
                context,
                parens,
                OwnedListSpec {
                    start: returning + 1,
                    end: update.span.end,
                    base_depth: update.span.base_depth,
                    root_indent: update.span.base_depth + 1,
                    kind: ExpressionOwnerKind::ReturningItem,
                    assignment: false,
                },
                &mut ranges,
            );
        }
    }

    for delete in sources.deletes {
        if let Some(returning) = delete.returning {
            add_list_expressions(
                context,
                parens,
                OwnedListSpec {
                    start: returning + 1,
                    end: delete.span.end,
                    base_depth: delete.span.base_depth,
                    root_indent: delete.span.base_depth + 1,
                    kind: ExpressionOwnerKind::ReturningItem,
                    assignment: false,
                },
                &mut ranges,
            );
        }
    }

    for merge in sources.merges {
        for branch in &merge.branches {
            if let MergeAction::Update { set } = branch.action {
                add_list_expressions(
                    context,
                    parens,
                    OwnedListSpec {
                        start: set + 1,
                        end: branch.end,
                        base_depth: merge.span.base_depth,
                        root_indent: merge.span.base_depth + 1,
                        kind: ExpressionOwnerKind::AssignmentValue,
                        assignment: true,
                    },
                    &mut ranges,
                );
            }
            if let MergeAction::Insert { values_open, .. } = branch.action {
                if let Some(&close) = parens.get(&values_open) {
                    add_values_expressions(
                        context,
                        parens,
                        values_open,
                        close,
                        merge.span.base_depth + 1,
                        &mut ranges,
                    );
                }
            }
        }
        if let Some(returning) = merge.returning {
            add_list_expressions(
                context,
                parens,
                OwnedListSpec {
                    start: returning + 1,
                    end: merge.span.end,
                    base_depth: merge.span.base_depth,
                    root_indent: merge.span.base_depth + 1,
                    kind: ExpressionOwnerKind::ReturningItem,
                    assignment: false,
                },
                &mut ranges,
            );
        }
    }

    for values in sources.values {
        for &(open, close) in &values.rows {
            add_values_expressions(
                context,
                parens,
                open,
                close,
                values.span.base_depth,
                &mut ranges,
            );
        }
    }

    for case in sources.cases {
        add_case_expressions(context, parens, *case, &mut ranges);
    }

    for list in sources
        .lists
        .iter()
        .filter(|list| is_function_call_open(context.tokens, list.open))
    {
        add_list_expressions(
            context,
            parens,
            OwnedListSpec {
                start: list.open + 1,
                end: list.close,
                base_depth: context.depths[list.open] + 1,
                root_indent: function_argument_indent(context, list.open, &ranges),
                kind: ExpressionOwnerKind::FunctionArgument,
                assignment: false,
            },
            &mut ranges,
        );
    }

    ranges.retain(|range| range.start < range.end);
    ranges.sort_unstable_by_key(|range| (range.start, range.end));
    ranges.dedup_by_key(|range| (range.start, range.end));
    ranges
}

fn add_list_expressions(
    context: &PlanningContext<'_, '_>,
    parens: &HashMap<usize, usize>,
    spec: OwnedListSpec,
    result: &mut Vec<ExpressionRange>,
) {
    if spec.start >= spec.end {
        return;
    }
    for (item_start, item_end) in owned_list_item_ranges(
        context.tokens,
        context.depths,
        context.cases,
        context.lists,
        spec.start,
        spec.end,
        spec.base_depth,
    ) {
        let mut expression_start = first_syntax(context.tokens, item_start, item_end);
        if spec.assignment {
            if let Some(equals) = (expression_start..item_end).find(|index| {
                context.depths[*index] == spec.base_depth
                    && context.tokens[*index].kind == Token::Ascii61
            }) {
                expression_start = first_syntax(context.tokens, equals + 1, item_end);
            }
        }
        let expression_end = expression_end(
            context.tokens,
            context.depths,
            parens,
            expression_start,
            item_end,
            spec.base_depth,
        );
        if expression_start < expression_end {
            result.push(ExpressionRange {
                kind: spec.kind,
                start: expression_start,
                end: expression_end,
                base_depth: spec.base_depth,
                root_indent: spec.root_indent,
                wrapper_close: None,
            });
        }
    }
}

fn add_values_expressions(
    context: &PlanningContext<'_, '_>,
    parens: &HashMap<usize, usize>,
    open: usize,
    close: usize,
    owner_indent: usize,
    result: &mut Vec<ExpressionRange>,
) {
    add_list_expressions(
        context,
        parens,
        OwnedListSpec {
            start: open + 1,
            end: close,
            base_depth: context.depths[open] + 1,
            root_indent: owner_indent + 1,
            kind: ExpressionOwnerKind::ValuesItem,
            assignment: false,
        },
        result,
    );
}

fn add_case_expressions(
    context: &PlanningContext<'_, '_>,
    parens: &HashMap<usize, usize>,
    case: CaseRange,
    result: &mut Vec<ExpressionRange>,
) {
    let branch_root_indent = enclosing_root_indent(context, case.start, result) + 1;
    let mut nested = 0usize;
    let mut condition_start = None;
    let mut result_start = None;
    for index in case.start + 1..=case.end {
        match context.tokens[index].kind {
            Token::Case => nested += 1,
            Token::EndP if nested > 0 => nested -= 1,
            Token::When if nested == 0 => {
                finish_case_result(
                    context,
                    parens,
                    result_start.take(),
                    index,
                    case,
                    branch_root_indent,
                    result,
                );
                condition_start = Some(index + 1);
            }
            Token::Then if nested == 0 => {
                if let Some(start) = condition_start.take() {
                    push_case_expression(
                        context,
                        parens,
                        CaseExpressionSpec {
                            start,
                            end: index,
                            case,
                            root_indent: branch_root_indent,
                            kind: ExpressionOwnerKind::CaseCondition,
                        },
                        result,
                    );
                }
                result_start = Some(index + 1);
            }
            Token::Else if nested == 0 => {
                finish_case_result(
                    context,
                    parens,
                    result_start.take(),
                    index,
                    case,
                    branch_root_indent,
                    result,
                );
                result_start = Some(index + 1);
            }
            Token::EndP if nested == 0 => {
                finish_case_result(
                    context,
                    parens,
                    result_start.take(),
                    index,
                    case,
                    branch_root_indent,
                    result,
                );
            }
            _ => {}
        }
    }
}

fn finish_case_result(
    context: &PlanningContext<'_, '_>,
    parens: &HashMap<usize, usize>,
    start: Option<usize>,
    end: usize,
    case: CaseRange,
    root_indent: usize,
    result: &mut Vec<ExpressionRange>,
) {
    if let Some(start) = start {
        push_case_expression(
            context,
            parens,
            CaseExpressionSpec {
                start,
                end,
                case,
                root_indent,
                kind: ExpressionOwnerKind::CaseResult,
            },
            result,
        );
    }
}

fn push_case_expression(
    context: &PlanningContext<'_, '_>,
    parens: &HashMap<usize, usize>,
    spec: CaseExpressionSpec,
    result: &mut Vec<ExpressionRange>,
) {
    let start = first_syntax(context.tokens, spec.start, spec.end);
    let end = expression_end(
        context.tokens,
        context.depths,
        parens,
        start,
        spec.end,
        context.depths[spec.case.start],
    );
    if start < end {
        result.push(ExpressionRange {
            kind: spec.kind,
            start,
            end,
            base_depth: context.depths[spec.case.start],
            root_indent: spec.root_indent,
            wrapper_close: None,
        });
    }
}

fn function_argument_indent(
    context: &PlanningContext<'_, '_>,
    open: usize,
    ranges: &[ExpressionRange],
) -> usize {
    enclosing_root_indent(context, open, ranges) + 1
}

fn enclosing_root_indent(
    context: &PlanningContext<'_, '_>,
    index: usize,
    ranges: &[ExpressionRange],
) -> usize {
    ranges
        .iter()
        .filter(|range| range.start <= index && index < range.end)
        .min_by_key(|range| range.end - range.start)
        .map(|range| range.root_indent + context.depths[index].saturating_sub(range.base_depth))
        .unwrap_or(context.depths[index])
}

fn first_syntax(tokens: &[SqlToken<'_>], start: usize, end: usize) -> usize {
    (start..end)
        .find(|index| !tokens[*index].is_comment())
        .unwrap_or(end)
}

fn expression_end(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    parens: &HashMap<usize, usize>,
    start: usize,
    end: usize,
    base_depth: usize,
) -> usize {
    let alias = (start..end)
        .find(|index| depths[*index] == base_depth && tokens[*index].kind == Token::As)
        .unwrap_or(end);
    if start < alias
        && tokens[start].kind == Token::Ascii40
        && parens
            .get(&start)
            .is_some_and(|close| *close < alias && tokens[*close].kind == Token::Ascii41)
    {
        let close = parens[&start];
        if (close + 1..alias).all(|index| tokens[index].is_comment()) {
            return close + 1;
        }
    }
    alias
}
