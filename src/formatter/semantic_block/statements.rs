use super::lists::plan_keyword_list;
use super::*;
use crate::formatter::layout_ir::{DeleteBlock, InsertSource, RelationSourceBlock, UpdateBlock};

pub(super) fn plan_update_statements(
    context: &PlanningContext<'_, '_>,
    boolean_ranges: &[BooleanRange],
    updates: &[UpdateBlock],
    plan: &mut LayoutPlan,
) {
    let tokens = context.tokens;
    let options = context.options;
    for update in updates {
        let authored = tokens[update.span.start + 1..update.span.end]
            .iter()
            .any(|token| token.line_breaks_before > 0);
        let compact_statement_width = update.span.base_depth * INDENT_WIDTH
            + compact_width(tokens, update.span.start, update.span.end, options);
        let width_driven = compact_statement_width > options.soft_line_width;
        let has_expanded_predicate = update.where_clause.is_some_and(|where_clause| {
            boolean_ranges
                .iter()
                .any(|range| range.start == where_clause + 1 && range.end <= update.span.end)
        });
        let expanded = authored || update.from.is_some() || width_driven || has_expanded_predicate;
        if !expanded {
            continue;
        }

        plan.break_before(update.set, 1, update.span.base_depth);
        let set_end = update
            .from
            .as_ref()
            .map(|source| source.introducer)
            .or(update.where_clause)
            .or(update.returning)
            .unwrap_or(update.span.end);
        plan_keyword_list(
            context,
            update.set,
            set_end,
            update.span.base_depth,
            true,
            plan,
        );

        if let Some(from) = &update.from {
            plan.break_before(from.introducer, 1, update.span.base_depth);
            plan_relation_source(from, plan);
        }
        if let Some(where_clause) = update.where_clause {
            plan.break_before(where_clause, 1, update.span.base_depth);
        }
        if let Some(returning) = update.returning {
            plan.break_before(returning, 1, update.span.base_depth);
            plan_keyword_list(
                context,
                returning,
                update.span.end,
                update.span.base_depth,
                width_driven,
                plan,
            );
        }
    }
}

pub(super) fn plan_delete_statements(
    context: &PlanningContext<'_, '_>,
    boolean_ranges: &[BooleanRange],
    deletes: &[DeleteBlock],
    plan: &mut LayoutPlan,
) {
    let tokens = context.tokens;
    let options = context.options;
    for delete in deletes {
        let authored = tokens[delete.span.start + 1..delete.span.end]
            .iter()
            .any(|token| token.line_breaks_before > 0);
        let compact_statement_width = delete.span.base_depth * INDENT_WIDTH
            + compact_width(tokens, delete.span.start, delete.span.end, options);
        let width_driven = compact_statement_width > options.soft_line_width;
        let has_expanded_predicate = delete.where_clause.is_some_and(|where_clause| {
            boolean_ranges
                .iter()
                .any(|range| range.start == where_clause + 1 && range.end <= delete.span.end)
        });
        let expanded = authored || delete.using.is_some() || width_driven || has_expanded_predicate;
        if !expanded {
            continue;
        }

        if let Some(using) = &delete.using {
            plan.break_before(using.introducer, 1, delete.span.base_depth);
            plan_relation_source(using, plan);
        }
        if let Some(where_clause) = delete.where_clause {
            plan.break_before(where_clause, 1, delete.span.base_depth);
        }
        if let Some(returning) = delete.returning {
            plan.break_before(returning, 1, delete.span.base_depth);
            plan_keyword_list(
                context,
                returning,
                delete.span.end,
                delete.span.base_depth,
                width_driven,
                plan,
            );
        }
    }
}

pub(super) fn is_insert_list_open(inserts: &[InsertBlock], open: usize) -> bool {
    inserts.iter().any(|insert| {
        insert.target_open == Some(open)
            || insert.rows.iter().any(|&(row, _)| row == open)
            || insert
                .on_conflict
                .is_some_and(|conflict| conflict.target_open == Some(open))
    })
}

