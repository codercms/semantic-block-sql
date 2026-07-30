use super::statements::{is_insert_list_open, is_merge_list_open};
use super::*;

pub(super) fn parenthesized_lists(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    parens: &HashMap<usize, usize>,
    cases: &[CaseRange],
    inserts: &[InsertBlock],
    merges: &[MergeBlock],
    options: &FormatOptions,
) -> Vec<ParenthesizedList> {
    let mut lists = Vec::new();

    for (open, token) in tokens.iter().enumerate() {
        if token.kind != Token::Ascii40
            || !(is_function_call_open(tokens, open)
                || is_insert_list_open(inserts, open)
                || is_merge_list_open(merges, open))
        {
            continue;
        }
        let Some(&close) = parens.get(&open) else {
            continue;
        };
        if open + 1 >= close {
            continue;
        }
        let inner_depth = depths[open] + 1;
        let has_arguments = (open + 1..close).any(|index| depths[index] == inner_depth);
        if !has_arguments {
            continue;
        }
        let authored = tokens[open + 1..close]
            .iter()
            .any(|token| token.line_breaks_before > 0);
        let contains_complex = cases
            .iter()
            .any(|case| case.expanded && case.start > open && case.end < close)
            || (open + 1..close)
                .any(|index| tokens[index].kind == Token::Select && depths[index] > depths[open]);
        let compact_start = inserts
            .iter()
            .find(|insert| insert.target_open == Some(open))
            .map(|insert| insert.span.start)
            .or_else(|| {
                merges.iter().find_map(|merge| {
                    merge
                        .branches
                        .iter()
                        .find_map(|branch| match branch.action {
                            MergeAction::Insert {
                                target_open,
                                values_open,
                                ..
                            } if target_open == Some(open) || values_open == open => {
                                Some(branch.start)
                            }
                            _ => None,
                        })
                })
            })
            .unwrap_or_else(|| open.saturating_sub(1));
        let compact = compact_width(tokens, compact_start, close + 1, options);
        let has_top_level_comma = (open + 1..close)
            .any(|index| tokens[index].kind == Token::Ascii44 && depths[index] == inner_depth);
        let unavoidable_single_argument = !has_top_level_comma
            && range_is_unavoidably_over_hard(tokens, open + 1, close, depths[open] + 1, options);
        lists.push(ParenthesizedList {
            open,
            close,
            expanded: authored
                || contains_complex
                || (!unavoidable_single_argument
                    && depths[open] * INDENT_WIDTH + compact > options.soft_line_width),
        });
    }

    lists
}

