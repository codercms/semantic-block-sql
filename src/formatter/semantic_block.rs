use crate::text::SourceIndex;
use std::collections::{HashMap, HashSet};

use pg_query::protobuf::Token;

use super::layout_ir::{
    InsertBlock, LayoutDocument, MergeAction, MergeBlock, PredicateBlock, QueryBlock,
    SetOperationBlock, WithBlock,
};
use super::ownership::SupportedDocument;
use super::structure::TokenStructure;
use super::tokens::{SqlToken, TokenRole, is_join_start, tokenize};
use super::{
    FormatDiagnostic, FormatOptions, FormatWarning, INDENT_WIDTH, NotEqualPolicy, SemicolonPolicy,
    SourceRange,
};

mod ddl;
mod expressions;
mod geometry;
mod groups;

use geometry::{LayoutPlan, compact_width};
mod lists;
mod render;
mod statements;

use ddl::{
    plan_alter_tables, plan_create_indexes, plan_create_tables, plan_materialized_views,
    plan_values_statements, plan_views,
};
use expressions::{ExpressionSources, owned_expression_ranges};
use groups::{
    GroupLayout, LayoutGroup, has_hard_boundary, has_list_hard_boundary, plan_clause_boundaries,
};
use lists::{
    ParenthesizedListSources, parenthesized_lists, plan_keyword_list_at_indent,
    plan_parenthesized_lists, plan_select_lists,
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

#[derive(Debug, Clone, Copy)]
struct BooleanRange {
    expanded: bool,
    join_on: Option<usize>,
    kind: ExpressionOwnerKind,
    introducer: Option<usize>,
    start: usize,
    end: usize,
    base_depth: usize,
    root_indent: usize,
    root_depth: Option<usize>,
    preserve_authored_breaks: bool,
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
    base_indent: Option<usize>,
    arguments: super::layout_ir::FunctionArgumentLayout,
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
    let mut tokens = tokenize(source)?;
    if tokens.is_empty() {
        return Ok(String::new());
    }

    let structure = TokenStructure::new(&tokens);
    let depths = structure.depths();
    let parens = structure.parenthesis_pairs();
    let layout = LayoutDocument::bind(document, &tokens, &structure)?;
    for &index in layout.identifier_tokens() {
        tokens[index].role = TokenRole::Identifier;
    }
    for call in layout.function_calls() {
        tokens[call.name].role = TokenRole::FunctionName;
    }
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
    let utilities = layout.utilities().cloned().collect::<Vec<_>>();
    let mut join_using_lists = layout
        .queries()
        .iter()
        .flat_map(|query| {
            query.from.iter().flat_map(move |source| {
                source.joins.iter().filter_map(move |join| {
                    join.using_open.map(|open| {
                        (
                            open,
                            query.indent + depths[open].saturating_sub(query.base_depth),
                        )
                    })
                })
            })
        })
        .collect::<Vec<_>>();
    join_using_lists.extend(
        updates
            .iter()
            .filter_map(|update| update.from.as_ref())
            .chain(deletes.iter().filter_map(|delete| delete.using.as_ref()))
            .chain(merges.iter().map(|merge| &merge.source))
            .flat_map(|source| {
                source
                    .joins
                    .iter()
                    .filter_map(|join| join.using_open.map(|open| (open, depths[open])))
            }),
    );
    let definition_headers = layout
        .queries()
        .iter()
        .filter_map(|query| query.from.as_ref().map(|source| (source, query.indent)))
        .chain(
            updates
                .iter()
                .filter_map(|update| update.from.as_ref())
                .map(|source| (source, source.base_depth)),
        )
        .chain(
            deletes
                .iter()
                .filter_map(|delete| delete.using.as_ref())
                .map(|source| (source, source.base_depth)),
        )
        .chain(
            merges
                .iter()
                .map(|merge| (&merge.source, merge.source.base_depth)),
        )
        .flat_map(|(source, indent)| {
            source.definition_lists.iter().map(move |&(open, _)| {
                let prefix = source
                    .joins
                    .iter()
                    .filter(|join| {
                        join.start < open && join.predicate.is_none_or(|(on, _)| open < on)
                    })
                    .map(|join| join.start)
                    .max()
                    .or_else(|| {
                        source
                            .items
                            .iter()
                            .find(|item| item.start <= open && open < item.end)
                            .map(|item| item.start)
                    })
                    .unwrap_or(source.range.start);
                (
                    open,
                    prefix,
                    indent + depths[open].saturating_sub(source.base_depth),
                )
            })
        })
        .collect::<Vec<_>>();
    let parenthesized_lists = parenthesized_lists(
        &tokens,
        depths,
        parens,
        ParenthesizedListSources {
            cases: &cases,
            inserts: &inserts,
            merges: &merges,
            join_using_lists: &join_using_lists,
            utilities: &utilities,
            values: &values,
            arrays: layout.arrays(),
            calls: layout.function_calls(),
            definition_headers: &definition_headers,
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
    let boolean_ranges = boolean_ranges(
        &tokens,
        depths,
        &expression_ranges,
        layout.queries(),
        layout.predicates(),
        parens,
        options,
    );
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
            plan_relation_source(source, depths, &mut plan);
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
    plan_create_tables(&context, &create_tables, &boolean_ranges, &mut plan);
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
    expanded_selects.extend(
        layout
            .window_blocks()
            .iter()
            .filter(|block| window_block_expands(&context, block))
            .map(|block| block.query_start),
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
    plan_set_operations(&context, layout.set_operations(), &mut plan);
    plan_cases(&tokens, depths, &cases, &mut plan);
    plan_booleans(&tokens, depths, &boolean_ranges, parens, options, &mut plan);
    // Array constructors may live in CASE/predicate groups whose final parent
    // line is planned after argument lists. Rebase the already-owned bracket
    // subtree, including nested CASE/query groups, instead of replanning it.
    for &(open, close) in layout.arrays() {
        if let Some(current) = plan.before.get(&close).map(|line_break| line_break.indent) {
            let desired = plan
                .before
                .iter()
                .filter(|(index, _)| **index <= open)
                .max_by_key(|(index, _)| **index)
                .map_or(current, |(_, line_break)| line_break.indent);
            plan.rebase_indents(open + 1..close + 1, current, desired);
        }
    }
    plan_case_result_boundaries(&tokens, &expression_ranges, options, &mut plan);
    plan_expression_comment_continuations(&tokens, &expression_ranges, &mut plan);
    plan_ctes(&tokens, depths, layout.with_blocks(), &mut plan);

    let terminal_semicolon = terminal_semicolon_plan(&tokens, options.semicolon_policy);
    Ok(render_plan(
        &tokens,
        depths,
        &plan,
        terminal_semicolon,
        options,
        source.ends_with('\n'),
    ))
}

/// Emit a parser-confirmed procedural command without a SQL statement adapter.
pub(super) fn format_procedural_command(
    source: &str,
    options: &FormatOptions,
) -> Result<String, FormatDiagnostic> {
    let tokens = tokenize(source)?;
    let structure = TokenStructure::new(&tokens);
    Ok(render_plan(
        &tokens,
        structure.depths(),
        &LayoutPlan::new(tokens.len()),
        terminal_semicolon_plan(&tokens, SemicolonPolicy::Preserve),
        options,
        source.ends_with('\n'),
    ))
}

fn render_plan(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    plan: &LayoutPlan,
    terminal_semicolon: TerminalSemicolonPlan,
    options: &FormatOptions,
    ends_with_newline: bool,
) -> String {
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

        if needs_space(tokens, previous_index, index) {
            writer.space();
        }
        writer.write(&render_token(tokens, index, options));
        if terminal_semicolon.insert_after == Some(index) {
            writer.write(";");
        }

        if token.is_comment() {
            let next_line_breaks = tokens
                .get(index + 1)
                .map_or(0, |next| next.line_breaks_before);
            if token.kind == Token::SqlComment || next_line_breaks > 0 {
                let indent = plan.indent_for(index, depths[index]);
                writer.newline(next_line_breaks.max(1), indent);
            }
        }

        previous_index = Some(index);
    }

    writer.finish(ends_with_newline)
}

fn query_indent(query: &QueryBlock, plan: &LayoutPlan) -> usize {
    query.wrapper.map_or(query.indent, |(open, _close)| {
        plan.line_indent_for(open, query.indent.saturating_sub(1)) + 1
    })
}

/// Render only a parser-bound routine declaration prefix. Its body is owned
/// by the SQL/procedural adapter and never enters this whitespace planner.
pub(super) fn format_routine_header(
    source: &str,
    owned_lists: &[(usize, usize)],
    clause_starts: &[usize],
    literal_arguments: &[usize],
    options: &FormatOptions,
) -> Result<String, FormatDiagnostic> {
    let mut tokens = tokenize(source)?;
    let structure = TokenStructure::new(&tokens);
    let depths = structure.depths();
    // Grammar casing has already been normalized by parser-owned locations.
    // Header layout has no authority to recase identifiers or type names.
    for token in &mut tokens {
        token.role = TokenRole::Identifier;
    }
    let header_expands = LayoutGroup {
        compact_line_width: source
            .lines()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0),
        structurally_complex: false,
        hard_boundary: false,
        force_expand: false,
        compact_overflow_is_unavoidable: false,
    }
    .decide(options)
        == GroupLayout::Expanded;
    let lists = owned_lists
        .iter()
        .map(|&(open, close)| {
            let authored = tokens[open + 1..close]
                .iter()
                .any(|token| token.line_breaks_before > 0);
            ParenthesizedList {
                open,
                close,
                expanded: open + 1 < close
                    && LayoutGroup {
                        compact_line_width: 0,
                        structurally_complex: false,
                        hard_boundary: has_hard_boundary(&tokens, open + 1, close),
                        force_expand: authored || header_expands,
                        compact_overflow_is_unavoidable: false,
                    }
                    .decide(options)
                        == GroupLayout::Expanded,
                base_indent: Some(0),
                arguments: super::layout_ir::FunctionArgumentLayout::Ordinary,
            }
        })
        .collect::<Vec<_>>();
    if !header_expands && !lists.iter().any(|list| list.expanded) {
        return Ok(source.to_owned());
    }
    let mut plan = LayoutPlan::new(tokens.len());
    for (index, token) in tokens.iter().enumerate() {
        if token.line_breaks_before > 0 {
            plan.break_before(index, token.line_breaks_before, depths[index]);
        }
    }
    plan_parenthesized_lists(&tokens, depths, &[], &lists, options, &mut plan);
    for &index in clause_starts {
        plan.break_before(index, tokens[index].line_breaks_before.max(1), 0);
    }
    if let (Some(&first), Some(&last)) = (literal_arguments.first(), literal_arguments.last()) {
        let as_index = clause_starts
            .iter()
            .copied()
            .find(|&index| index < first && tokens[index].kind == Token::As)
            .ok_or_else(|| {
                FormatDiagnostic::Ownership("external literals have no owned AS clause".into())
            })?;
        let width = compact_width(&tokens, as_index, last + 1, options);
        if (LayoutGroup {
            compact_line_width: width,
            structurally_complex: false,
            hard_boundary: has_hard_boundary(&tokens, as_index, last + 1),
            force_expand: literal_arguments
                .iter()
                .any(|&index| tokens[index].line_breaks_before > 0),
            compact_overflow_is_unavoidable: false,
        })
        .decide(options)
            == GroupLayout::Expanded
        {
            for &index in literal_arguments {
                plan.break_before(index, tokens[index].line_breaks_before.max(1), 1);
            }
        }
    }
    let mut output = render_plan(
        &tokens,
        depths,
        &plan,
        TerminalSemicolonPlan::default(),
        options,
        source.ends_with('\n'),
    );
    // Preserve the framing gap between AS and the literal owned by the body.
    if let Some(last) = tokens.last() {
        output.truncate(output.trim_end().len());
        output.push_str(&source[last.end..]);
    }
    Ok(output)
}

pub(super) fn identifier_spellings(
    source: &str,
    document: &SupportedDocument,
) -> Result<Vec<String>, FormatDiagnostic> {
    let tokens = tokenize(source)?;
    let structure = TokenStructure::new(&tokens);
    let layout = LayoutDocument::bind(document, &tokens, &structure)?;
    Ok(layout
        .identifier_tokens()
        .iter()
        .map(|index| tokens[*index].text.to_owned())
        .collect())
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

fn window_block_expands(
    context: &PlanningContext<'_, '_>,
    block: &super::layout_ir::WindowBlock,
) -> bool {
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
    authored || has_multiple_sections || width > context.options.soft_line_width
}

fn plan_window_blocks(
    context: &PlanningContext<'_, '_>,
    blocks: &[super::layout_ir::WindowBlock],
    plan: &mut LayoutPlan,
) {
    for block in blocks {
        if !window_block_expands(context, block) {
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

        for section in [block.partition_by, block.order_by].into_iter().flatten() {
            let by = section + 1;
            let list_end = [block.partition_by, block.order_by, block.frame]
                .into_iter()
                .flatten()
                .filter(|candidate| *candidate > section)
                .min()
                .unwrap_or(block.close);
            plan_keyword_list_at_indent(
                context,
                by,
                list_end,
                context.depths[by],
                indent,
                false,
                plan,
            );
        }
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
    let source_index = SourceIndex::new(output);

    for (line_number, range) in source_index.lines() {
        let line_start = range.start;
        let line_end = range.end;
        let line = &output[line_start..line_end];
        let width = source_index
            .line_width(line_number)
            .expect("indexed physical line");
        let ignored = ignored_ranges
            .iter()
            .any(|range| range.start < line_end && range.end > line_start);
        if width > options.hard_line_width && !ignored {
            let indent = line
                .chars()
                .take_while(|character| *character == ' ')
                .count();
            let indivisible = tokens.iter().any(|token| {
                let start = token.start.max(line_start);
                let end = token.end.min(line_end);
                if start >= end {
                    return false;
                }
                let token_indent = if token.start < line_start { 0 } else { indent };
                token_indent + output[start..end].chars().count() > options.hard_line_width
                    || (token.is_comment()
                        && token.start >= line_start
                        && token.end
                            <= source_index
                                .line_span(line_number)
                                .expect("indexed physical line")
                                .end
                        && output[line_start..end].chars().count() > options.hard_line_width)
            });
            if indivisible {
                warnings.push(FormatWarning::IndivisibleTokenExceedsHardWidth {
                    line: line_number,
                    width,
                });
            } else {
                return Err(FormatDiagnostic::HardLineExceeded {
                    line: line_number,
                    width,
                    hard_limit: options.hard_line_width,
                });
            }
        }
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
        let indent = query_indent(query, plan);
        if let Some(source) = &query.from {
            plan.token_indents[source.introducer] = Some(indent);
            plan_relation_source(source, depths, plan);
        }
        let end = query.end;
        let has_join = query
            .from
            .as_ref()
            .is_some_and(|source| !source.joins.is_empty());
        let has_expanded_boolean = boolean_ranges
            .iter()
            .filter(|range| range.expanded)
            .any(|range| range.start > select && range.start < end);
        let nested_in_expanded_boolean = boolean_ranges
            .iter()
            .filter(|range| range.expanded)
            .any(|range| range.start < select && select < range.end);
        let mut has_expanded_clause_list = false;
        for clause in [query.clauses.group_by, query.clauses.order_by]
            .into_iter()
            .flatten()
        {
            let by = clause + 1;
            let list_end = query.clauses.next_after(clause, end);
            has_expanded_clause_list |=
                plan_keyword_list_at_indent(context, by, list_end, base_depth, indent, false, plan);
        }
        if let Some(window) = query.clauses.window {
            let list_end = query.clauses.next_after(window, end);
            has_expanded_clause_list |= plan_keyword_list_at_indent(
                context, window, list_end, base_depth, indent, false, plan,
            );
        }

        let width_driven = indent * INDENT_WIDTH + compact_width(tokens, select, end, options)
            > options.soft_line_width;
        let expanded = expanded_selects.contains(&select)
            || has_join
            || has_expanded_boolean
            || nested_in_expanded_boolean
            || has_expanded_clause_list
            || with_body_starts.contains(&select)
            || cte_body_selects.contains(&select)
            || width_driven;
        // Authored clause boundaries are independent of width-driven expansion.
        // Preserve them without expanding the query's remaining inline clauses.
        let has_authored_clause = plan_clause_boundaries(
            context,
            query
                .clauses
                .ordered_boundaries(end)
                .into_iter()
                .filter(|boundary| *boundary < end),
            indent,
            expanded,
            plan,
        );
        if !expanded && !has_authored_clause {
            continue;
        }

        if let Some((_open, close)) = query.wrapper {
            plan.break_before(select, 1, indent);
            plan.set_fallback_indent(select..close, depths, base_depth, indent);
            plan.break_before(close, 1, indent.saturating_sub(1));
        }

        if query.clauses.locking.is_some() {
            for index in select + 1..end {
                if depths[index] == base_depth && tokens[index].kind == Token::For {
                    plan.break_before(index, tokens[index].line_breaks_before.clamp(1, 2), indent);
                }
            }
        }
        for (index, depth) in depths.iter().enumerate().take(end).skip(select + 1) {
            if *depth == base_depth && is_join_start(tokens, index) {
                plan.break_before(index, tokens[index].line_breaks_before.clamp(1, 2), indent);
            }
        }
    }
}

fn plan_set_operations(
    context: &PlanningContext<'_, '_>,
    operations: &[SetOperationBlock],
    plan: &mut LayoutPlan,
) {
    let tokens = context.tokens;
    let depths = context.depths;
    let options = context.options;

    for operation in operations {
        debug_assert_eq!(
            operation.branches.len(),
            operation.operators.len() + 1,
            "validated set-operation ownership must be complete",
        );

        if operation.owner_start < operation.owner_end {
            plan.break_before(operation.branches[0].start, 1, operation.base_depth);
        }
        if let Some((_, close)) = operation.owner_wrapper {
            plan.break_before(close, 1, operation.base_depth.saturating_sub(1));
        }
        if operation.owner_end < tokens.len()
            && matches!(
                tokens[operation.owner_end].kind,
                Token::Order | Token::Limit | Token::Offset | Token::Fetch | Token::For
            )
        {
            plan.break_before(operation.owner_end, 1, operation.base_depth);
        }

        for branch in &operation.branches {
            let Some((open, close)) = branch.wrapper else {
                continue;
            };
            let query_depth = depths[branch.query_start];
            let contains_nested_sql = (branch.query_start + 1..close).any(|index| {
                depths[index] > query_depth
                    && matches!(
                        tokens[index].kind,
                        Token::Select | Token::With | Token::Values
                    )
            });
            let authored_multiline = tokens[branch.query_start].line_breaks_before > 0
                || tokens[close].line_breaks_before > 0;
            let layout = LayoutGroup {
                compact_line_width: operation.base_depth * INDENT_WIDTH
                    + compact_width(tokens, open, close + 1, options),
                structurally_complex: contains_nested_sql,
                hard_boundary: has_hard_boundary(tokens, open + 1, close),
                force_expand: authored_multiline,
                compact_overflow_is_unavoidable: false,
            }
            .decide(options);
            if layout == GroupLayout::Expanded {
                plan.break_before(open, 1, operation.base_depth);
                plan.break_before(branch.query_start, 1, operation.base_depth + 1);
                plan.break_before(close, 1, operation.base_depth);
            }
        }
        for (position, operator) in operation.operators.iter().copied().enumerate() {
            plan.break_before(operator, 2, operation.base_depth);
            let branch = &operation.branches[position + 1];
            plan.break_before(branch.start, 2, operation.base_depth);
        }
    }
}

fn boolean_ranges(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    expressions: &[ExpressionRange],
    queries: &[QueryBlock],
    predicates: &[PredicateBlock],
    parens: &HashMap<usize, usize>,
    options: &FormatOptions,
) -> Vec<BooleanRange> {
    let mut result = Vec::new();

    for expression in expressions {
        let root_depth = boolean_root_depth(tokens, depths, parens, *expression, queries);
        let has_and = (expression.start..expression.end)
            .any(|candidate| tokens[candidate].kind == Token::And);
        let has_or =
            (expression.start..expression.end).any(|candidate| tokens[candidate].kind == Token::Or);
        let contains_nested_sql = (expression.start..expression.end).any(|candidate| {
            depths[candidate] > expression.base_depth
                && matches!(tokens[candidate].kind, Token::Select | Token::With)
        });
        let hard_boundary = has_hard_boundary(tokens, expression.start, expression.end);
        let structurally_complex =
            root_depth.is_some() && ((has_and && has_or) || contains_nested_sql);
        let compact_line_width = expression.root_indent * INDENT_WIDTH
            + compact_width(tokens, expression.start, expression.end, options);
        let authored_root_break = expression.kind == ExpressionOwnerKind::Predicate
            && predicate_has_authored_root_break(tokens, depths, *expression, root_depth);
        let natural_layout = LayoutGroup {
            compact_line_width,
            structurally_complex,
            hard_boundary,
            force_expand: false,
            compact_overflow_is_unavoidable: false,
        }
        .decide(options);
        let layout = LayoutGroup {
            compact_line_width,
            structurally_complex,
            hard_boundary,
            force_expand: authored_root_break,
            compact_overflow_is_unavoidable: false,
        }
        .decide(options);
        let expanded = layout == GroupLayout::Expanded
            && (expression.kind == ExpressionOwnerKind::Predicate || root_depth.is_some());

        let predicate = predicates.iter().find(|predicate| {
            predicate.start == expression.start && predicate.wrapper_close.is_none()
        });
        let join_on = predicate
            .filter(|predicate| predicate.kind == super::layout_ir::PredicateKind::JoinOn)
            .map(|predicate| predicate.introducer);
        // JOIN header geometry is available only after its relation/query owner
        // is planned. Retain compact candidates for that final width decision.
        if expanded || join_on.is_some() {
            result.push(BooleanRange {
                expanded,
                join_on,
                kind: expression.kind,
                introducer: predicate.map(|predicate| predicate.introducer),
                start: expression.start,
                end: expression.end,
                base_depth: expression.base_depth,
                root_indent: expression.root_indent,
                root_depth,
                preserve_authored_breaks: authored_root_break
                    && natural_layout == GroupLayout::Compact,
                wrapper_close: expression.wrapper_close,
            });
        }
    }

    result
}

fn predicate_has_authored_root_break(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    expression: ExpressionRange,
    root_depth: Option<usize>,
) -> bool {
    tokens[expression.start].line_breaks_before > 0
        || root_depth.is_some_and(|root_depth| {
            (expression.start + 1..expression.end).any(|index| {
                depths[index] == root_depth
                    && matches!(tokens[index].kind, Token::And | Token::Or)
                    && tokens[index].line_breaks_before > 0
            })
        })
}

fn boolean_root_depth(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    parens: &HashMap<usize, usize>,
    range: ExpressionRange,
    queries: &[QueryBlock],
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
        .filter(|index| {
            !queries.iter().any(|query| {
                range.start <= query.select
                    && query.end <= range.end
                    && query.select <= *index
                    && *index < query.end
            })
        })
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
        if let Some(introducer) = range.join_on {
            let layout = LayoutGroup {
                compact_line_width: plan.line_width_through(
                    tokens,
                    introducer,
                    range.end,
                    range.root_indent.saturating_sub(1),
                    options,
                ),
                structurally_complex: false,
                hard_boundary: false,
                force_expand: range.expanded,
                compact_overflow_is_unavoidable: false,
            }
            .decide(options);
            if layout == GroupLayout::Compact {
                continue;
            }
        }
        let root_has_and = range.root_depth.is_some_and(|root_depth| {
            (range.start..range.end)
                .any(|index| depths[index] == root_depth && tokens[index].kind == Token::And)
        });
        let mut root_indent = range.introducer.map_or_else(
            || {
                plan.line_indent_for(
                    range.start,
                    plan.relative_indent(range.start, range.root_indent),
                )
                .max(plan.relative_indent(range.start, range.root_indent))
            },
            |introducer| plan.line_indent_for(introducer, range.root_indent.saturating_sub(1)) + 1,
        );
        let joined_opener = options.inline_predicate_group_opener
            && range.introducer.is_some_and(|introducer| {
                let last = (range.start..range.end).rfind(|index| !tokens[*index].is_comment());
                tokens[range.start].kind == Token::Ascii40
                    && parens.get(&range.start).copied() == last
                    && tokens[range.start].line_breaks_before <= 1
                    && plan
                        .before
                        .get(&range.start)
                        .is_none_or(|line| line.lines <= 1)
                    && plan.line_width_through(
                        tokens,
                        introducer,
                        range.start + 1,
                        root_indent.saturating_sub(1),
                        options,
                    ) <= options.soft_line_width
            });
        if joined_opener {
            plan.before.remove(&range.start);
            root_indent = root_indent.saturating_sub(1);
        }

        if !joined_opener
            && !matches!(
                range.kind,
                ExpressionOwnerKind::AssignmentValue
                    | ExpressionOwnerKind::CaseCondition
                    | ExpressionOwnerKind::CaseResult
            )
        {
            plan.break_before(
                range.start,
                tokens[range.start].line_breaks_before.clamp(1, 2),
                root_indent,
            );
        }
        for index in range.start..range.end {
            if range.root_depth == Some(depths[index])
                && matches!(tokens[index].kind, Token::And | Token::Or)
            {
                if range.preserve_authored_breaks && tokens[index].line_breaks_before == 0 {
                    continue;
                }
                let indent = root_indent + depths[index].saturating_sub(range.base_depth);
                let mut trivia_start = index;
                while trivia_start > range.start
                    && tokens[trivia_start - 1].is_comment()
                    && tokens[trivia_start].line_breaks_before == 1
                {
                    trivia_start -= 1;
                }
                if trivia_start < index {
                    plan.set_indent(trivia_start..index, indent);
                    plan.break_before(trivia_start, 1, indent);
                }
                plan.break_before(index, tokens[index].line_breaks_before.clamp(1, 2), indent);
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
                        plan.rebase_indents(query_start..close, current_indent, query_indent);
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
                    depths[candidate] >= inner_depth
                        && matches!(tokens[candidate].kind, Token::Select | Token::With)
                        && plan.before.contains_key(&candidate)
                });
                let authored_boundary = tokens[index + 1..close]
                    .iter()
                    .any(|token| token.is_comment() || token.line_breaks_before > 1);
                let over_soft = root_indent * INDENT_WIDTH
                    + compact_width(tokens, index, close + 1, options)
                    > options.soft_line_width;
                let owns_complete_range = index == range.start && close + 1 == range.end;
                let wraps_boolean_root = range.root_depth.is_some_and(|depth| depth >= inner_depth)
                    && tokens[range.start..index]
                        .iter()
                        .all(|token| token.is_comment() || token.kind == Token::Ascii40)
                    && tokens[close + 1..range.end]
                        .iter()
                        .all(|token| token.is_comment() || token.kind == Token::Ascii41);
                if (contains_boolean || wraps_boolean_root || contains_nested_sql)
                    && (owns_complete_range
                        || wraps_boolean_root
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
                        if range.preserve_authored_breaks
                            && range.root_depth == Some(depths[connector])
                            && tokens[connector].line_breaks_before == 0
                        {
                            continue;
                        }
                        plan.break_before(
                            connector,
                            tokens[connector].line_breaks_before.clamp(1, 2),
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

fn plan_case_result_boundaries(
    tokens: &[SqlToken<'_>],
    expressions: &[ExpressionRange],
    options: &FormatOptions,
    plan: &mut LayoutPlan,
) {
    for result in expressions
        .iter()
        .filter(|range| range.kind == ExpressionOwnerKind::CaseResult)
    {
        let Some((&line_start, line_break)) = plan.before.range(..=result.start).next_back() else {
            continue;
        };
        if line_start == result.start {
            continue;
        }
        let indent = line_break.indent;
        let line_end = plan
            .before
            .keys()
            .copied()
            .filter(|index| result.start < *index && *index < result.end)
            .min()
            .unwrap_or(result.end);
        if plan.line_width_through(tokens, result.start, line_end, indent, options)
            > options.hard_line_width
        {
            plan.break_before(result.start, 1, indent + 1);
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

struct Writer {
    output: String,
    pending_layout: String,
}

impl Writer {
    fn new() -> Self {
        Self {
            output: String::new(),
            pending_layout: String::new(),
        }
    }

    fn at_line_start(&self) -> bool {
        self.output.is_empty()
            || self.output.ends_with('\n')
            || self.pending_layout.contains('\n')
            || self.pending_layout.ends_with(' ')
    }

    fn write(&mut self, text: &str) {
        self.output.push_str(&self.pending_layout);
        self.pending_layout.clear();
        self.output.push_str(text);
    }

    fn space(&mut self) {
        if !self.at_line_start() && !self.pending_layout.ends_with(' ') {
            self.pending_layout.push(' ');
        }
    }

    fn newline(&mut self, lines: usize, indent: usize) {
        while self.pending_layout.ends_with(' ') {
            self.pending_layout.pop();
        }
        if self.output.is_empty() {
            return;
        }

        let pending_newlines = self
            .pending_layout
            .as_bytes()
            .iter()
            .rev()
            .take_while(|byte| **byte == b'\n')
            .count();
        let existing = if pending_newlines == 0 {
            self.output
                .as_bytes()
                .iter()
                .rev()
                .take_while(|byte| **byte == b'\n')
                .count()
        } else {
            pending_newlines
        };
        self.pending_layout
            .extend(std::iter::repeat_n('\n', lines.saturating_sub(existing)));
        self.pending_layout
            .extend(std::iter::repeat_n(' ', indent * INDENT_WIDTH));
    }

    fn finish(mut self, trailing_newline: bool) -> String {
        self.pending_layout.clear();
        if trailing_newline && !self.output.is_empty() && !self.output.ends_with('\n') {
            self.output.push('\n');
        }
        self.output
    }
}

#[cfg(test)]
mod hard_width_tests {
    use super::*;

    #[test]
    fn physical_line_width_excludes_lf_and_crlf_terminators() {
        let options = FormatOptions {
            soft_line_width: 9,
            hard_line_width: 9,
            ..FormatOptions::default()
        };
        assert!(
            validate_hard_width("SELECT 1;\n", &options)
                .unwrap()
                .is_empty()
        );
        assert!(
            validate_hard_width("SELECT 1;\r\n", &options)
                .unwrap()
                .is_empty()
        );
    }

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

#[cfg(test)]
mod writer_tests {
    use super::Writer;

    #[test]
    fn layout_operations_never_trim_emitted_token_bytes() {
        let mut writer = Writer::new();
        writer.write("token ");
        writer.newline(1, 0);
        writer.write("next");
        assert_eq!(writer.finish(false), "token \nnext");

        let mut writer = Writer::new();
        writer.write("token");
        writer.space();
        writer.newline(1, 0);
        writer.write("next");
        assert_eq!(writer.finish(false), "token\nnext");

        let mut writer = Writer::new();
        writer.write("token ");
        assert_eq!(writer.finish(false), "token ");

        let mut writer = Writer::new();
        writer.write("token\n");
        assert_eq!(writer.finish(false), "token\n");
    }
}
