use super::{FormatOptions, SqlToken};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum GroupLayout {
    Compact,
    Expanded,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct LayoutGroup {
    pub compact_line_width: usize,
    pub structurally_complex: bool,
    pub hard_boundary: bool,
    pub force_expand: bool,
    pub compact_overflow_is_unavoidable: bool,
}

impl LayoutGroup {
    pub fn decide(self, options: &FormatOptions) -> GroupLayout {
        if self.hard_boundary
            || self.structurally_complex
            || self.force_expand
            || (!self.compact_overflow_is_unavoidable
                && self.compact_line_width > options.soft_line_width)
        {
            GroupLayout::Expanded
        } else {
            GroupLayout::Compact
        }
    }
}

pub(super) fn has_hard_boundary(tokens: &[SqlToken<'_>], start: usize, end: usize) -> bool {
    tokens[start..end]
        .iter()
        .any(|token| token.is_comment() || token.line_breaks_before > 1)
}

pub(super) fn has_list_hard_boundary(tokens: &[SqlToken<'_>], start: usize, end: usize) -> bool {
    tokens[start..end].iter().any(|token| {
        token.line_breaks_before > 1 || (token.is_comment() && token.line_breaks_before > 0)
    })
}