fn is_function_call_open(tokens: &[SqlToken<'_>], open: usize) -> bool {
    open.checked_sub(1).is_some_and(|previous| {
        tokens[previous].kind == Token::Ident
            || matches!(
                tokens[previous].kind,
                Token::Coalesce | Token::Nullif | Token::Greatest | Token::Least
            )
    })
}

pub(super) fn plan_keyword_list(
    context: &PlanningContext<'_, '_>,
    keyword: usize,
    end: usize,
    base_depth: usize,
    force_expand: bool,
    plan: &mut LayoutPlan,
) {
    let tokens = context.tokens;
    let depths = context.depths;
    let cases = context.cases;
    let lists = context.lists;
    let options = context.options;
    let list_end = (keyword + 1..end)
        .find(|&index| depths[index] == base_depth && tokens[index].kind == Token::Ascii59)
        .unwrap_or(end);
    if keyword + 1 >= list_end {
        return;
    }

    let mut items = split_list_items(
        tokens,
        depths,
        cases,
        lists,
        keyword + 1,
        list_end,
        base_depth,
    );
    if items.is_empty() {
        return;
    }

    let indent = base_depth + 1;
    let authored = tokens[items[0].start].line_breaks_before > 0
        || items
            .iter()
            .skip(1)
            .any(|item| tokens[item.start].line_breaks_before > 0);
    let has_complex = items.iter().any(|item| item.complex);
    let compact_line_width =
        base_depth * INDENT_WIDTH + compact_width(tokens, keyword, list_end, options);
    let expanded = authored
        || has_complex
        || (force_expand && (tokens[keyword].kind == Token::Set || items.len() > 1))
        || compact_line_width > options.soft_line_width;
    if !expanded {
        return;
    }

    for item in &items {
        plan.set_indent(item.start..item.end, indent);
        if let Some(comma) = item.comma {
            plan.token_indents[comma] = Some(indent);
        }
    }
    plan.break_before(items[0].start, 1, indent);
    let lines = if authored {
        authored_list_lines(tokens, &items, indent, options)
    } else {
        expanded_one_line_items(&items)
    };
    for (line_number, (item_index, blank_before)) in lines.into_iter().enumerate() {
        if line_number == 0 {
            continue;
        }
        plan.break_before(
            items[item_index].start,
            if blank_before { 2 } else { 1 },
            indent,
        );
    }

    for item in &mut items {
        if tokens[item.start..item.end]
            .iter()
            .any(|token| token.kind == Token::SqlComment)
        {
            plan.set_indent(item.start..item.end, indent);
        }
    }
}

pub(super) fn plan_select_lists(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    cases: &[CaseRange],
    parenthesized_lists: &[ParenthesizedList],
    queries: &[QueryBlock],
    options: &FormatOptions,
    plan: &mut LayoutPlan,
) -> HashSet<usize> {
    let mut expanded_selects = HashSet::new();

    for query in queries {
        let select = query.select;
        let base_depth = query.base_depth;
        let base_indent = query.indent;
        let end = query
            .clauses
            .ordered_boundaries(query.end)
            .into_iter()
            .next()
            .unwrap_or(query.end);
        if query.list_start >= end {
            continue;
        }

        let mut items = split_list_items(
            tokens,
            depths,
            cases,
            parenthesized_lists,
            query.list_start,
            end,
            base_depth,
        );
        if items.is_empty() {
            continue;
        }
        let indent = base_indent + 1;
        for item in &items {
            plan.set_indent(item.start..item.end, indent);
            if let Some(comma) = item.comma {
                plan.token_indents[comma] = Some(indent);
            }
        }

        let authored = tokens[items[0].start].line_breaks_before > 0
            || items
                .iter()
                .skip(1)
                .any(|item| tokens[item.start].line_breaks_before > 0);
        let has_complex = items.iter().any(|item| item.complex);
        let compact_line_width =
            base_indent * INDENT_WIDTH + compact_width(tokens, select, end, options);
        let unavoidable_single_item = items.len() == 1
            && range_is_unavoidably_over_hard(
                tokens,
                items[0].start,
                items[0].end,
                indent,
                options,
            );
        let expanded = authored
            || has_complex
            || (!unavoidable_single_item && compact_line_width > options.soft_line_width);
        if !expanded {
            continue;
        }

        expanded_selects.insert(select);
        plan.break_before(items[0].start, 1, indent);
        let lines = if authored {
            authored_list_lines(tokens, &items, indent, options)
        } else {
            expanded_one_line_items(&items)
        };

        for (line_number, (item_index, blank_before)) in lines.into_iter().enumerate() {
            if line_number == 0 {
                continue;
            }
            let lines = if blank_before { 2 } else { 1 };
            plan.break_before(items[item_index].start, lines, indent);
        }

        // Complex expressions are stable singleton lines.
        for index in 0..items.len() {
            if !items[index].complex {
                continue;
            }
            if index > 0 {
                plan.break_before(items[index].start, 1, indent);
            }
            if index + 1 < items.len() {
                plan.break_before(items[index + 1].start, 1, indent);
            }
        }

        // A line comment after a comma owns the remainder of its physical line;
        // keep the next list token at the list indentation.
        for item in &mut items {
            if tokens[item.start..item.end]
                .iter()
                .any(|token| token.kind == Token::SqlComment)
            {
                plan.set_indent(item.start..item.end, indent);
            }
        }
    }

    expanded_selects
}

fn split_list_items(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    cases: &[CaseRange],
    parenthesized_lists: &[ParenthesizedList],
    start: usize,
    end: usize,
    base_depth: usize,
) -> Vec<ListItem> {
    let mut result = Vec::new();
    let mut item_start = start;

    for index in start..end {
        if tokens[index].kind != Token::Ascii44 || depths[index] != base_depth {
            continue;
        }
        let item_end = tokens
            .get(index + 1)
            .filter(|next| next.is_comment() && next.line_breaks_before == 0)
            .map_or(index, |_| index + 2);
        result.push(ListItem {
            start: item_start,
            end: item_end,
            comma: Some(index),
            complex: item_is_complex(
                tokens,
                cases,
                parenthesized_lists,
                item_start,
                item_end,
                base_depth,
                depths,
            ),
        });
        item_start = item_end.max(index + 1);
    }
    if item_start < end {
        result.push(ListItem {
            start: item_start,
            end,
            comma: None,
            complex: item_is_complex(
                tokens,
                cases,
                parenthesized_lists,
                item_start,
                end,
                base_depth,
                depths,
            ),
        });
    }
    result
}

fn item_is_complex(
    tokens: &[SqlToken<'_>],
    cases: &[CaseRange],
    parenthesized_lists: &[ParenthesizedList],
    start: usize,
    end: usize,
    base_depth: usize,
    depths: &[usize],
) -> bool {
    cases
        .iter()
        .any(|range| range.expanded && range.start >= start && range.start < end)
        || parenthesized_lists
            .iter()
            .any(|list| list.expanded && list.open >= start && list.close < end)
        || tokens[start..end]
            .iter()
            .enumerate()
            .any(|(offset, token)| {
                token.kind == Token::Select && depths[start + offset] > base_depth
            })
}

pub(super) fn plan_parenthesized_lists(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    cases: &[CaseRange],
    lists: &[ParenthesizedList],
    options: &FormatOptions,
    plan: &mut LayoutPlan,
) {
    for list in lists.iter().filter(|list| list.expanded) {
        let inner_depth = depths[list.open] + 1;
        let mut items = split_list_items(
            tokens,
            depths,
            cases,
            lists,
            list.open + 1,
            list.close,
            inner_depth,
        );
        if items.is_empty() {
            continue;
        }
        let base_indent = plan.indent_for(list.open, depths[list.open]);
        let indent = base_indent + 1;
        for item in &items {
            plan.set_indent(item.start..item.end, indent);
            if let Some(comma) = item.comma {
                plan.token_indents[comma] = Some(indent);
            }
        }
        let authored = tokens[items[0].start].line_breaks_before > 0
            || items
                .iter()
                .skip(1)
                .any(|item| tokens[item.start].line_breaks_before > 0);
        let lines = if authored {
            authored_list_lines(tokens, &items, indent, options)
        } else {
            expanded_one_line_items(&items)
        };

        plan.break_before(items[0].start, 1, indent);
        for (line_number, (item_index, blank_before)) in lines.into_iter().enumerate() {
            if line_number == 0 {
                continue;
            }
            plan.break_before(
                items[item_index].start,
                if blank_before { 2 } else { 1 },
                indent,
            );
        }
        plan.break_before(list.close, 1, base_indent);

        for item in &mut items {
            if tokens[item.start..item.end]
                .iter()
                .any(|token| token.kind == Token::SqlComment)
            {
                plan.set_indent(item.start..item.end, indent);
            }
        }
    }
}

fn authored_list_lines(
    tokens: &[SqlToken<'_>],
    items: &[ListItem],
    indent: usize,
    options: &FormatOptions,
) -> Vec<(usize, bool)> {
    let mut source_groups = vec![(0usize, false)];
    for (index, item) in items.iter().enumerate().skip(1) {
        if tokens[item.start].line_breaks_before > 0 {
            source_groups.push((index, tokens[item.start].line_breaks_before > 1));
        }
    }

    split_groups_at_width(
        tokens,
        items,
        &source_groups,
        indent,
        options.hard_line_width,
        options,
    )
}

fn expanded_one_line_items(items: &[ListItem]) -> Vec<(usize, bool)> {
    items
        .iter()
        .enumerate()
        .map(|(index, _)| (index, false))
        .collect()
}

fn split_groups_at_width(
    tokens: &[SqlToken<'_>],
    items: &[ListItem],
    groups: &[(usize, bool)],
    indent: usize,
    limit: usize,
    options: &FormatOptions,
) -> Vec<(usize, bool)> {
    let mut lines = Vec::new();
    for (group_position, &(group_start, blank_before)) in groups.iter().enumerate() {
        let group_end = groups
            .get(group_position + 1)
            .map_or(items.len(), |group| group.0);
        let mut line_start = group_start;
        let mut line_width = indent * INDENT_WIDTH;
        lines.push((line_start, blank_before));

        for index in group_start..group_end {
            let item_width = compact_width(tokens, items[index].start, items[index].end, options)
                + usize::from(
                    items[index]
                        .comma
                        .is_some_and(|comma| comma >= items[index].end),
                );
            let separator = usize::from(index > line_start);
            if index > line_start
                && (items[index].complex
                    || items[index - 1].complex
                    || line_width + separator + item_width > limit)
            {
                line_start = index;
                line_width = indent * INDENT_WIDTH;
                lines.push((index, false));
            }
            line_width += usize::from(index > line_start) + item_width;
        }
    }
    lines
}
