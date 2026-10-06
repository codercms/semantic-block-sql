use crate::formatter::layout_ir::{
    AlterTableBlock, CreateIndexBlock, CreateTableBlock, CreateTableItem, MaterializedViewBlock,
    ValuesBlock, ViewBlock,
};
use crate::formatter::ownership::TokenRange;

use super::lists::plan_owned_delimited_list;
use super::*;

pub(super) fn plan_values_statements(
    context: &PlanningContext<'_, '_>,
    statements: &[ValuesBlock],
    plan: &mut LayoutPlan,
) {
    for values in statements {
        let authored = values
            .rows
            .iter()
            .any(|(open, _)| context.tokens[*open].line_breaks_before > 0);
        let width = values.span.base_depth * INDENT_WIDTH
            + compact_width(
                context.tokens,
                values.span.start,
                values.span.end,
                context.options,
            );
        if values.rows.len() <= 1 && !authored && width <= context.options.soft_line_width {
            continue;
        }
        let keyword_indent = values
            .wrapper
            .map_or(values.span.base_depth, |(open, close)| {
                let parent_indent =
                    plan.line_indent_for(open, values.span.base_depth.saturating_sub(1));
                plan.break_before(values.keyword, 1, parent_indent + 1);
                plan.break_before(close, 1, parent_indent);
                parent_indent + 1
            });
        let indent = keyword_indent + 1;
        for &(open, close) in &values.rows {
            plan.set_indent(open..close + 1, indent);
            plan.break_before(open, 1, indent);
        }
    }
}

pub(super) fn plan_views(
    context: &PlanningContext<'_, '_>,
    statements: &[ViewBlock],
    plan: &mut LayoutPlan,
) {
    for view in statements {
        plan_clause_boundaries(
            context,
            [Some(view.query_start), view.check_option]
                .into_iter()
                .flatten(),
            view.span.base_depth,
            true,
            plan,
        );
    }
}

pub(super) fn plan_materialized_views(
    context: &PlanningContext<'_, '_>,
    statements: &[MaterializedViewBlock],
    plan: &mut LayoutPlan,
) {
    for view in statements {
        plan_clause_boundaries(
            context,
            [
                view.using,
                view.tablespace,
                view.options.map(|(open, _)| open.saturating_sub(1)),
                Some(view.query_start),
                view.data_clause,
            ]
            .into_iter()
            .flatten(),
            view.span.base_depth,
            true,
            plan,
        );
    }
}

pub(super) fn plan_create_tables(
    context: &PlanningContext<'_, '_>,
    statements: &[CreateTableBlock],
    plan: &mut LayoutPlan,
) {
    for table in statements {
        let indent = table.span.base_depth + 1;
        for (position, item) in table.items.iter().enumerate() {
            plan_item_indent(item, indent, plan);
            let blank_line = position > 0
                && table.items[position - 1].kind.is_column()
                && !item.kind.is_column();
            plan.break_before(item.range.start, if blank_line { 2 } else { 1 }, indent);
            if let Some(identity) = &item.identity {
                plan_identity(context, identity, indent, plan);
            }
        }
        if let Some(close) = table.close {
            plan.break_before(close, 1, table.span.base_depth);
        }
        plan_clause_boundaries(
            context,
            table.clauses.iter().copied(),
            table.span.base_depth,
            true,
            plan,
        );
    }
}

fn plan_item_indent(item: &CreateTableItem, indent: usize, plan: &mut LayoutPlan) {
    plan.set_indent(item.range.start..item.range.end, indent);
}

pub(super) fn plan_create_indexes(
    context: &PlanningContext<'_, '_>,
    statements: &[CreateIndexBlock],
    plan: &mut LayoutPlan,
) {
    for index in statements {
        let authored = context.tokens[index.span.start + 1..index.span.end]
            .iter()
            .any(|token| token.line_breaks_before > 0);
        let width = index.span.base_depth * INDENT_WIDTH
            + compact_width(
                context.tokens,
                index.span.start,
                index.span.end,
                context.options,
            );
        let has_secondary_clauses =
            index.include.is_some() || index.with_options.is_some() || index.tablespace.is_some();
        let expanded = authored || has_secondary_clauses || width > context.options.soft_line_width;

        let key_width = compact_width(
            context.tokens,
            index.key_open,
            index.key_close + 1,
            context.options,
        );
        if index.key_items.len() > 1
            || key_width + index.span.base_depth * INDENT_WIDTH > context.options.soft_line_width
            || context.tokens[index.key_open + 1].line_breaks_before > 0
        {
            plan_ranges(
                &index.key_items,
                index.key_close,
                index.span.base_depth,
                plan,
            );
        }

        if !expanded {
            continue;
        }
        plan_clause_boundaries(
            context,
            [
                index.include.as_ref().map(|(keyword, ..)| *keyword),
                index.with_options.as_ref().map(|(keyword, ..)| *keyword),
                index.tablespace,
                index.where_clause,
            ]
            .into_iter()
            .flatten(),
            index.span.base_depth,
            true,
            plan,
        );
        if let Some((_keyword, _open, close, items)) = &index.include {
            if items.len() > 1 {
                plan_ranges(items, *close, index.span.base_depth, plan);
            }
        }
        if let Some((_keyword, _open, close, items)) = &index.with_options {
            if items.len() > 1 {
                plan_ranges(items, *close, index.span.base_depth, plan);
            }
        }
    }
}

