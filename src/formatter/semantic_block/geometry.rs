use std::collections::BTreeMap;

use super::{FormatOptions, INDENT_WIDTH, SqlToken, needs_space, render_token};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Break {
    pub(super) lines: usize,
    pub(super) indent: usize,
}

#[derive(Debug)]
pub(super) struct LayoutPlan {
    pub(super) before: BTreeMap<usize, Break>,
    pub(super) token_indents: Vec<Option<usize>>,
    indent_offsets: Vec<isize>,
}

impl LayoutPlan {
    pub(super) fn new(token_count: usize) -> Self {
        Self {
            before: BTreeMap::new(),
            token_indents: vec![None; token_count],
            indent_offsets: vec![0; token_count],
        }
    }

    pub(super) fn break_before(&mut self, index: usize, lines: usize, indent: usize) {
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

    pub(super) fn set_indent(&mut self, range: std::ops::Range<usize>, indent: usize) {
        for slot in &mut self.token_indents[range] {
            *slot = Some(indent);
        }
    }

    pub(super) fn set_fallback_indent(
        &mut self,
        range: std::ops::Range<usize>,
        depths: &[usize],
        base_depth: usize,
        indent: usize,
    ) {
        for index in range {
            self.token_indents[index]
                .get_or_insert(indent + depths[index].saturating_sub(base_depth));
        }
    }

    pub(super) fn indent_for(&self, index: usize, fallback: usize) -> usize {
        self.token_indents[index].unwrap_or(fallback)
    }

    pub(super) fn line_indent_for(&self, index: usize, fallback: usize) -> usize {
        self.before
            .get(&index)
            .map(|line_break| line_break.indent)
            .unwrap_or_else(|| self.indent_for(index, fallback))
    }

    pub(super) fn relative_indent(&self, index: usize, base: usize) -> usize {
        base.saturating_add_signed(self.indent_offsets[index])
    }

    pub(super) fn line_start(&self, index: usize) -> usize {
        self.before
            .range(..=index)
            .next_back()
            .map_or(0, |(&start, _)| start)
    }

    pub(super) fn line_width_through(
        &self,
        tokens: &[SqlToken<'_>],
        index: usize,
        end: usize,
        fallback: usize,
        options: &FormatOptions,
    ) -> usize {
        let start = self.line_start(index);
        self.line_indent_for(start, fallback) * INDENT_WIDTH
            + compact_width(tokens, start, end, options)
    }

    pub(super) fn rebase_indents(
        &mut self,
        range: std::ops::Range<usize>,
        current: usize,
        desired: usize,
    ) {
        if current == desired {
            return;
        }
        let adjust = |indent: usize| {
            if desired > current {
                indent + desired - current
            } else {
                indent.saturating_sub(current - desired)
            }
        };
        for (index, line_break) in &mut self.before {
            if range.contains(index) {
                line_break.indent = adjust(line_break.indent);
            }
        }
        for indent in self.token_indents[range.clone()].iter_mut().flatten() {
            *indent = adjust(*indent);
        }
        for offset in &mut self.indent_offsets[range] {
            *offset += desired as isize - current as isize;
        }
    }
}

pub(super) fn compact_width(
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outward_then_inward_rebasing_restores_fallback_coordinates() {
        let mut plan = LayoutPlan::new(3);
        plan.set_indent(1..3, 4);
        plan.break_before(1, 1, 4);
        plan.rebase_indents(1..3, 4, 2);
        assert_eq!(plan.relative_indent(1, 4), 2);
        plan.rebase_indents(1..3, 2, 4);
        assert_eq!(plan.line_indent_for(1, 4), 4);
        assert_eq!(plan.indent_for(2, 4), 4);
        assert_eq!(plan.indent_offsets[1], 0);
        assert_eq!(plan.indent_offsets[0], 0);
    }

    #[test]
    fn nested_rebasing_matches_direct_coordinates_without_touching_siblings() {
        let mut nested = LayoutPlan::new(5);
        nested.set_indent(0..5, 6);
        nested.rebase_indents(1..5, 6, 4);
        nested.rebase_indents(2..4, 4, 3);
        let mut direct = LayoutPlan::new(5);
        direct.set_indent(0..5, 6);
        direct.rebase_indents(2..4, 6, 3);
        assert_eq!(nested.indent_for(2, 6), direct.indent_for(2, 6));
        assert_eq!(nested.relative_indent(2, 6), direct.relative_indent(2, 6));
        assert_eq!(nested.relative_indent(0, 6), 6);
        assert_eq!(nested.relative_indent(4, 6), 4);
    }

    #[test]
    fn planned_line_width_includes_the_owner_prefix_and_display_indent() {
        let tokens = super::super::tokenize("SELECT alpha WHERE (beta = 1)").unwrap();
        let options = FormatOptions::default();
        let where_index = tokens
            .iter()
            .position(|token| token.kind == super::super::Token::Where)
            .unwrap();
        let opener = where_index + 1;
        let mut plan = LayoutPlan::new(tokens.len());
        // Insert in reverse order: predecessor lookup must not depend on insertion order.
        plan.break_before(where_index, 1, 2);
        plan.break_before(0, 1, 0);
        assert_eq!(plan.line_start(opener), where_index);
        assert_eq!(
            plan.line_width_through(&tokens, opener, opener + 1, 0, &options),
            15
        );
        assert_eq!(plan.line_start(1), 0);
    }
}
