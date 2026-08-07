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

#[cfg(test)]
mod tests {
    use super::*;

    fn group(width: usize) -> LayoutGroup {
        LayoutGroup {
            compact_line_width: width,
            structurally_complex: false,
            hard_boundary: false,
            force_expand: false,
            compact_overflow_is_unavoidable: false,
        }
    }

    #[test]
    fn compact_width_is_inclusive_at_the_soft_limit() {
        let options = FormatOptions {
            soft_line_width: 40,
            hard_line_width: 80,
            ..FormatOptions::default()
        };

        assert_eq!(group(39).decide(&options), GroupLayout::Compact);
        assert_eq!(group(40).decide(&options), GroupLayout::Compact);
        assert_eq!(group(41).decide(&options), GroupLayout::Expanded);
    }

    #[test]
    fn semantic_and_authored_boundaries_override_width() {
        let options = FormatOptions::default();

        assert_eq!(
            LayoutGroup {
                structurally_complex: true,
                ..group(10)
            }
            .decide(&options),
            GroupLayout::Expanded,
        );
        assert_eq!(
            LayoutGroup {
                hard_boundary: true,
                ..group(10)
            }
            .decide(&options),
            GroupLayout::Expanded,
        );
        assert_eq!(
            LayoutGroup {
                force_expand: true,
                ..group(10)
            }
            .decide(&options),
            GroupLayout::Expanded,
        );
    }

    #[test]
    fn indivisible_overflow_does_not_invent_an_unsafe_break() {
        let options = FormatOptions {
            soft_line_width: 20,
            hard_line_width: 40,
            ..FormatOptions::default()
        };

        assert_eq!(
            LayoutGroup {
                compact_overflow_is_unavoidable: true,
                ..group(100)
            }
            .decide(&options),
            GroupLayout::Compact,
        );
    }
}
