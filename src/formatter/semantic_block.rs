use std::collections::{HashMap, HashSet};

use pg_query::protobuf::{KeywordKind, Token};

use super::tokens::{SqlToken, tokenize};
use super::{FormatDiagnostic, FormatOptions, FormatWarning};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Break {
    lines: usize,
    indent: usize,
}

#[derive(Debug)]
struct LayoutPlan {
    before: HashMap<usize, Break>,
    token_indents: Vec<Option<usize>>,
}

impl LayoutPlan {
    fn new(token_count: usize) -> Self {
        Self {
            before: HashMap::new(),
            token_indents: vec![None; token_count],
        }
    }

    fn break_before(&mut self, index: usize, lines: usize, indent: usize) {
        if index >= self.token_indents.len() {
            return;
        }
        let candidate = Break {
            lines: lines.max(1),
            indent,
        };
        self.before
            .entry(index)
            .and_modify(|current| {
                if candidate.lines >= current.lines {
                    *current = candidate;
                }
            })
            .or_insert(candidate);
    }

    fn set_indent(&mut self, range: std::ops::Range<usize>, indent: usize) {
        for slot in &mut self.token_indents[range] {
            *slot = Some(indent);
        }
    }

    fn indent_for(&self, index: usize, fallback: usize) -> usize {
        self.token_indents[index].unwrap_or(fallback)
    }
}

#[derive(Debug, Clone, Copy)]
struct BooleanRange {
    start: usize,
    end: usize,
    base_depth: usize,
}

#[derive(Debug, Clone, Copy)]
struct CaseRange {
    start: usize,
    end: usize,
    expanded: bool,
}

#[derive(Debug, Clone, Copy)]
struct ListItem {
    start: usize,
    end: usize,
    comma: Option<usize>,
    complex: bool,
}

#[derive(Debug)]
struct CteBlock {
    with_index: usize,
    ctes: Vec<(usize, usize)>,
    main_select: usize,
}

#[derive(Debug, Clone, Copy)]
struct ParenthesizedList {
    open: usize,
    close: usize,
    expanded: bool,
}

pub(super) fn format(source: &str, options: &FormatOptions) -> Result<String, FormatDiagnostic> {
    let tokens = tokenize(source)?;
    if tokens.is_empty() {
        return Ok(String::new());
    }

    let depths = token_depths(&tokens);
    let parens = parenthesis_pairs(&tokens);
    let cases = case_ranges(&tokens, options);
    let parenthesized_lists = parenthesized_lists(&tokens, &depths, &parens, &cases, options);
    let ctes = cte_blocks(&tokens, &depths, &parens);
    let boolean_ranges = boolean_ranges(&tokens, &depths, options);
    let mut plan = LayoutPlan::new(tokens.len());

    let expanded_selects = plan_select_lists(
        &tokens,
        &depths,
        &cases,
        &parenthesized_lists,
        options,
        &mut plan,
    );
    plan_parenthesized_lists(
        &tokens,
        &depths,
        &cases,
        &parenthesized_lists,
        options,
        &mut plan,
    );
    plan_query_clauses(
        &tokens,
        &depths,
        &boolean_ranges,
        &ctes,
        &expanded_selects,
        &mut plan,
    );
    plan_booleans(&tokens, &depths, &boolean_ranges, &parens, &mut plan);
    plan_cases(&tokens, &depths, &cases, &mut plan);
    plan_ctes(&tokens, &depths, &ctes, &mut plan);

    let mut writer = Writer::new(options.indent_width);
    let mut previous_index = None;

    for (index, token) in tokens.iter().enumerate() {
        let planned_break = plan.before.get(&index).copied();
        if let Some(line_break) = planned_break {
            writer.newline(line_break.lines, line_break.indent);
        } else if token.is_comment() && token.line_breaks_before > 0 {
            let lines = if options.preserve_blank_lines {
                token.line_breaks_before.min(2)
            } else {
                1
            };
            writer.newline(lines, plan.indent_for(index, depths[index]));
        }

        if needs_space(&tokens, previous_index, index) {
            writer.space();
        }
        writer.write(&render_token(&tokens, index));

        if token.is_comment() {
            let next_starts_authored_line = tokens
                .get(index + 1)
                .is_some_and(|next| next.line_breaks_before > 0);
            if token.kind == Token::SqlComment || next_starts_authored_line {
                let indent = plan.indent_for(index, depths[index]);
                writer.newline(1, indent);
            }
        }

        previous_index = Some(index);
    }

    Ok(writer.finish())
}

