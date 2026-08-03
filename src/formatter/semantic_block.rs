use std::collections::{HashMap, HashSet};

use pg_query::protobuf::Token;

use super::layout_ir::{
    InsertBlock, LayoutDocument, MergeAction, MergeBlock, PredicateBlock, QueryBlock,
    SetOperationBlock, WithBlock,
};
use super::ownership::SupportedDocument;
use super::structure::TokenStructure;
use super::tokens::{SqlToken, tokenize};
use super::{
    FormatDiagnostic, FormatOptions, FormatWarning, INDENT_WIDTH, NotEqualPolicy, SemicolonPolicy,
    SourceRange,
};

mod ddl;
mod expressions;
mod lists;
mod render;
mod statements;

use ddl::{
    plan_alter_tables, plan_create_indexes, plan_create_tables, plan_materialized_views,
    plan_values_statements, plan_views,
};
use expressions::{ExpressionSources, owned_expression_ranges};
use lists::{
    ParenthesizedListSources, parenthesized_lists, plan_keyword_list, plan_parenthesized_lists,
    plan_select_lists,
};
pub(in crate::formatter) use render::needs_space;
pub(super) use render::{
    is_compact_grammar_parenthesis, is_function_call_name, is_function_call_syntax,
    is_type_keyword, is_type_modifier_syntax, is_uppercase_builtin, render_token,
};
use statements::{
    plan_delete_statements, plan_insert_statements, plan_merge_statements, plan_relation_source,
    plan_update_statements, plan_utility_statements,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Break {
    lines: usize,
    indent: usize,
}

#[derive(Debug)]
struct LayoutPlan {
    before: HashMap<usize, Break>,
    token_indents: Vec<Option<usize>>,
    indent_offsets: Vec<usize>,
}

impl LayoutPlan {
    fn new(token_count: usize) -> Self {
        Self {
            before: HashMap::new(),
            token_indents: vec![None; token_count],
            indent_offsets: vec![0; token_count],
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

    fn line_indent_for(&self, index: usize, fallback: usize) -> usize {
        self.before
            .get(&index)
            .map(|line_break| line_break.indent)
            .unwrap_or_else(|| self.indent_for(index, fallback))
    }

    fn shift_indents(&mut self, range: std::ops::Range<usize>, levels: usize) {
        if levels == 0 {
            return;
        }
        for (index, line_break) in &mut self.before {
            if range.contains(index) {
                line_break.indent += levels;
            }
        }
        for indent in self.token_indents[range.clone()].iter_mut().flatten() {
            *indent += levels;
        }
        for offset in &mut self.indent_offsets[range] {
            *offset += levels;
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct BooleanRange {
    kind: ExpressionOwnerKind,
    start: usize,
    end: usize,
    base_depth: usize,
    root_indent: usize,
    root_depth: Option<usize>,
    wrapper_close: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExpressionOwnerKind {
    Predicate,
    SelectTarget,
    ReturningItem,
    AssignmentValue,
    ValuesItem,
    CaseCondition,
    CaseResult,
    FunctionArgument,
}

#[derive(Debug, Clone, Copy)]
struct ExpressionRange {
    kind: ExpressionOwnerKind,
    start: usize,
    end: usize,
    base_depth: usize,
    root_indent: usize,
    wrapper_close: Option<usize>,
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

#[derive(Debug, Clone, Copy)]
struct ParenthesizedList {
    open: usize,
    close: usize,
    expanded: bool,
}

#[derive(Debug, Clone, Copy)]
struct PlanningContext<'a, 'sql> {
    tokens: &'a [SqlToken<'sql>],
    depths: &'a [usize],
    cases: &'a [CaseRange],
    lists: &'a [ParenthesizedList],
    options: &'a FormatOptions,
}

#[derive(Debug, Clone, Copy, Default)]
struct TerminalSemicolonPlan {
    omit: Option<usize>,
    insert_after: Option<usize>,
}

pub(super) fn format(
    source: &str,
    options: &FormatOptions,
    document: &SupportedDocument,
) -> Result<String, FormatDiagnostic> {
    let tokens = tokenize(source)?;
    if tokens.is_empty() {
        return Ok(String::new());
    }

    let structure = TokenStructure::new(&tokens);
    let depths = structure.depths();
    let parens = structure.parenthesis_pairs();
    let layout = LayoutDocument::bind(document, &tokens, &structure)?;
    let cases = case_ranges(&tokens, options);
    let selects = layout.selects().cloned().collect::<Vec<_>>();
    let inserts = layout.inserts().cloned().collect::<Vec<_>>();
    let updates = layout.updates().cloned().collect::<Vec<_>>();
    let deletes = layout.deletes().cloned().collect::<Vec<_>>();
    let merges = layout.merges().cloned().collect::<Vec<_>>();
    let views = layout.views().cloned().collect::<Vec<_>>();
    let materialized_views = layout.materialized_views().cloned().collect::<Vec<_>>();
    let values = layout.values().cloned().collect::<Vec<_>>();
    let create_tables = layout.create_tables().cloned().collect::<Vec<_>>();
    let create_indexes = layout.create_indexes().cloned().collect::<Vec<_>>();
    let alter_tables = layout.alter_tables().cloned().collect::<Vec<_>>();
    let utilities = layout.utilities().copied().collect::<Vec<_>>();
    let parenthesized_lists = parenthesized_lists(
        &tokens,
        depths,
        parens,
        ParenthesizedListSources {
            cases: &cases,
            inserts: &inserts,
            merges: &merges,
            utilities: &utilities,
            values: &values,
        },
        options,
    );
    let context = PlanningContext {
        tokens: &tokens,
        depths,
        cases: &cases,
        lists: &parenthesized_lists,
        options,
    };
    let expression_ranges = owned_expression_ranges(
        &context,
        parens,
        ExpressionSources {
            predicates: layout.predicates(),
            queries: layout.queries(),
            inserts: &inserts,
            updates: &updates,
            deletes: &deletes,
            merges: &merges,
            values: &values,
            lists: &parenthesized_lists,
            cases: &cases,
        },
    );
    let boolean_ranges = boolean_ranges(&tokens, depths, &expression_ranges, parens, options);
    let mut plan = LayoutPlan::new(tokens.len());

    for span in layout.statement_spans().skip(1) {
        let authored_lines = tokens[span.start].line_breaks_before;
        if authored_lines > 0 {
            plan.break_before(span.start, authored_lines.min(2), span.base_depth);
        }
    }

    let mut expanded_selects = plan_select_lists(
        &tokens,
        depths,
        &cases,
        &parenthesized_lists,
        layout.queries(),
        options,
        &mut plan,
    );
    for update in &updates {
        if let Some(source) = &update.from {
            extend_relation_query_starts(&mut expanded_selects, &tokens, source);
        }
    }
    for delete in &deletes {
        if let Some(source) = &delete.using {
            extend_relation_query_starts(&mut expanded_selects, &tokens, source);
        }
    }
    for merge in &merges {
        extend_relation_query_starts(&mut expanded_selects, &tokens, &merge.source);
    }
    expanded_selects.extend(views.iter().map(|view| view.query_start));
    expanded_selects.extend(materialized_views.iter().map(|view| view.query_start));
    for select in &selects {
        if let Some(source) = &select.from {
            if source.items.len() > 1
                || !source.joins.is_empty()
                || source
                    .item_kinds
                    .iter()
                    .any(|kind| *kind != crate::formatter::ownership::RelationItemSpec::Relation)
            {
                expanded_selects.insert(select.query_start);
            }
            plan_relation_source(source, &mut plan);
        }
    }
    let insert_query_starts = plan_insert_statements(&context, &inserts, &mut plan);
    expanded_selects.extend(insert_query_starts);
    plan_update_statements(&context, &boolean_ranges, &updates, &mut plan);
    plan_delete_statements(&context, &boolean_ranges, &deletes, &mut plan);
    plan_merge_statements(&context, &merges, &mut plan);
    plan_views(&context, &views, &mut plan);
    plan_materialized_views(&context, &materialized_views, &mut plan);
    plan_values_statements(&context, &values, &mut plan);
    plan_create_tables(&context, &create_tables, &mut plan);
    plan_create_indexes(&context, &create_indexes, &mut plan);
    plan_alter_tables(&context, &alter_tables, &mut plan);
    plan_utility_statements(&context, &utilities, &mut plan);
    plan_parenthesized_lists(
        &tokens,
        depths,
        &cases,
        &parenthesized_lists,
        options,
        &mut plan,
    );
    plan_query_clauses(
        &context,
        layout.queries(),
        &boolean_ranges,
        layout.with_blocks(),
        &expanded_selects,
        &mut plan,
    );
    plan_window_blocks(&context, layout.window_blocks(), &mut plan);
    plan_set_operations(layout.set_operations(), &mut plan);
    plan_cases(&tokens, depths, &cases, &mut plan);
    plan_booleans(&tokens, depths, &boolean_ranges, parens, options, &mut plan);
    plan_expression_comment_continuations(&tokens, &expression_ranges, &mut plan);
    plan_ctes(&tokens, depths, layout.with_blocks(), &mut plan);

    let terminal_semicolon = terminal_semicolon_plan(&tokens, options.semicolon_policy);
    let mut writer = Writer::new();
    let mut previous_index = None;

    for (index, token) in tokens.iter().enumerate() {
        if terminal_semicolon.omit == Some(index) {
            continue;
        }

        // An inline comment belongs to the expression immediately before it.
        // Never let a later layout pass move it onto a standalone line; comment
        // attachment outranks width and canonical-layout preferences.
        let planned_break = plan
            .before
            .get(&index)
            .copied()
            .filter(|_| !token.is_comment() || token.line_breaks_before > 0);
        if let Some(line_break) = planned_break {
            writer.newline(line_break.lines, line_break.indent);
        } else if token.is_comment() && token.line_breaks_before > 0 {
            let lines = token.line_breaks_before.min(2);
            writer.newline(lines, plan.indent_for(index, depths[index]));
        }

        if needs_space(&tokens, previous_index, index) {
            writer.space();
        }
        writer.write(&render_token(&tokens, index, options));
        if terminal_semicolon.insert_after == Some(index) {
            writer.write(";");
        }

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

    Ok(writer.finish(source.ends_with('\n')))
}

fn extend_relation_query_starts(
    expanded: &mut HashSet<usize>,
    tokens: &[SqlToken<'_>],
    source: &super::layout_ir::RelationSourceBlock,
) {
    for range in &source.items {
        expanded
            .extend((range.start..range.end).filter(|index| tokens[*index].kind == Token::Select));
    }
}

fn plan_window_blocks(
    context: &PlanningContext<'_, '_>,
    blocks: &[super::layout_ir::WindowBlock],
    plan: &mut LayoutPlan,
) {
    for block in blocks {
        let authored = context.tokens[block.open + 1..block.close]
            .iter()
            .any(|token| token.line_breaks_before > 0);
        let width = compact_width(context.tokens, block.open, block.close + 1, context.options)
            + block.base_depth * INDENT_WIDTH;
        let has_multiple_sections = [block.partition_by, block.order_by, block.frame]
            .into_iter()
            .flatten()
            .count()
            > 1;
        if !authored && !has_multiple_sections && width <= context.options.soft_line_width {
            continue;
        }
        let indent = plan.indent_for(block.open, block.base_depth) + 1;
        let first = block
            .partition_by
            .or(block.order_by)
            .or(block.frame)
            .unwrap_or(block.open + 1);
        plan.break_before(first, 1, indent);
        for boundary in [block.partition_by, block.order_by, block.frame]
            .into_iter()
            .flatten()
        {
            plan.break_before(boundary, 1, indent);
        }
        plan.set_indent(block.open + 1..block.close, indent);
        plan.break_before(
            block.close,
            1,
            plan.indent_for(block.open, block.base_depth),
        );
    }
}

fn terminal_semicolon_plan(
    tokens: &[SqlToken<'_>],
    policy: SemicolonPolicy,
) -> TerminalSemicolonPlan {
    let Some(last_syntax) = tokens.iter().rposition(|token| !token.is_comment()) else {
        return TerminalSemicolonPlan::default();
    };
    let has_terminal_semicolon = tokens[last_syntax].kind == Token::Ascii59;

    match (policy, has_terminal_semicolon) {
        (SemicolonPolicy::Preserve, _) => TerminalSemicolonPlan::default(),
        (SemicolonPolicy::Require, false) => TerminalSemicolonPlan {
            insert_after: Some(last_syntax),
            ..TerminalSemicolonPlan::default()
        },
        (SemicolonPolicy::Omit, true) => TerminalSemicolonPlan {
            omit: Some(last_syntax),
            ..TerminalSemicolonPlan::default()
        },
        (SemicolonPolicy::Require | SemicolonPolicy::Omit, _) => TerminalSemicolonPlan::default(),
    }
}

pub(super) fn validate_hard_width(
    output: &str,
    options: &FormatOptions,
) -> Result<Vec<FormatWarning>, FormatDiagnostic> {
    validate_hard_width_except(output, options, &[])
}

pub(super) fn validate_hard_width_except(
    output: &str,
    options: &FormatOptions,
    ignored_ranges: &[SourceRange],
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
        let ignored = ignored_ranges
            .iter()
            .any(|range| range.start < line_end && range.end > line_start);
        if width > options.hard_line_width && !ignored {
            let indent = line
                .chars()
                .take_while(|character| *character == ' ')
                .count();
            let indivisible = tokens.iter().any(|token| {
                token.start >= line_start
                    && token.end <= line_end
                    && (indent + token.text.chars().count() > options.hard_line_width
                        || (token.is_comment()
                            && output[line_start..token.end].chars().count()
                                > options.hard_line_width))
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
                let compact_width = compact_width(tokens, start, index + 1, options);
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

fn plan_query_clauses(
    context: &PlanningContext<'_, '_>,
    queries: &[QueryBlock],
    boolean_ranges: &[BooleanRange],
    with_blocks: &[WithBlock],
    expanded_selects: &HashSet<usize>,
    plan: &mut LayoutPlan,
) {
    let tokens = context.tokens;
    let depths = context.depths;
    let options = context.options;
    let with_body_starts: HashSet<_> = with_blocks.iter().map(|with| with.body_start).collect();
    let cte_body_selects: HashSet<_> = with_blocks
        .iter()
        .flat_map(|with| {
            with.definitions.iter().filter_map(|&(open, close)| {
                let body_depth = depths[open] + 1;
                let body_start = (open + 1..close)
                    .find(|index| depths[*index] == body_depth && !tokens[*index].is_comment())?;
                if !matches!(tokens[body_start].kind, Token::Select | Token::With) {
                    return None;
                }
                queries
                    .iter()
                    .find(|query| {
                        query.select > open
                            && query.select < close
                            && query.base_depth == body_depth
                    })
                    .map(|query| query.select)
            })
        })
        .collect();

    for query in queries {
        let select = query.select;
        let base_depth = query.base_depth;
        let indent = query.indent;
        let end = query.end;
        let has_join = (select + 1..end)
            .any(|index| depths[index] == base_depth && is_join_start(tokens, index));
        let has_expanded_boolean = boolean_ranges
            .iter()
            .any(|range| range.start > select && range.start < end);
        let nested_in_expanded_boolean = boolean_ranges
            .iter()
            .any(|range| range.start < select && select < range.end);
        let width_driven = indent * INDENT_WIDTH + compact_width(tokens, select, end, options)
            > options.soft_line_width;
        let expanded = expanded_selects.contains(&select)
            || has_join
            || has_expanded_boolean
            || nested_in_expanded_boolean
            || with_body_starts.contains(&select)
            || cte_body_selects.contains(&select)
            || width_driven;
        if !expanded {
            continue;
        }

        if let Some((_open, close)) = query.wrapper {
            plan.break_before(select, 1, indent);
            plan.set_indent(select..close, indent);
            plan.break_before(close, 1, indent.saturating_sub(1));
        }

        for boundary in query.clauses.ordered_boundaries(end) {
            if boundary < end {
                plan.break_before(boundary, 1, indent);
            }
        }
        if query.clauses.locking.is_some() {
            for index in select + 1..end {
                if depths[index] == base_depth && tokens[index].kind == Token::For {
                    plan.break_before(index, 1, indent);
                }
            }
        }
        for (index, depth) in depths.iter().enumerate().take(end).skip(select + 1) {
            if *depth == base_depth && is_join_start(tokens, index) {
                plan.break_before(index, 1, indent);
            }
        }
        for clause in [query.clauses.group_by, query.clauses.order_by]
            .into_iter()
            .flatten()
        {
            let by = clause + 1;
            let list_end = query.clauses.next_after(clause, end);
            plan_keyword_list(context, by, list_end, base_depth, false, plan);
        }
        if let Some(window) = query.clauses.window {
            let list_end = query.clauses.next_after(window, end);
            plan_keyword_list(context, window, list_end, base_depth, false, plan);
        }
    }
}

fn plan_set_operations(operations: &[SetOperationBlock], plan: &mut LayoutPlan) {
    for operation in operations {
        plan.break_before(operation.operator, 2, operation.base_depth);
        plan.break_before(operation.next_branch, 2, operation.base_depth);
    }
}

fn boolean_ranges(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    expressions: &[ExpressionRange],
    parens: &HashMap<usize, usize>,
    options: &FormatOptions,
) -> Vec<BooleanRange> {
    let mut result = Vec::new();

    for expression in expressions {
        let root_depth = boolean_root_depth(tokens, depths, parens, *expression);
        let has_and = (expression.start..expression.end)
            .any(|candidate| tokens[candidate].kind == Token::And);
        let has_or =
            (expression.start..expression.end).any(|candidate| tokens[candidate].kind == Token::Or);
        let contains_nested_sql = (expression.start..expression.end).any(|candidate| {
            depths[candidate] > expression.base_depth
                && matches!(tokens[candidate].kind, Token::Select | Token::With)
        });
        let hides_structure = expression.root_indent * INDENT_WIDTH
            + compact_width(tokens, expression.start, expression.end, options)
            > options.soft_line_width;
        let authored_predicate = expression.kind == ExpressionOwnerKind::Predicate
            && tokens[expression.start..expression.end]
                .iter()
                .any(|token| token.is_comment() || token.line_breaks_before > 0);
        let expanded_boolean = root_depth.is_some()
            && ((has_and && has_or) || contains_nested_sql || authored_predicate);
        let expanded = expanded_boolean
            || (hides_structure
                && (expression.kind == ExpressionOwnerKind::Predicate || root_depth.is_some()));

        if expanded {
            result.push(BooleanRange {
                kind: expression.kind,
                start: expression.start,
                end: expression.end,
                base_depth: expression.base_depth,
                root_indent: expression.root_indent,
                root_depth,
                wrapper_close: expression.wrapper_close,
            });
        }
    }

    result
}

fn boolean_root_depth(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    parens: &HashMap<usize, usize>,
    range: ExpressionRange,
) -> Option<usize> {
    let mut start = range.start;
    let mut end = range.end;
    while start < end && tokens[start].kind == Token::Ascii40 {
        let close = *parens.get(&start)?;
        if close + 1 != end {
            break;
        }
        start += 1;
        end = close;
        while start < end && tokens[start].is_comment() {
            start += 1;
        }
    }
    if start >= end {
        return None;
    }
    let root_depth = depths[start];
    let first_connector_depth = (start..end)
        .filter(|index| matches!(tokens[*index].kind, Token::And | Token::Or))
        .map(|index| depths[index])
        .min()?;
    (first_connector_depth == root_depth || tokens[start].kind == Token::Not)
        .then_some(first_connector_depth)
}

fn plan_booleans(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    ranges: &[BooleanRange],
    parens: &HashMap<usize, usize>,
    options: &FormatOptions,
    plan: &mut LayoutPlan,
) {
    for range in ranges {
        let root_has_and = range.root_depth.is_some_and(|root_depth| {
            (range.start..range.end)
                .any(|index| depths[index] == root_depth && tokens[index].kind == Token::And)
        });
        let root_indent = plan
            .line_indent_for(
                range.start,
                range.root_indent + plan.indent_offsets[range.start],
            )
            .max(range.root_indent + plan.indent_offsets[range.start]);
        if !matches!(
            range.kind,
            ExpressionOwnerKind::AssignmentValue
                | ExpressionOwnerKind::CaseCondition
                | ExpressionOwnerKind::CaseResult
        ) {
            plan.break_before(range.start, 1, root_indent);
        }
        for index in range.start..range.end {
            if range.root_depth == Some(depths[index])
                && matches!(tokens[index].kind, Token::And | Token::Or)
            {
                plan.break_before(
                    index,
                    1,
                    root_indent + depths[index].saturating_sub(range.base_depth),
                );
            }
            if tokens[index].kind == Token::Ascii40 {
                let Some(&close) = parens.get(&index) else {
                    continue;
                };
                if close >= range.end {
                    continue;
                }
                let inner_depth = depths[index] + 1;
                let query_wrapper = (index + 1..close)
                    .find(|candidate| !tokens[*candidate].is_comment())
                    .filter(|candidate| {
                        matches!(tokens[*candidate].kind, Token::Select | Token::With)
                    });
                if let Some(query_start) = query_wrapper {
                    if plan.before.contains_key(&query_start) {
                        let query_indent =
                            root_indent + 1 + depths[index].saturating_sub(range.base_depth);
                        let current_indent = plan.line_indent_for(query_start, query_indent);
                        plan.shift_indents(
                            query_start..close,
                            query_indent.saturating_sub(current_indent),
                        );
                        plan.break_before(
                            close,
                            1,
                            root_indent + depths[close].saturating_sub(range.base_depth),
                        );
                    }
                    continue;
                }
                let direct_connectors = (index + 1..close)
                    .filter(|candidate| {
                        depths[*candidate] == inner_depth
                            && matches!(tokens[*candidate].kind, Token::And | Token::Or)
                    })
                    .collect::<Vec<_>>();
                let contains_boolean = !direct_connectors.is_empty();
                let independently_complex = direct_connectors.len() > 1;
                let mixed_boolean = direct_connectors
                    .iter()
                    .any(|candidate| tokens[*candidate].kind == Token::And)
                    && direct_connectors
                        .iter()
                        .any(|candidate| tokens[*candidate].kind == Token::Or);
                let precedence_boundary = root_has_and
                    && direct_connectors
                        .iter()
                        .any(|candidate| tokens[*candidate].kind == Token::Or);
                let contains_nested_sql = (index + 1..close).any(|candidate| {
                    depths[candidate] == inner_depth
                        && matches!(tokens[candidate].kind, Token::Select | Token::With)
                });
                let authored_boundary = tokens[index + 1..close]
                    .iter()
                    .any(|token| token.is_comment() || token.line_breaks_before > 1);
                let over_soft = root_indent * INDENT_WIDTH
                    + compact_width(tokens, index, close + 1, options)
                    > options.soft_line_width;
                let owns_complete_range = index == range.start && close + 1 == range.end;
                if contains_boolean
                    && (owns_complete_range
                        || precedence_boundary
                        || independently_complex
                        || mixed_boolean
                        || contains_nested_sql
                        || authored_boundary
                        || over_soft)
                {
                    if index + 1 < close {
                        plan.break_before(
                            index + 1,
                            1,
                            root_indent + inner_depth.saturating_sub(range.base_depth),
                        );
                    }
                    for connector in direct_connectors {
                        plan.break_before(
                            connector,
                            1,
                            root_indent + depths[connector].saturating_sub(range.base_depth),
                        );
                    }
                    plan.break_before(
                        close,
                        1,
                        root_indent + depths[close].saturating_sub(range.base_depth),
                    );
                }
            }
        }
        if let Some(close) = range.wrapper_close {
            plan.break_before(close, 1, root_indent.saturating_sub(1));
        }
    }
}

fn plan_expression_comment_continuations(
    tokens: &[SqlToken<'_>],
    expressions: &[ExpressionRange],
    plan: &mut LayoutPlan,
) {
    for expression in expressions {
        if expression.start > 0
            && tokens[expression.start - 1].is_comment()
            && tokens[expression.start].line_breaks_before > 0
        {
            plan.break_before(expression.start, 1, expression.root_indent);
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
        let base_indent = plan.line_indent_for(case.start, depths[case.start]);
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

fn plan_ctes(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    blocks: &[WithBlock],
    plan: &mut LayoutPlan,
) {
    for block in blocks {
        let base_indent = depths[block.with_index];
        for (position, &(open, close)) in block.definitions.iter().enumerate() {
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
            } else if position + 1 == block.definitions.len() {
                for (clause, token) in tokens
                    .iter()
                    .enumerate()
                    .take(block.body_start)
                    .skip(close + 1)
                {
                    if matches!(token.kind, Token::Search | Token::Cycle) {
                        plan.break_before(clause, 1, base_indent);
                    }
                }
                plan.break_before(block.body_start, 1, base_indent);
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
        plan.break_before(block.body_start, 1, base_indent);
    }
}

fn range_is_unavoidably_over_hard(
    tokens: &[SqlToken<'_>],
    start: usize,
    end: usize,
    indent: usize,
    options: &FormatOptions,
) -> bool {
    let mut width = indent * INDENT_WIDTH;
    let mut previous = None;
    for index in start..end {
        if needs_space(tokens, previous, index) {
            width += 1;
        }
        let token_width = render_token(tokens, index, options).chars().count();
        width += token_width;
        if indent * INDENT_WIDTH + token_width > options.hard_line_width
            || (tokens[index].is_comment() && width > options.hard_line_width)
        {
            return true;
        }
        previous = Some(index);
    }
    false
}

fn compact_width(
    tokens: &[SqlToken<'_>],
    start: usize,
    end: usize,
    options: &FormatOptions,
) -> usize {
    let mut width = 0usize;
    let mut previous = None;
    for index in start..end {
        if needs_space(tokens, previous, index) {
            width += 1;
        }
        width += render_token(tokens, index, options).chars().count();
        previous = Some(index);
    }
    width
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

struct Writer {
    output: String,
}

impl Writer {
    fn new() -> Self {
        Self {
            output: String::new(),
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
            .extend(std::iter::repeat_n(' ', indent * INDENT_WIDTH));
    }

    fn finish(mut self, trailing_newline: bool) -> String {
        while self.output.ends_with([' ', '\n']) {
            self.output.pop();
        }
        if trailing_newline && !self.output.is_empty() {
            self.output.push('\n');
        }
        self.output
    }
}

#[cfg(test)]
mod hard_width_tests {
    use super::*;

    #[test]
    fn short_indivisible_token_past_the_limit_does_not_excuse_a_breakable_line() {
        let output = "CHECK ((status = 'processing' AND claim_token IS NOT NULL AND claimed_revision IS NOT NULL AND processing_started_at IS NOT NULL) OR (status != 'processing' AND claim_token IS NULL));";
        let options = FormatOptions {
            soft_line_width: 32,
            hard_line_width: 40,
            ..FormatOptions::default()
        };

        assert!(matches!(
            validate_hard_width(output, &options),
            Err(FormatDiagnostic::HardLineExceeded { line: 1, .. })
        ));
    }
}