fn plan_ranges(ranges: &[TokenRange], close: usize, base_depth: usize, plan: &mut LayoutPlan) {
    let indent = base_depth + 1;
    for range in ranges {
        plan.set_indent(range.start..range.end, indent);
        plan.break_before(range.start, 1, indent);
    }
    plan.break_before(close, 1, base_depth);
}

pub(super) fn plan_alter_tables(
    context: &PlanningContext<'_, '_>,
    statements: &[AlterTableBlock],
    plan: &mut LayoutPlan,
) {
    for table in statements {
        let authored = context.tokens[table.span.start + 1..table.span.end]
            .iter()
            .any(|token| token.line_breaks_before > 0);
        let width = table.span.base_depth * INDENT_WIDTH
            + compact_width(
                context.tokens,
                table.span.start,
                table.span.end,
                context.options,
            );
        let expands_check = table
            .actions
            .iter()
            .flat_map(|action| &action.checks)
            .any(|check| {
                (check.open + 1..check.close).any(|index| {
                    context.depths[index] > context.depths[check.open]
                        && matches!(context.tokens[index].kind, Token::And | Token::Or)
                }) || check.indent * INDENT_WIDTH
                    + compact_width(
                        context.tokens,
                        check.introducer,
                        check.close + 1,
                        context.options,
                    )
                    > context.options.soft_line_width
            });
        if table.actions.len() == 1
            && !authored
            && !expands_check
            && width <= context.options.soft_line_width
        {
            continue;
        }
        let indent = table.span.base_depth + 1;
        for (position, action) in table.actions.iter().enumerate() {
            plan.set_indent(action.range.start..action.range.end, indent);
            let group_changed = position > 0 && table.actions[position - 1].group != action.group;
            plan.break_before(
                action.range.start,
                if group_changed {
                    2
                } else {
                    context.tokens[action.range.start]
                        .line_breaks_before
                        .clamp(1, 2)
                },
                indent,
            );
            if let Some(identity) = &action.identity {
                plan_identity(context, identity, indent + 1, plan);
            }
            if let Some(options) = &action.relation_options {
                plan_owned_delimited_list(
                    context,
                    action.range,
                    options.close,
                    &options.items,
                    indent,
                    plan,
                );
            }
            if !action.foreign_key_clauses.is_empty() {
                let clause_indent = indent + 1;
                for &index in &action.foreign_key_clauses {
                    plan.break_before(
                        index,
                        context.tokens[index].line_breaks_before.max(1),
                        clause_indent,
                    );
                    plan.set_indent(index..action.range.end, clause_indent);
                }
            }
        }
    }
}

fn plan_identity(
    context: &PlanningContext<'_, '_>,
    block: &crate::formatter::layout_ir::IdentityBlock,
    indent: usize,
    plan: &mut LayoutPlan,
) {
    let Some((open, close)) = block.options else {
        return;
    };
    let authored = context.tokens[open + 1..close]
        .iter()
        .any(|token| token.line_breaks_before > 0);
    let width = compact_width(context.tokens, block.introducer, close + 1, context.options)
        + indent * INDENT_WIDTH;
    if !authored && width <= context.options.soft_line_width {
        return;
    }
    plan.break_before(
        block.introducer,
        context.tokens[block.introducer].line_breaks_before.max(1),
        indent,
    );
    plan.set_indent(open + 1..close, indent + 1);
    for &index in &block.clauses {
        plan.break_before(
            index,
            context.tokens[index].line_breaks_before.max(1),
            indent + 1,
        );
    }
    plan.break_before(close, 1, indent);
}