pub(super) fn validate_hard_width(
    output: &str,
    options: &FormatOptions,
) -> Result<Vec<FormatWarning>, FormatDiagnostic> {
    let tokens = tokenize(output)?;
    let mut warnings = Vec::new();
    let mut line_start = 0usize;

    for (line_index, line_with_newline) in output.split_inclusive('\n').enumerate() {
        let line = line_with_newline
            .strip_suffix('\n')
            .unwrap_or(line_with_newline);
        let width = line.chars().count();
        let line_end = line_start + line.len();
        if width > options.hard_line_width {
            let indivisible = tokens.iter().any(|token| {
                token.start >= line_start
                    && token.end <= line_end
                    && is_indivisible(token)
                    && output[line_start..token.start].chars().count() + token.text.chars().count()
                        > options.hard_line_width
            });
            if indivisible {
                warnings.push(FormatWarning::IndivisibleTokenExceedsHardWidth {
                    line: line_index + 1,
                    width,
                });
            } else {
                return Err(FormatDiagnostic::HardLineExceeded {
                    line: line_index + 1,
                    width,
                    hard_limit: options.hard_line_width,
                });
            }
        }
        line_start += line_with_newline.len();
    }

    Ok(warnings)
}

fn is_indivisible(token: &SqlToken<'_>) -> bool {
    matches!(
        token.kind,
        Token::Ident
            | Token::Uident
            | Token::Sconst
            | Token::Usconst
            | Token::Bconst
            | Token::Xconst
            | Token::SqlComment
            | Token::CComment
    ) || token.text.starts_with('"')
}