pub(super) fn is_merge_list_open(merges: &[MergeBlock], open: usize) -> bool {
    merges.iter().any(|merge| {
        merge.branches.iter().any(|branch| match branch.action {
            MergeAction::Insert {
                target_open,
                values_open,
                ..
            } => target_open == Some(open) || values_open == open,
            _ => false,
        })
    })
}

pub(super) fn plan_merge_statements(
    context: &PlanningContext<'_, '_>,
    merges: &[MergeBlock],
    plan: &mut LayoutPlan,
) {
    let tokens = context.tokens;
    let lists = context.lists;
    for merge in merges {
        plan.break_before(merge.source.introducer, 1, merge.span.base_depth);
        plan_relation_source(&merge.source, plan);
        if !merge.source.joins.is_empty()
            || merge
                .source
                .item_kinds
                .iter()
                .any(|kind| *kind != crate::formatter::ownership::RelationItemSpec::Relation)
        {
            plan.break_before(merge.on, 1, merge.span.base_depth);
        }
        for branch in &merge.branches {
            let branch_lines = if branch
                .start
                .checked_sub(1)
                .is_some_and(|previous| tokens[previous].is_comment())
            {
                1
            } else {
                2
            };
            plan.break_before(branch.start, branch_lines, merge.span.base_depth);
            match branch.action {
                MergeAction::Update { set } => {
                    plan_keyword_list(context, set, branch.end, merge.span.base_depth, true, plan);
                }
                MergeAction::Insert {
                    values,
                    values_open,
                    ..
                } => {
                    plan.break_before(values, 1, merge.span.base_depth + 1);
                    if let Some(close) = lists
                        .iter()
                        .find(|list| list.open == values_open)
                        .map(|list| list.close)
                    {
                        plan.set_indent(values_open..close + 1, merge.span.base_depth + 1);
                    }
                }
                MergeAction::Delete | MergeAction::Nothing => {}
            }
        }
        if let Some(returning) = merge.returning {
            plan.break_before(returning, 1, merge.span.base_depth);
            plan_keyword_list(
                context,
                returning,
                merge.span.end,
                merge.span.base_depth,
                false,
                plan,
            );
        }
    }
}

pub(super) fn plan_relation_source(source: &RelationSourceBlock, plan: &mut LayoutPlan) {
    let item_indent = source.base_depth + 1;
    if source.items.len() > 1 {
        for item in &source.items {
            plan.break_before(item.start, 1, item_indent);
            plan.set_indent(item.start..item.end, item_indent);
        }
    }

    let minimum_join_indent = if source.items.len() > 1 {
        item_indent
    } else {
        source.base_depth
    };
    for join in &source.joins {
        plan.break_before(join.start, 1, join.depth.max(minimum_join_indent));
    }
    for &(open, close, inner_depth) in &source.wrappers {
        plan.break_before(open + 1, 1, inner_depth);
        plan.set_indent(open + 1..close, inner_depth);
        plan.break_before(close, 1, inner_depth.saturating_sub(1));
    }
}

