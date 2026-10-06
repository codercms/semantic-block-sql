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
        let compact_statement_width = update.span.base_depth * INDENT_WIDTH
            + compact_width(tokens, update.span.start, update.span.end, options);
        let has_expanded_predicate = update.where_clause.is_some_and(|where_clause| {
            boolean_ranges
                .iter()
                .any(|range| range.start == where_clause + 1 && range.end <= update.span.end)
        });
        // Retain compact one-line DML; authored layout permits the child owners
        // to plan their groups without forcing every sibling clause to expand.
        let mut expanded = LayoutGroup {
            compact_line_width: compact_statement_width,
            structurally_complex: update.from.is_some() || has_expanded_predicate,
            hard_boundary: false,
            force_expand: false,
            compact_overflow_is_unavoidable: false,
        }
        .decide(options)
            == GroupLayout::Expanded;
        if !expanded
            && !tokens[update.span.start + 1..update.span.end]
                .iter()
                .any(|token| token.line_breaks_before > 0)
        {
            continue;
        }
        let set_end = update
            .from
            .as_ref()
            .map(|source| source.introducer)
            .or(update.where_clause)
            .or(update.returning)
            .unwrap_or(update.span.end);
        let mut has_expanded_list = plan_keyword_list(
            context,
            update.set,
            set_end,
            update.span.base_depth,
            false,
            plan,
        );

        if let Some(returning) = update.returning {
            has_expanded_list |= plan_keyword_list(
                context,
                returning,
                update.span.end,
                update.span.base_depth,
                false,
                plan,
            );
        }
        expanded |= has_expanded_list;
        plan_clause_boundaries(
            context,
            [
                Some(update.set),
                update.from.as_ref().map(|source| source.introducer),
                update.where_clause,
                update.returning,
            ]
            .into_iter()
            .flatten(),
            update.span.base_depth,
            expanded,
            plan,
        );
        if let Some(from) = &update.from {
            plan_relation_source(from, context.depths, plan);
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
        let compact_statement_width = delete.span.base_depth * INDENT_WIDTH
            + compact_width(tokens, delete.span.start, delete.span.end, options);
        let width_driven = compact_statement_width > options.soft_line_width;
        let has_expanded_predicate = delete.where_clause.is_some_and(|where_clause| {
            boolean_ranges
                .iter()
                .any(|range| range.start == where_clause + 1 && range.end <= delete.span.end)
        });
        let mut expanded = LayoutGroup {
            compact_line_width: compact_statement_width,
            structurally_complex: delete.using.is_some() || has_expanded_predicate,
            hard_boundary: false,
            force_expand: false,
            compact_overflow_is_unavoidable: false,
        }
        .decide(options)
            == GroupLayout::Expanded;
        if !expanded
            && !tokens[delete.span.start + 1..delete.span.end]
                .iter()
                .any(|token| token.line_breaks_before > 0)
        {
            continue;
        }
        let mut has_expanded_list = false;
        if let Some(returning) = delete.returning {
            has_expanded_list = plan_keyword_list(
                context,
                returning,
                delete.span.end,
                delete.span.base_depth,
                width_driven,
                plan,
            );
        }
        expanded |= has_expanded_list;
        plan_clause_boundaries(
            context,
            [
                delete.using.as_ref().map(|source| source.introducer),
                delete.where_clause,
                delete.returning,
            ]
            .into_iter()
            .flatten(),
            delete.span.base_depth,
            expanded,
            plan,
        );
        if let Some(using) = &delete.using {
            plan_relation_source(using, context.depths, plan);
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
        plan_relation_source(&merge.source, context.depths, plan);
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

pub(super) fn plan_relation_source(
    source: &RelationSourceBlock,
    depths: &[usize],
    plan: &mut LayoutPlan,
) {
    let owner_indent = plan.indent_for(source.introducer, source.base_depth);
    let relative_indent = |depth: usize| owner_indent + depth.saturating_sub(source.base_depth);
    let item_indent = owner_indent + 1;
    if source.items.len() > 1 {
        for item in &source.items {
            plan.break_before(item.start, 1, item_indent);
            plan.set_indent(item.start..item.end, item_indent);
        }
    }

    let minimum_join_indent = if source.items.len() > 1 {
        item_indent
    } else {
        owner_indent
    };
    for &(open, close, inner_depth) in &source.wrappers {
        let indent = relative_indent(inner_depth) + usize::from(source.items.len() > 1);
        // Only direct tokens belong to this relation wrapper. Nested query and
        // expression owners establish their own contextual indentation later.
        for (index, &depth) in depths.iter().enumerate().take(close).skip(open + 1) {
            if depth == inner_depth {
                plan.token_indents[index] = Some(indent);
            }
        }
        plan.break_before(open + 1, 1, indent);
        plan.break_before(close, 1, indent.saturating_sub(1));
    }
    for join in &source.joins {
        let indent = plan.indent_for(join.start, relative_indent(join.depth));
        plan.break_before(join.start, 1, indent.max(minimum_join_indent));
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
        let mut has_expanded_list = lists.iter().any(|list| {
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
        let mut expanded = LayoutGroup {
            compact_line_width: compact_statement_width,
            structurally_complex: has_expanded_list || has_update || query_source.is_some(),
            hard_boundary: false,
            force_expand: insert
                .rows
                .iter()
                .any(|(open, _)| tokens[*open].line_breaks_before > 0),
            compact_overflow_is_unavoidable: false,
        }
        .decide(options)
            == GroupLayout::Expanded;
        if !expanded
            && !tokens[insert.span.start + 1..insert.span.end]
                .iter()
                .any(|token| token.line_breaks_before > 0)
        {
            continue;
        }
        if let Some(returning) = insert.returning {
            has_expanded_list |= plan_keyword_list(
                context,
                returning,
                insert.span.end,
                insert.span.base_depth,
                width_driven,
                plan,
            );
        }
        expanded |= has_expanded_list;

        let source_start = match insert.source {
            InsertSource::Values { keyword } => keyword,
            InsertSource::DefaultValues { default, .. } => default,
            InsertSource::Query { start } => start,
        };
        let authored = plan_clause_boundaries(
            context,
            [
                Some(source_start),
                insert.on_conflict.map(|conflict| conflict.start),
                insert.returning,
            ]
            .into_iter()
            .flatten(),
            insert.span.base_depth,
            expanded,
            plan,
        );
        if !expanded && !authored {
            continue;
        }

        if let Some(start) = query_source {
            query_starts.insert(start);
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
            plan_clause_boundaries(
                context,
                [Some(conflict.action), conflict.set, conflict.action_where]
                    .into_iter()
                    .flatten(),
                insert.span.base_depth,
                conflict.update,
                plan,
            );
            if conflict.update {
                if let Some(set) = conflict.set {
                    let set_end = conflict
                        .action_where
                        .or(insert.returning)
                        .unwrap_or(insert.span.end);
                    // An authored SET clause may already have a valid inline list.
                    let force_expand = tokens[set].line_breaks_before == 0;
                    plan_keyword_list(
                        context,
                        set,
                        set_end,
                        insert.span.base_depth,
                        force_expand,
                        plan,
                    );
                }
            }
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
            UtilityStatementKind::CreateSequence(_) | UtilityStatementKind::CreateTrigger(_)
                if expanded =>
            {
                for &index in &utility.clauses {
                    plan.break_before(
                        index,
                        context.tokens[index].line_breaks_before.max(1),
                        span.base_depth,
                    );
                }
            }
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