fn token_depths(tokens: &[SqlToken<'_>]) -> Vec<usize> {
    let mut depth = 0usize;
    tokens
        .iter()
        .map(|token| {
            if token.kind == Token::Ascii41 {
                depth = depth.saturating_sub(1);
            }
            let current = depth;
            if token.kind == Token::Ascii40 {
                depth += 1;
            }
            current
        })
        .collect()
}

fn parenthesis_pairs(tokens: &[SqlToken<'_>]) -> HashMap<usize, usize> {
    let mut stack = Vec::new();
    let mut pairs = HashMap::new();
    for (index, token) in tokens.iter().enumerate() {
        match token.kind {
            Token::Ascii40 => stack.push(index),
            Token::Ascii41 => {
                if let Some(open) = stack.pop() {
                    pairs.insert(open, index);
                    pairs.insert(index, open);
                }
            }
            _ => {}
        }
    }
    pairs
}

fn case_ranges(tokens: &[SqlToken<'_>], options: &FormatOptions) -> Vec<CaseRange> {
    let mut stack = Vec::new();
    let mut ranges = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        match token.kind {
            Token::Case => stack.push(index),
            Token::EndP => {
                let Some(start) = stack.pop() else {
                    continue;
                };
                let top_level_when_count = tokens[start + 1..index]
                    .iter()
                    .filter(|token| token.kind == Token::When)
                    .count();
                let authored_multiline = tokens[start + 1..=index]
                    .iter()
                    .any(|token| token.line_breaks_before > 0);
                let compact_width = compact_width(tokens, start, index + 1);
                ranges.push(CaseRange {
                    start,
                    end: index,
                    expanded: top_level_when_count > 1
                        || authored_multiline
                        || compact_width > options.soft_line_width,
                });
            }
            _ => {}
        }
    }
    ranges.sort_unstable_by_key(|range| range.start);
    ranges
}

fn parenthesized_lists(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    parens: &HashMap<usize, usize>,
    cases: &[CaseRange],
    options: &FormatOptions,
) -> Vec<ParenthesizedList> {
    let mut lists = Vec::new();

    for (open, token) in tokens.iter().enumerate() {
        if token.kind != Token::Ascii40 || !is_function_call_open(tokens, open) {
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
        let compact = compact_width(tokens, open.saturating_sub(1), close + 1);
        lists.push(ParenthesizedList {
            open,
            close,
            expanded: authored
                || contains_complex
                || depths[open] * options.indent_width + compact > options.soft_line_width,
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

fn plan_select_lists(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    cases: &[CaseRange],
    parenthesized_lists: &[ParenthesizedList],
    options: &FormatOptions,
    plan: &mut LayoutPlan,
) -> HashSet<usize> {
    let mut expanded_selects = HashSet::new();

    for (select, token) in tokens.iter().enumerate() {
        if token.kind != Token::Select {
            continue;
        }
        let base_depth = depths[select];
        let end = (select + 1..tokens.len())
            .find(|&index| {
                depths[index] < base_depth
                    || (depths[index] == base_depth
                        && (is_select_list_terminator(tokens, index)
                            || tokens[index].kind == Token::Ascii59))
            })
            .unwrap_or(tokens.len());
        if select + 1 >= end {
            continue;
        }

        let mut items = split_list_items(
            tokens,
            depths,
            cases,
            parenthesized_lists,
            select + 1,
            end,
            base_depth,
        );
        if items.is_empty() {
            continue;
        }
        let indent = base_depth + 1;
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
            base_depth * options.indent_width + compact_width(tokens, select, end);
        let expanded = authored || has_complex || compact_line_width > options.soft_line_width;
        if !expanded {
            continue;
        }

        expanded_selects.insert(select);
        plan.break_before(items[0].start, 1, indent);
        let lines = if authored && options.preserve_list_groups {
            authored_list_lines(tokens, &items, indent, options)
        } else {
            packed_list_lines(tokens, &items, indent, options.soft_line_width, options)
        };

        for (line_number, (item_index, blank_before)) in lines.into_iter().enumerate() {
            if line_number == 0 {
                continue;
            }
            let lines = if blank_before && options.preserve_blank_lines {
                2
            } else {
                1
            };
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
        result.push(ListItem {
            start: item_start,
            end: index,
            comma: Some(index),
            complex: item_is_complex(
                tokens,
                cases,
                parenthesized_lists,
                item_start,
                index,
                base_depth,
                depths,
            ),
        });
        item_start = index + 1;
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

fn plan_parenthesized_lists(
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
        let lines = if authored && options.preserve_list_groups {
            authored_list_lines(tokens, &items, indent, options)
        } else {
            packed_list_lines(tokens, &items, indent, options.soft_line_width, options)
        };

        plan.break_before(items[0].start, 1, indent);
        for (line_number, (item_index, blank_before)) in lines.into_iter().enumerate() {
            if line_number == 0 {
                continue;
            }
            plan.break_before(
                items[item_index].start,
                if blank_before && options.preserve_blank_lines {
                    2
                } else {
                    1
                },
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

fn packed_list_lines(
    tokens: &[SqlToken<'_>],
    items: &[ListItem],
    indent: usize,
    width: usize,
    options: &FormatOptions,
) -> Vec<(usize, bool)> {
    let starts = vec![(0usize, false)];
    split_groups_at_width(tokens, items, &starts, indent, width, options)
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
        let mut line_width = indent * options.indent_width;
        lines.push((line_start, blank_before));

        for index in group_start..group_end {
            let item_width = compact_width(tokens, items[index].start, items[index].end)
                + usize::from(items[index].comma.is_some());
            let separator = usize::from(index > line_start);
            if index > line_start
                && (items[index].complex
                    || items[index - 1].complex
                    || line_width + separator + item_width > limit)
            {
                line_start = index;
                line_width = indent * options.indent_width;
                lines.push((index, false));
            }
            line_width += usize::from(index > line_start) + item_width;
        }
    }
    lines
}

fn plan_query_clauses(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    boolean_ranges: &[BooleanRange],
    ctes: &[CteBlock],
    expanded_selects: &HashSet<usize>,
    plan: &mut LayoutPlan,
) {
    let cte_main_selects: HashSet<_> = ctes.iter().map(|cte| cte.main_select).collect();
    let cte_body_selects: HashSet<_> = ctes
        .iter()
        .flat_map(|cte| {
            cte.ctes.iter().filter_map(|&(open, close)| {
                (open + 1..close).find(|&index| tokens[index].kind == Token::Select)
            })
        })
        .collect();

    for (select, token) in tokens.iter().enumerate() {
        if token.kind != Token::Select {
            continue;
        }
        let base_depth = depths[select];
        let end = query_end(tokens, depths, select, base_depth);
        let has_join = (select + 1..end)
            .any(|index| depths[index] == base_depth && is_join_start(tokens, index));
        let has_expanded_boolean = boolean_ranges
            .iter()
            .any(|range| range.start > select && range.start < end);
        let expanded = expanded_selects.contains(&select)
            || has_join
            || has_expanded_boolean
            || cte_main_selects.contains(&select)
            || cte_body_selects.contains(&select);
        if !expanded {
            continue;
        }

        for (index, &depth) in depths.iter().enumerate().take(end).skip(select + 1) {
            if depth != base_depth {
                continue;
            }
            if is_major_clause_start(tokens, index) || is_join_start(tokens, index) {
                plan.break_before(index, 1, base_depth);
            }
        }
    }
}

fn query_end(tokens: &[SqlToken<'_>], depths: &[usize], select: usize, base_depth: usize) -> usize {
    (select + 1..tokens.len())
        .find(|&index| {
            depths[index] < base_depth
                || (depths[index] == base_depth && tokens[index].kind == Token::Ascii59)
        })
        .unwrap_or(tokens.len())
}

fn boolean_ranges(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    options: &FormatOptions,
) -> Vec<BooleanRange> {
    let mut result = Vec::new();

    for (index, token) in tokens.iter().enumerate() {
        if !matches!(token.kind, Token::Where | Token::On) {
            continue;
        }
        let base_depth = depths[index];
        let end = (index + 1..tokens.len())
            .find(|&candidate| {
                depths[candidate] < base_depth
                    || (depths[candidate] == base_depth
                        && (is_major_clause_start(tokens, candidate)
                            || is_join_start(tokens, candidate)
                            || matches!(
                                tokens[candidate].kind,
                                Token::Ascii59 | Token::Union | Token::Intersect | Token::Except
                            )))
            })
            .unwrap_or(tokens.len());
        let has_connector = (index + 1..end).any(|candidate| {
            depths[candidate] >= base_depth
                && matches!(tokens[candidate].kind, Token::And | Token::Or)
        });
        let hides_structure = base_depth * options.indent_width + compact_width(tokens, index, end)
            > options.soft_line_width;

        if has_connector || hides_structure {
            result.push(BooleanRange {
                start: index + 1,
                end,
                base_depth,
            });
        }
    }

    result
}

fn plan_booleans(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    ranges: &[BooleanRange],
    parens: &HashMap<usize, usize>,
    plan: &mut LayoutPlan,
) {
    for range in ranges {
        plan.break_before(range.start, 1, range.base_depth + 1);
        for index in range.start..range.end {
            if matches!(tokens[index].kind, Token::And | Token::Or) {
                plan.break_before(index, 1, 1 + depths[index].saturating_sub(range.base_depth));
            }
            if tokens[index].kind == Token::Ascii40 {
                let Some(&close) = parens.get(&index) else {
                    continue;
                };
                if close >= range.end {
                    continue;
                }
                let inner_depth = depths[index] + 1;
                let contains_boolean = (index + 1..close).any(|candidate| {
                    depths[candidate] == inner_depth
                        && matches!(tokens[candidate].kind, Token::And | Token::Or)
                });
                if contains_boolean {
                    if index + 1 < close {
                        plan.break_before(
                            index + 1,
                            1,
                            1 + inner_depth.saturating_sub(range.base_depth),
                        );
                    }
                    plan.break_before(
                        close,
                        1,
                        depths[close].saturating_sub(range.base_depth).max(1),
                    );
                }
            }
        }
    }
}

fn plan_cases(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    cases: &[CaseRange],
    plan: &mut LayoutPlan,
) {
    for case in cases.iter().filter(|case| case.expanded) {
        let base_indent = plan.indent_for(case.start, depths[case.start]);
        plan.set_indent(case.start..case.end + 1, base_indent);
        let mut nested_case_depth = 0usize;
        for (index, token) in tokens
            .iter()
            .enumerate()
            .take(case.end + 1)
            .skip(case.start + 1)
        {
            match token.kind {
                Token::Case => nested_case_depth += 1,
                Token::EndP if nested_case_depth > 0 => nested_case_depth -= 1,
                Token::When | Token::Else if nested_case_depth == 0 => {
                    plan.break_before(index, 1, base_indent + 1);
                }
                Token::EndP if nested_case_depth == 0 => {
                    plan.break_before(index, 1, base_indent);
                }
                _ => {}
            }
        }
    }
}

fn cte_blocks(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    parens: &HashMap<usize, usize>,
) -> Vec<CteBlock> {
    let mut result = Vec::new();

    for (with_index, token) in tokens.iter().enumerate() {
        if token.kind != Token::With {
            continue;
        }
        let base_depth = depths[with_index];
        let mut definitions = Vec::new();
        let mut main_select = None;

        for index in with_index + 1..tokens.len() {
            if depths[index] < base_depth || tokens[index].kind == Token::Ascii59 {
                break;
            }
            if depths[index] == base_depth && tokens[index].kind == Token::Select {
                main_select = Some(index);
                break;
            }
            if depths[index] != base_depth || tokens[index].kind != Token::As {
                continue;
            }
            let open = (index + 1..tokens.len()).take(4).find(|&candidate| {
                depths[candidate] == base_depth && tokens[candidate].kind == Token::Ascii40
            });
            let Some(open) = open else {
                continue;
            };
            let Some(&close) = parens.get(&open) else {
                continue;
            };
            if (open + 1..close).any(|candidate| tokens[candidate].kind == Token::Select) {
                definitions.push((open, close));
            }
        }

        if let Some(main_select) = main_select.filter(|_| !definitions.is_empty()) {
            result.push(CteBlock {
                with_index,
                ctes: definitions,
                main_select,
            });
        }
    }

    result
}

fn plan_ctes(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    blocks: &[CteBlock],
    plan: &mut LayoutPlan,
) {
    for block in blocks {
        let base_indent = depths[block.with_index];
        for (position, &(open, close)) in block.ctes.iter().enumerate() {
            if open + 1 < close {
                plan.break_before(open + 1, 1, base_indent + 1);
            }
            plan.break_before(close, 1, base_indent);

            let after_close = close + 1;
            if tokens
                .get(after_close)
                .is_some_and(|token| token.kind == Token::Ascii44)
            {
                if after_close + 1 < tokens.len() {
                    let blank = tokens[after_close + 1].line_breaks_before > 1;
                    plan.break_before(after_close + 1, usize::from(blank) + 1, base_indent);
                }
            } else if position + 1 == block.ctes.len() {
                plan.break_before(block.main_select, 1, base_indent);
            }

            for index in open + 1..close {
                if depths[index] != base_indent + 1 || tokens[index].kind != Token::Union {
                    continue;
                }
                plan.break_before(index, 2, base_indent + 1);
                let next_select =
                    (index + 1..close).find(|&candidate| tokens[candidate].kind == Token::Select);
                if let Some(next_select) = next_select {
                    plan.break_before(next_select, 2, base_indent + 1);
                }
            }
        }
        plan.break_before(block.main_select, 1, base_indent);
    }
}

fn compact_width(tokens: &[SqlToken<'_>], start: usize, end: usize) -> usize {
    let mut width = 0usize;
    let mut previous = None;
    for index in start..end {
        if needs_space(tokens, previous, index) {
            width += 1;
        }
        width += render_token(tokens, index).chars().count();
        previous = Some(index);
    }
    width
}

fn is_select_list_terminator(tokens: &[SqlToken<'_>], index: usize) -> bool {
    matches!(
        tokens[index].kind,
        Token::From
            | Token::Where
            | Token::GroupP
            | Token::Having
            | Token::Order
            | Token::Limit
            | Token::Offset
            | Token::Union
            | Token::Intersect
            | Token::Except
            | Token::Returning
    )
}

fn is_major_clause_start(tokens: &[SqlToken<'_>], index: usize) -> bool {
    match tokens[index].kind {
        Token::From
        | Token::Where
        | Token::Having
        | Token::Limit
        | Token::Offset
        | Token::Returning => true,
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

fn render_token(tokens: &[SqlToken<'_>], index: usize) -> String {
    let token = &tokens[index];
    let previous = index.checked_sub(1).map(|previous| &tokens[previous]);
    let next = tokens.get(index + 1);

    if token.kind == Token::NotEquals {
        return "!=".into();
    }
    if token.kind == Token::Ident
        && !token.text.starts_with('"')
        && (next.is_some_and(|next| next.kind == Token::Ascii40)
            || previous.is_some_and(|previous| previous.kind == Token::Typecast))
    {
        return token.text.to_lowercase();
    }
    if is_type_keyword(token.kind)
        || (is_ordinary_function(token.kind)
            && !token.text.starts_with('"')
            && next.is_some_and(|next| next.kind == Token::Ascii40))
    {
        return token.text.to_lowercase();
    }
    if (token.keyword_kind == KeywordKind::ReservedKeyword || is_keyword_like(token.kind))
        && previous.is_none_or(|previous| previous.kind != Token::Ascii46)
        && next.is_none_or(|next| next.kind != Token::Ascii46)
    {
        return token.text.to_uppercase();
    }
    token.text.to_owned()
}

fn is_ordinary_function(kind: Token) -> bool {
    kind == Token::Ident
}

fn is_keyword_like(kind: Token) -> bool {
    matches!(
        kind,
        Token::All
            | Token::And
            | Token::As
            | Token::By
            | Token::Case
            | Token::Coalesce
            | Token::Cross
            | Token::Distinct
            | Token::Else
            | Token::EndP
            | Token::Except
            | Token::FalseP
            | Token::From
            | Token::Full
            | Token::GroupP
            | Token::Having
            | Token::InnerP
            | Token::Intersect
            | Token::Is
            | Token::Join
            | Token::Left
            | Token::Limit
            | Token::Natural
            | Token::Not
            | Token::NullP
            | Token::Nullif
            | Token::Offset
            | Token::On
            | Token::Or
            | Token::Order
            | Token::OuterP
            | Token::Recursive
            | Token::Returning
            | Token::Right
            | Token::Select
            | Token::Then
            | Token::TrueP
            | Token::Union
            | Token::When
            | Token::Where
            | Token::With
    )
}

fn is_type_keyword(kind: Token) -> bool {
    matches!(
        kind,
        Token::Bigint
            | Token::Bit
            | Token::BooleanP
            | Token::CharP
            | Token::Character
            | Token::DecimalP
            | Token::FloatP
            | Token::IntP
            | Token::Integer
            | Token::Interval
            | Token::Json
            | Token::Numeric
            | Token::Real
            | Token::Smallint
            | Token::TextP
            | Token::Time
            | Token::Timestamp
            | Token::Varchar
    )
}

fn needs_space(tokens: &[SqlToken<'_>], previous: Option<usize>, current: usize) -> bool {
    let Some(previous_index) = previous else {
        return false;
    };
    let previous = &tokens[previous_index];
    let current = &tokens[current];
    if previous.kind == Token::SqlComment
        || (previous.kind == Token::CComment && current.line_breaks_before > 0)
    {
        return false;
    }
    if matches!(
        current.kind,
        Token::Ascii44
            | Token::Ascii59
            | Token::Ascii41
            | Token::Ascii93
            | Token::Ascii46
            | Token::Typecast
    ) || matches!(
        previous.kind,
        Token::Ascii40 | Token::Ascii91 | Token::Ascii46 | Token::Typecast
    ) {
        return false;
    }
    if current.kind == Token::Ascii40
        && (previous.kind == Token::Ident
            || is_ordinary_function(previous.kind)
            || matches!(
                previous.kind,
                Token::Coalesce | Token::Nullif | Token::Greatest | Token::Least
            ))
    {
        return false;
    }
    if matches!(previous.kind, Token::Ascii43 | Token::Ascii45)
        && is_unary_sign(tokens, previous_index)
    {
        return false;
    }
    true
}

fn is_unary_sign(tokens: &[SqlToken<'_>], index: usize) -> bool {
    if !matches!(tokens[index].kind, Token::Ascii43 | Token::Ascii45) {
        return false;
    }
    let Some(previous) = index.checked_sub(1).map(|previous| tokens[previous].kind) else {
        return true;
    };
    matches!(
        previous,
        Token::Ascii40
            | Token::Ascii43
            | Token::Ascii44
            | Token::Ascii45
            | Token::Ascii47
            | Token::Ascii61
            | Token::And
            | Token::Else
            | Token::Op
            | Token::Or
            | Token::Select
            | Token::Then
            | Token::When
    )
}

struct Writer {
    output: String,
    indent_width: usize,
}

impl Writer {
    fn new(indent_width: usize) -> Self {
        Self {
            output: String::new(),
            indent_width,
        }
    }

    fn at_line_start(&self) -> bool {
        self.output.is_empty() || self.output.ends_with('\n') || self.output.ends_with(' ')
    }

    fn write(&mut self, text: &str) {
        self.output.push_str(text);
    }

    fn space(&mut self) {
        if !self.at_line_start() && !self.output.ends_with(' ') {
            self.output.push(' ');
        }
    }

    fn newline(&mut self, lines: usize, indent: usize) {
        while self.output.ends_with(' ') {
            self.output.pop();
        }
        if self.output.is_empty() {
            return;
        }

        let existing = self
            .output
            .as_bytes()
            .iter()
            .rev()
            .take_while(|byte| **byte == b'\n')
            .count();
        self.output
            .extend(std::iter::repeat_n('\n', lines.saturating_sub(existing)));
        self.output
            .extend(std::iter::repeat_n(' ', indent * self.indent_width));
    }

    fn finish(mut self) -> String {
        while self.output.ends_with([' ', '\n']) {
            self.output.pop();
        }
        if !self.output.is_empty() {
            self.output.push('\n');
        }
        self.output
    }
}
