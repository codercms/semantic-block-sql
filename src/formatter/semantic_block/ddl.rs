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
        let indent = values.span.base_depth + 1;
        for &(open, close) in &values.rows {
            plan.set_indent(open..close + 1, indent);
            plan.break_before(open, 1, indent);
        }
    }
}

pub(super) fn plan_views(
    _context: &PlanningContext<'_, '_>,
    statements: &[ViewBlock],
    plan: &mut LayoutPlan,
) {
    for view in statements {
        plan.break_before(view.query_start, 1, view.span.base_depth);
        if let Some(check) = view.check_option {
            plan.break_before(check, 1, view.span.base_depth);
        }
    }
}

pub(super) fn plan_materialized_views(
    _context: &PlanningContext<'_, '_>,
    statements: &[MaterializedViewBlock],
    plan: &mut LayoutPlan,
) {
    for view in statements {
        for clause in [view.using, view.tablespace].into_iter().flatten() {
            plan.break_before(clause, 1, view.span.base_depth);
        }
        if let Some((open, _)) = view.options {
            let with = open.saturating_sub(1);
            plan.break_before(with, 1, view.span.base_depth);
        }
        plan.break_before(view.query_start, 1, view.span.base_depth);
        if let Some(data_clause) = view.data_clause {
            plan.break_before(data_clause, 1, view.span.base_depth);
        }
    }
}

pub(super) fn plan_create_tables(
    _context: &PlanningContext<'_, '_>,
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
        }
        if let Some(close) = table.close {
            plan.break_before(close, 1, table.span.base_depth);
        }
        for clause in &table.clauses {
            plan.break_before(*clause, 1, table.span.base_depth);
        }
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
        if let Some((keyword, _open, close, items)) = &index.include {
            plan.break_before(*keyword, 1, index.span.base_depth);
            if items.len() > 1 {
                plan_ranges(items, *close, index.span.base_depth, plan);
            }
        }
        if let Some((keyword, _open, close, items)) = &index.with_options {
            plan.break_before(*keyword, 1, index.span.base_depth);
            if items.len() > 1 {
                plan_ranges(items, *close, index.span.base_depth, plan);
            }
        }
        if let Some(tablespace) = index.tablespace {
            plan.break_before(tablespace, 1, index.span.base_depth);
        }
        if let Some(where_clause) = index.where_clause {
            plan.break_before(where_clause, 1, index.span.base_depth);
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
                if group_changed { 2 } else { 1 },
                indent,
            );
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
        }
    }
}