pub(super) fn plan_insert_statements(
    context: &PlanningContext<'_, '_>,
    inserts: &[InsertBlock],
    plan: &mut LayoutPlan,
) -> HashSet<usize> {
    let tokens = context.tokens;
    let lists = context.lists;
    let options = context.options;
    let mut query_starts = HashSet::new();
    for insert in inserts {
        let authored = tokens[insert.span.start + 1..insert.span.end]
            .iter()
            .any(|token| token.line_breaks_before > 0);
        let has_expanded_list = lists.iter().any(|list| {
            list.expanded
                && (insert.target_open == Some(list.open)
                    || insert.rows.iter().any(|&(open, _)| open == list.open)
                    || insert
                        .on_conflict
                        .is_some_and(|conflict| conflict.target_open == Some(list.open)))
        });
        let compact_statement_width = insert.span.base_depth * INDENT_WIDTH
            + compact_width(tokens, insert.span.start, insert.span.end, options);
        let width_driven = compact_statement_width > options.soft_line_width;
        let has_update = insert.on_conflict.is_some_and(|conflict| conflict.update);
        let query_source = match insert.source {
            InsertSource::Query { start } => Some(start),
            _ => None,
        };
        let expanded =
            authored || has_expanded_list || width_driven || has_update || query_source.is_some();
        if !expanded {
            continue;
        }

        if let Some(start) = query_source {
            query_starts.insert(start);
        }
        if let Some(values) = insert.values_keyword() {
            plan.break_before(values, 1, insert.span.base_depth);
        } else if let InsertSource::Query { start } = insert.source {
            plan.break_before(start, 1, insert.span.base_depth);
        }

        let rows_are_multiline = insert.rows.len() > 1
            || insert
                .rows
                .first()
                .is_some_and(|(open, _)| tokens[*open].line_breaks_before > 0);
        if rows_are_multiline {
            for &(open, close) in &insert.rows {
                plan.set_indent(open..close + 1, insert.span.base_depth + 1);
                plan.break_before(open, 1, insert.span.base_depth + 1);
            }
        }

        if let Some(conflict) = insert.on_conflict {
            plan.break_before(conflict.start, 1, insert.span.base_depth);
            if conflict.update {
                plan.break_before(conflict.action, 1, insert.span.base_depth);
                if let Some(set) = conflict.set {
                    plan.break_before(set, 1, insert.span.base_depth);
                    let set_end = conflict
                        .action_where
                        .or(insert.returning)
                        .unwrap_or(insert.span.end);
                    plan_keyword_list(context, set, set_end, insert.span.base_depth, true, plan);
                }
                if let Some(action_where) = conflict.action_where {
                    plan.break_before(action_where, 1, insert.span.base_depth);
                }
            }
        }

        if let Some(returning) = insert.returning {
            plan.break_before(returning, 1, insert.span.base_depth);
            plan_keyword_list(
                context,
                returning,
                insert.span.end,
                insert.span.base_depth,
                width_driven,
                plan,
            );
        }
    }
    query_starts
}

pub(super) fn plan_utility_statements(
    context: &PlanningContext<'_, '_>,
    utilities: &[crate::formatter::layout_ir::UtilityBlock],
    plan: &mut LayoutPlan,
) {
    use crate::formatter::ownership::UtilityStatementKind;

    for utility in utilities {
        let span = utility.span;
        let authored = context.tokens[span.start + 1..span.end]
            .iter()
            .any(|token| token.line_breaks_before > 0);
        let width = span.base_depth * INDENT_WIDTH
            + compact_width(context.tokens, span.start, span.end, context.options);
        let expanded = authored || width > context.options.soft_line_width;

        match utility.kind {
            UtilityStatementKind::Explain => {
                if let Some(statement) = (span.start + 1..span.end).find(|index| {
                    context.depths[*index] == span.base_depth
                        && matches!(
                            context.tokens[*index].kind,
                            Token::Select
                                | Token::Insert
                                | Token::Update
                                | Token::DeleteP
                                | Token::Merge
                                | Token::With
                        )
                }) {
                    plan.break_before(statement, 1, span.base_depth);
                }
            }
            UtilityStatementKind::CreateRule => {
                for index in span.start + 1..span.end {
                    if context.depths[index] == span.base_depth
                        && matches!(context.tokens[index].kind, Token::Where | Token::Do)
                    {
                        plan.break_before(index, 1, span.base_depth);
                    }
                }
            }
            UtilityStatementKind::AlterPolicy if expanded => {
                for index in span.start + 1..span.end {
                    if context.depths[index] == span.base_depth
                        && matches!(context.tokens[index].kind, Token::Using | Token::With)
                    {
                        plan.break_before(index, 1, span.base_depth);
                    }
                }
            }
            UtilityStatementKind::CreateStatistics if expanded => {
                for index in span.start + 1..span.end {
                    if context.depths[index] == span.base_depth
                        && matches!(context.tokens[index].kind, Token::On | Token::From)
                    {
                        plan.break_before(index, 1, span.base_depth);
                    }
                }
            }
            UtilityStatementKind::Copy if expanded => {
                if let Some(to_or_from) = (span.start + 1..span.end).find(|index| {
                    context.depths[*index] == span.base_depth
                        && matches!(context.tokens[*index].kind, Token::To | Token::From)
                }) {
                    plan.break_before(to_or_from, 1, span.base_depth);
                }
            }
            _ => {}
        }
    }
}
