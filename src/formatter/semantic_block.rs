use std::collections::{HashMap, HashSet};

use pg_query::protobuf::{KeywordKind, Token};

use super::ownership::{StatementKind, SupportedDocument, TokenStatement, bind_token_statements};
use super::tokens::{SqlToken, tokenize};
use super::{
    FormatDiagnostic, FormatOptions, FormatWarning, INDENT_WIDTH, NotEqualPolicy, SemicolonPolicy,
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

#[derive(Debug, Clone, Copy)]
struct OnConflictBlock {
    start: usize,
    target_open: Option<usize>,
    action: usize,
    update: bool,
    set: Option<usize>,
    action_where: Option<usize>,
}

#[derive(Debug, Clone)]
struct InsertBlock {
    start: usize,
    end: usize,
    base_depth: usize,
    target_open: Option<usize>,
    values: usize,
    rows: Vec<(usize, usize)>,
    on_conflict: Option<OnConflictBlock>,
    returning: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
struct UpdateBlock {
    start: usize,
    end: usize,
    base_depth: usize,
    set: usize,
    from: Option<usize>,
    where_clause: Option<usize>,
    returning: Option<usize>,
}

#[derive(Debug, Clone, Copy)]
struct DeleteBlock {
    start: usize,
    end: usize,
    base_depth: usize,
    using: Option<usize>,
    where_clause: Option<usize>,
    returning: Option<usize>,
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

    let depths = token_depths(&tokens);
    let statements = bind_token_statements(document, &tokens, &depths)?;
    let parens = parenthesis_pairs(&tokens);
    let cases = case_ranges(&tokens, options);
    let inserts = insert_blocks(&tokens, &depths, &parens, &statements)?;
    let updates = update_blocks(&tokens, &depths, &statements)?;
    let deletes = delete_blocks(&tokens, &depths, &statements)?;
    let parenthesized_lists =
        parenthesized_lists(&tokens, &depths, &parens, &cases, &inserts, options);
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
    plan_insert_statements(
        &tokens,
        &depths,
        &cases,
        &parenthesized_lists,
        &inserts,
        options,
        &mut plan,
    );
    plan_update_statements(
        &tokens,
        &depths,
        &cases,
        &parenthesized_lists,
        &boolean_ranges,
        &updates,
        options,
        &mut plan,
    );
    plan_delete_statements(
        &tokens,
        &depths,
        &cases,
        &parenthesized_lists,
        &boolean_ranges,
        &deletes,
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

fn parenthesized_lists(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    parens: &HashMap<usize, usize>,
    cases: &[CaseRange],
    inserts: &[InsertBlock],
    options: &FormatOptions,
) -> Vec<ParenthesizedList> {
    let mut lists = Vec::new();

    for (open, token) in tokens.iter().enumerate() {
        if token.kind != Token::Ascii40
            || !(is_function_call_open(tokens, open) || is_insert_list_open(inserts, open))
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
            .map_or_else(|| open.saturating_sub(1), |insert| insert.start);
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

fn insert_blocks(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    parens: &HashMap<usize, usize>,
    statements: &[TokenStatement],
) -> Result<Vec<InsertBlock>, FormatDiagnostic> {
    let mut blocks = Vec::new();

    for statement in statements
        .iter()
        .filter(|statement| statement.kind == StatementKind::Insert)
    {
        let start = statement.start;
        if tokens[start].kind != Token::Insert {
            return Err(FormatDiagnostic::Ownership(format!(
                "INSERT statement starts with token {:?}",
                tokens[start].kind
            )));
        }
        let base_depth = statement.base_depth;
        let end = statement.end;
        let values = (start + 1..end)
            .find(|&index| depths[index] == base_depth && tokens[index].kind == Token::Values)
            .ok_or_else(|| {
                FormatDiagnostic::Ownership("supported INSERT has no VALUES clause".into())
            })?;
        let returning = (values + 1..end)
            .find(|&index| depths[index] == base_depth && tokens[index].kind == Token::Returning);
        let conflict_start = (values + 1..returning.unwrap_or(end)).find(|&index| {
            depths[index] == base_depth
                && tokens[index].kind == Token::On
                && tokens
                    .get(index + 1)
                    .is_some_and(|next| next.kind == Token::Conflict)
        });
        let on_conflict = conflict_start
            .map(|conflict| {
                let boundary = returning.unwrap_or(end);
                let action = (conflict + 2..boundary)
                    .find(|&index| depths[index] == base_depth && tokens[index].kind == Token::Do)
                    .ok_or_else(|| {
                        FormatDiagnostic::Ownership("ON CONFLICT has no DO action".into())
                    })?;
                let target_open = (conflict + 2..action).find(|&index| {
                    depths[index] == base_depth
                        && tokens[index].kind == Token::Ascii40
                        && parens.get(&index).is_some_and(|close| *close < action)
                });
                let update = tokens
                    .get(action + 1)
                    .is_some_and(|token| token.kind == Token::Update);
                let set = if update {
                    Some(
                        (action + 1..boundary)
                            .find(|&index| {
                                depths[index] == base_depth && tokens[index].kind == Token::Set
                            })
                            .ok_or_else(|| {
                                FormatDiagnostic::Ownership(
                                    "ON CONFLICT DO UPDATE has no SET clause".into(),
                                )
                            })?,
                    )
                } else {
                    None
                };
                let action_where = set.and_then(|set| {
                    (set + 1..boundary).find(|&index| {
                        depths[index] == base_depth && tokens[index].kind == Token::Where
                    })
                });
                Ok(OnConflictBlock {
                    start: conflict,
                    target_open,
                    action,
                    update,
                    set,
                    action_where,
                })
            })
            .transpose()?;
        let rows_end = conflict_start.or(returning).unwrap_or(end);
        let target_open = (start + 1..values).find(|&index| {
            depths[index] == base_depth
                && tokens[index].kind == Token::Ascii40
                && parens.get(&index).is_some_and(|close| *close < values)
        });
        let rows = (values + 1..rows_end)
            .filter(|&index| {
                depths[index] == base_depth
                    && tokens[index].kind == Token::Ascii40
                    && parens.get(&index).is_some_and(|close| *close < rows_end)
            })
            .filter_map(|open| parens.get(&open).copied().map(|close| (open, close)))
            .collect();

        blocks.push(InsertBlock {
            start,
            end,
            base_depth,
            target_open,
            values,
            rows,
            on_conflict,
            returning,
        });
    }

    Ok(blocks)
}

fn update_blocks(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    statements: &[TokenStatement],
) -> Result<Vec<UpdateBlock>, FormatDiagnostic> {
    let mut blocks = Vec::new();

    for statement in statements
        .iter()
        .filter(|statement| statement.kind == StatementKind::Update)
    {
        let start = statement.start;
        if tokens[start].kind != Token::Update {
            return Err(FormatDiagnostic::Ownership(format!(
                "UPDATE statement starts with token {:?}",
                tokens[start].kind
            )));
        }
        let base_depth = statement.base_depth;
        let end = statement.end;
        let set = (start + 1..end)
            .find(|&index| depths[index] == base_depth && tokens[index].kind == Token::Set)
            .ok_or_else(|| {
                FormatDiagnostic::Ownership("supported UPDATE has no SET clause".into())
            })?;
        let from = (set + 1..end)
            .find(|&index| depths[index] == base_depth && tokens[index].kind == Token::From);
        let where_clause = (set + 1..end)
            .find(|&index| depths[index] == base_depth && tokens[index].kind == Token::Where);
        let returning = (set + 1..end)
            .find(|&index| depths[index] == base_depth && tokens[index].kind == Token::Returning);
        blocks.push(UpdateBlock {
            start,
            end,
            base_depth,
            set,
            from,
            where_clause,
            returning,
        });
    }

    Ok(blocks)
}

#[allow(clippy::too_many_arguments)]
fn plan_update_statements(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    cases: &[CaseRange],
    lists: &[ParenthesizedList],
    boolean_ranges: &[BooleanRange],
    updates: &[UpdateBlock],
    options: &FormatOptions,
    plan: &mut LayoutPlan,
) {
    for update in updates {
        let authored = tokens[update.start + 1..update.end]
            .iter()
            .any(|token| token.line_breaks_before > 0);
        let compact_statement_width = update.base_depth * INDENT_WIDTH
            + compact_width(tokens, update.start, update.end, options);
        let width_driven = compact_statement_width > options.soft_line_width;
        let has_expanded_predicate = update.where_clause.is_some_and(|where_clause| {
            boolean_ranges
                .iter()
                .any(|range| range.start == where_clause + 1 && range.end <= update.end)
        });
        let expanded = authored || update.from.is_some() || width_driven || has_expanded_predicate;
        if !expanded {
            continue;
        }

        plan.break_before(update.set, 1, update.base_depth);
        let set_end = update
            .from
            .or(update.where_clause)
            .or(update.returning)
            .unwrap_or(update.end);
        plan_keyword_list(
            tokens,
            depths,
            cases,
            lists,
            update.set,
            set_end,
            update.base_depth,
            true,
            options,
            plan,
        );

        if let Some(from) = update.from {
            plan.break_before(from, 1, update.base_depth);
        }
        if let Some(where_clause) = update.where_clause {
            plan.break_before(where_clause, 1, update.base_depth);
        }
        if let Some(returning) = update.returning {
            plan.break_before(returning, 1, update.base_depth);
            plan_keyword_list(
                tokens,
                depths,
                cases,
                lists,
                returning,
                update.end,
                update.base_depth,
                width_driven,
                options,
                plan,
            );
        }
    }
}

fn delete_blocks(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    statements: &[TokenStatement],
) -> Result<Vec<DeleteBlock>, FormatDiagnostic> {
    let mut blocks = Vec::new();

    for statement in statements
        .iter()
        .filter(|statement| statement.kind == StatementKind::Delete)
    {
        let start = statement.start;
        if tokens[start].kind != Token::DeleteP {
            return Err(FormatDiagnostic::Ownership(format!(
                "DELETE statement starts with token {:?}",
                tokens[start].kind
            )));
        }
        let base_depth = statement.base_depth;
        let end = statement.end;
        let using = (start + 1..end)
            .find(|&index| depths[index] == base_depth && tokens[index].kind == Token::Using);
        let where_clause = (start + 1..end)
            .find(|&index| depths[index] == base_depth && tokens[index].kind == Token::Where);
        let returning = (start + 1..end)
            .find(|&index| depths[index] == base_depth && tokens[index].kind == Token::Returning);
        blocks.push(DeleteBlock {
            start,
            end,
            base_depth,
            using,
            where_clause,
            returning,
        });
    }

    Ok(blocks)
}

#[allow(clippy::too_many_arguments)]
fn plan_delete_statements(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    cases: &[CaseRange],
    lists: &[ParenthesizedList],
    boolean_ranges: &[BooleanRange],
    deletes: &[DeleteBlock],
    options: &FormatOptions,
    plan: &mut LayoutPlan,
) {
    for delete in deletes {
        let authored = tokens[delete.start + 1..delete.end]
            .iter()
            .any(|token| token.line_breaks_before > 0);
        let compact_statement_width = delete.base_depth * INDENT_WIDTH
            + compact_width(tokens, delete.start, delete.end, options);
        let width_driven = compact_statement_width > options.soft_line_width;
        let has_expanded_predicate = delete.where_clause.is_some_and(|where_clause| {
            boolean_ranges
                .iter()
                .any(|range| range.start == where_clause + 1 && range.end <= delete.end)
        });
        let expanded = authored || delete.using.is_some() || width_driven || has_expanded_predicate;
        if !expanded {
            continue;
        }

        if let Some(using) = delete.using {
            plan.break_before(using, 1, delete.base_depth);
        }
        if let Some(where_clause) = delete.where_clause {
            plan.break_before(where_clause, 1, delete.base_depth);
        }
        if let Some(returning) = delete.returning {
            plan.break_before(returning, 1, delete.base_depth);
            plan_keyword_list(
                tokens,
                depths,
                cases,
                lists,
                returning,
                delete.end,
                delete.base_depth,
                width_driven,
                options,
                plan,
            );
        }
    }
}

fn is_insert_list_open(inserts: &[InsertBlock], open: usize) -> bool {
    inserts.iter().any(|insert| {
        insert.target_open == Some(open)
            || insert.rows.iter().any(|&(row, _)| row == open)
            || insert
                .on_conflict
                .is_some_and(|conflict| conflict.target_open == Some(open))
    })
}

fn plan_insert_statements(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    cases: &[CaseRange],
    lists: &[ParenthesizedList],
    inserts: &[InsertBlock],
    options: &FormatOptions,
    plan: &mut LayoutPlan,
) {
    for insert in inserts {
        let authored = tokens[insert.start + 1..insert.end]
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
        let compact_statement_width = insert.base_depth * INDENT_WIDTH
            + compact_width(tokens, insert.start, insert.end, options);
        let width_driven = compact_statement_width > options.soft_line_width;
        let has_update = insert.on_conflict.is_some_and(|conflict| conflict.update);
        let expanded = authored || has_expanded_list || width_driven || has_update;
        if !expanded {
            continue;
        }

        plan.break_before(insert.values, 1, insert.base_depth);

        let rows_are_multiline = insert.rows.len() > 1
            || insert
                .rows
                .first()
                .is_some_and(|(open, _)| tokens[*open].line_breaks_before > 0);
        if rows_are_multiline {
            for &(open, close) in &insert.rows {
                plan.set_indent(open..close + 1, insert.base_depth + 1);
                plan.break_before(open, 1, insert.base_depth + 1);
            }
        }

        if let Some(conflict) = insert.on_conflict {
            plan.break_before(conflict.start, 1, insert.base_depth);
            if conflict.update {
                plan.break_before(conflict.action, 1, insert.base_depth);
                if let Some(set) = conflict.set {
                    plan.break_before(set, 1, insert.base_depth);
                    let set_end = conflict
                        .action_where
                        .or(insert.returning)
                        .unwrap_or(insert.end);
                    plan_keyword_list(
                        tokens,
                        depths,
                        cases,
                        lists,
                        set,
                        set_end,
                        insert.base_depth,
                        true,
                        options,
                        plan,
                    );
                }
                if let Some(action_where) = conflict.action_where {
                    plan.break_before(action_where, 1, insert.base_depth);
                }
            }
        }

        if let Some(returning) = insert.returning {
            plan.break_before(returning, 1, insert.base_depth);
            plan_keyword_list(
                tokens,
                depths,
                cases,
                lists,
                returning,
                insert.end,
                insert.base_depth,
                width_driven,
                options,
                plan,
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn plan_keyword_list(
    tokens: &[SqlToken<'_>],
    depths: &[usize],
    cases: &[CaseRange],
    lists: &[ParenthesizedList],
    keyword: usize,
    end: usize,
    base_depth: usize,
    force_expand: bool,
    options: &FormatOptions,
    plan: &mut LayoutPlan,
) {
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
            base_depth * INDENT_WIDTH + compact_width(tokens, select, end, options);
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
                                Token::Ascii59
                                    | Token::Do
                                    | Token::Union
                                    | Token::Using
                                    | Token::Intersect
                                    | Token::Except
                            )))
            })
            .unwrap_or(tokens.len());
        let has_connector = (index + 1..end).any(|candidate| {
            depths[candidate] >= base_depth
                && matches!(tokens[candidate].kind, Token::And | Token::Or)
        });
        let hides_structure = base_depth * INDENT_WIDTH
            + compact_width(tokens, index, end, options)
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
        width += render_token(tokens, index, options).chars().count();
        if width > options.hard_line_width && is_indivisible(&tokens[index]) {
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

pub(super) fn render_token(
    tokens: &[SqlToken<'_>],
    index: usize,
    options: &FormatOptions,
) -> String {
    let token = &tokens[index];
    let previous = index.checked_sub(1).map(|previous| &tokens[previous]);
    let next = tokens.get(index + 1);

    if token.kind == Token::NotEquals {
        return match options.not_equal_policy {
            NotEqualPolicy::Preserve => token.text.to_owned(),
            NotEqualPolicy::PreferBang => "!=".into(),
        };
    }
    if is_on_conflict_excluded(tokens, index) {
        return token.text.to_uppercase();
    }
    if token.kind == Token::Interval {
        return if next.is_some_and(|next| is_string_literal(next.kind)) {
            token.text.to_uppercase()
        } else {
            token.text.to_lowercase()
        };
    }
    if is_function_call_name(tokens, index) {
        return if is_uppercase_builtin(token.text) {
            token.text.to_uppercase()
        } else {
            token.text.to_lowercase()
        };
    }
    if token.kind == Token::Ident
        && !token.text.starts_with('"')
        && previous.is_some_and(|previous| previous.kind == Token::Typecast)
    {
        return token.text.to_lowercase();
    }
    if is_type_keyword(token.kind) {
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

pub(super) fn is_function_call_name(tokens: &[SqlToken<'_>], index: usize) -> bool {
    !tokens[index].text.starts_with('"') && is_function_call_syntax(tokens, index)
}

pub(super) fn is_function_call_syntax(tokens: &[SqlToken<'_>], index: usize) -> bool {
    tokens
        .get(index + 1)
        .is_some_and(|next| next.kind == Token::Ascii40)
        && matches!(
            tokens[index].kind,
            Token::Ident
                | Token::Coalesce
                | Token::Extract
                | Token::Greatest
                | Token::JsonArray
                | Token::JsonArrayagg
                | Token::JsonExists
                | Token::JsonObject
                | Token::JsonObjectagg
                | Token::JsonQuery
                | Token::JsonScalar
                | Token::JsonSerialize
                | Token::JsonTable
                | Token::JsonValue
                | Token::Least
                | Token::MergeAction
                | Token::Normalize
                | Token::Nullif
                | Token::Overlay
                | Token::Position
                | Token::Substring
                | Token::Trim
                | Token::Xmlattributes
                | Token::Xmlconcat
                | Token::Xmlelement
                | Token::Xmlexists
                | Token::Xmlforest
                | Token::Xmlnamespaces
                | Token::Xmlparse
                | Token::Xmlpi
                | Token::Xmlroot
                | Token::Xmlserialize
                | Token::Xmltable
        )
}

pub(super) fn is_compact_grammar_parenthesis(tokens: &[SqlToken<'_>], index: usize) -> bool {
    tokens
        .get(index + 1)
        .is_some_and(|next| next.kind == Token::Ascii40)
        && matches!(tokens[index].kind, Token::Cast | Token::Treat)
}

pub(super) fn is_type_modifier_syntax(tokens: &[SqlToken<'_>], index: usize) -> bool {
    tokens
        .get(index + 1)
        .is_some_and(|next| next.kind == Token::Ascii40)
        && (tokens[index].kind == Token::Interval || is_type_keyword(tokens[index].kind))
}

pub(super) fn is_uppercase_builtin(name: &str) -> bool {
    matches!(
        name.to_ascii_uppercase().as_str(),
        "COUNT"
            | "SUM"
            | "AVG"
            | "MIN"
            | "MAX"
            | "COALESCE"
            | "NULLIF"
            | "GREATEST"
            | "LEAST"
            | "NOW"
            | "EXTRACT"
    )
}

fn is_on_conflict_excluded(tokens: &[SqlToken<'_>], index: usize) -> bool {
    if tokens[index].kind != Token::Ident
        || !tokens[index].text.eq_ignore_ascii_case("excluded")
        || tokens
            .get(index + 1)
            .is_none_or(|next| next.kind != Token::Ascii46)
    {
        return false;
    }

    let statement_start = tokens[..index]
        .iter()
        .rposition(|token| token.kind == Token::Ascii59)
        .map_or(0, |semicolon| semicolon + 1);
    let statement = &tokens[statement_start..index];
    let Some(conflict) = statement
        .iter()
        .rposition(|token| token.kind == Token::Conflict)
    else {
        return false;
    };
    let Some(action) = statement[conflict + 1..]
        .iter()
        .position(|token| token.kind == Token::Do)
        .map(|offset| conflict + 1 + offset)
    else {
        return false;
    };
    let Some(update) = statement[action + 1..]
        .iter()
        .position(|token| token.kind == Token::Update)
        .map(|offset| action + 1 + offset)
    else {
        return false;
    };
    let Some(set) = statement[update + 1..]
        .iter()
        .position(|token| token.kind == Token::Set)
        .map(|offset| update + 1 + offset)
    else {
        return false;
    };

    !statement[set + 1..]
        .iter()
        .any(|token| token.kind == Token::Returning)
}

fn is_string_literal(kind: Token) -> bool {
    matches!(kind, Token::Sconst | Token::Usconst)
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
            | Token::Conflict
            | Token::Cross
            | Token::CurrentDate
            | Token::CurrentRole
            | Token::CurrentSchema
            | Token::CurrentTime
            | Token::CurrentTimestamp
            | Token::CurrentUser
            | Token::DayP
            | Token::Distinct
            | Token::DeleteP
            | Token::Do
            | Token::Else
            | Token::EndP
            | Token::Except
            | Token::FalseP
            | Token::From
            | Token::Full
            | Token::GroupP
            | Token::Having
            | Token::HourP
            | Token::InnerP
            | Token::Insert
            | Token::Intersect
            | Token::Is
            | Token::Join
            | Token::Left
            | Token::Limit
            | Token::Localtime
            | Token::Localtimestamp
            | Token::MinuteP
            | Token::MonthP
            | Token::Natural
            | Token::Nothing
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
            | Token::SecondP
            | Token::Select
            | Token::Set
            | Token::SessionUser
            | Token::Then
            | Token::TrueP
            | Token::Union
            | Token::Update
            | Token::Values
            | Token::When
            | Token::Where
            | Token::With
            | Token::YearP
    )
}

pub(super) fn is_type_keyword(kind: Token) -> bool {
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

fn is_insert_target_list_open(tokens: &[SqlToken<'_>], open: usize) -> bool {
    if tokens
        .get(open)
        .is_none_or(|token| token.kind != Token::Ascii40)
    {
        return false;
    }
    for token in tokens[..open].iter().rev() {
        match token.kind {
            Token::Ascii59 | Token::Values => return false,
            Token::Insert => return true,
            _ => {}
        }
    }
    false
}

fn needs_space(tokens: &[SqlToken<'_>], previous: Option<usize>, current: usize) -> bool {
    let Some(previous_index) = previous else {
        return false;
    };
    let current_index = current;
    let previous = &tokens[previous_index];
    let current = &tokens[current_index];
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
    if current.kind == Token::Ascii40 && is_insert_target_list_open(tokens, current_index) {
        return true;
    }
    if current.kind == Token::Ascii40
        && (is_function_call_syntax(tokens, previous_index)
            || is_type_modifier_syntax(tokens, previous_index)
            || is_compact_grammar_parenthesis(tokens, previous_index))
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
