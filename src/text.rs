//! Immutable text coordinates shared by formatter diagnostics and adapters.
//!
//! Construct a separate index for each input, normalized, or formatted source
//! frame. AST depth and display indentation are unrelated to these coordinates.

/// Half-open UTF-8 byte range in its explicitly associated source frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceRange {
    pub start: usize,
    pub end: usize,
}

impl SourceRange {
    pub const fn new(start: usize, end: usize) -> Self {
        Self { start, end }
    }

    pub const fn shifted(self, offset: usize) -> Self {
        Self {
            start: self.start + offset,
            end: self.end + offset,
        }
    }
}

/// One-based physical line and Unicode-scalar column in a particular text frame.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceLocation {
    pub line: usize,
    pub column: usize,
}

/// UTF-8 byte offsets indexed once for an immutable source frame.
#[derive(Debug)]
pub struct SourceIndex<'a> {
    source: &'a str,
    line_starts: Vec<usize>,
}

impl<'a> SourceIndex<'a> {
    pub fn new(source: &'a str) -> Self {
        let line_starts = std::iter::once(0)
            .chain(
                source
                    .bytes()
                    .enumerate()
                    .filter_map(|(index, byte)| (byte == b'\n').then_some(index + 1)),
            )
            .collect();
        Self {
            source,
            line_starts,
        }
    }

    /// Locate a byte offset, clamping to EOF and the preceding UTF-8 boundary.
    pub fn location(&self, byte_offset: usize) -> SourceLocation {
        let mut offset = byte_offset.min(self.source.len());
        while !self.source.is_char_boundary(offset) {
            offset -= 1;
        }
        let index = self.line_starts.partition_point(|start| *start <= offset) - 1;
        SourceLocation {
            line: index + 1,
            column: self.source[self.line_starts[index]..offset].chars().count() + 1,
        }
    }

    /// Number of completed LF or CRLF physical lines, including a final newline.
    pub fn completed_line_count(&self) -> usize {
        self.line_starts.len() - 1
    }

    /// Number of physical line spans; a final newline does not add an empty span.
    pub fn line_count(&self) -> usize {
        self.line_starts.len() - usize::from(self.line_starts.last() == Some(&self.source.len()))
    }

    /// Half-open byte span of a one-based line, including its line terminator.
    pub fn line_span(&self, line: usize) -> Option<SourceRange> {
        let index = line.checked_sub(1)?;
        if index >= self.line_count() {
            return None;
        }
        Some(SourceRange::new(
            self.line_starts[index],
            self.line_starts
                .get(index + 1)
                .copied()
                .unwrap_or(self.source.len()),
        ))
    }

    /// Half-open content range of a one-based line, excluding LF/CRLF terminators.
    pub fn line_range(&self, line: usize) -> Option<SourceRange> {
        let mut range = self.line_span(line)?;
        if self.source.as_bytes()[range.end - 1] == b'\n' {
            range.end -= 1;
            if range.end > range.start && self.source.as_bytes()[range.end - 1] == b'\r' {
                range.end -= 1;
            }
        }
        Some(range)
    }

    /// Width in Unicode scalars, using the same physical content range as diagnostics.
    pub fn line_width(&self, line: usize) -> Option<usize> {
        let range = self.line_range(line)?;
        Some(self.source[range.start..range.end].chars().count())
    }

    /// One-based physical lines with their half-open content byte ranges.
    pub fn lines(&self) -> impl ExactSizeIterator<Item = (usize, SourceRange)> + '_ {
        (0..self.line_count()).map(|index| {
            let line = index + 1;
            (line, self.line_range(line).expect("indexed physical line"))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_columns_and_crlf_ranges_share_the_same_frame() {
        let source = "я🙂\r\n\r\nx";
        let index = SourceIndex::new(source);
        assert_eq!(index.location(3), SourceLocation { line: 1, column: 2 });
        assert_eq!(index.location(6), SourceLocation { line: 1, column: 3 });
        assert_eq!(index.location(8), SourceLocation { line: 2, column: 1 });
        assert_eq!(
            index.location(usize::MAX),
            SourceLocation { line: 3, column: 2 }
        );
        assert_eq!(index.line_range(1), Some(SourceRange::new(0, 6)));
        assert_eq!(index.line_span(1), Some(SourceRange::new(0, 8)));
        assert_eq!(index.line_range(2), Some(SourceRange::new(8, 8)));
        assert_eq!(index.line_width(1), Some(2));
        assert_eq!(index.completed_line_count(), 2);
        assert_eq!(index.lines().len(), 3);
    }

    #[test]
    fn empty_and_terminal_newline_coordinates_are_explicit() {
        let empty = SourceIndex::new("");
        assert_eq!(empty.line_count(), 0);
        assert_eq!(empty.location(0), SourceLocation { line: 1, column: 1 });
        assert_eq!(empty.line_range(0), None);
        assert_eq!(empty.line_range(1), None);
        let terminated = SourceIndex::new("a\n");
        assert_eq!(terminated.line_count(), 1);
        assert_eq!(
            terminated.location(2),
            SourceLocation { line: 2, column: 1 }
        );
        assert_eq!(terminated.line_range(2), None);
    }

    #[test]
    fn input_and_output_indices_do_not_share_line_offsets() {
        let input = SourceIndex::new("SELECT 1; SELECT 2;");
        let output = SourceIndex::new("SELECT 1;\nSELECT 2;");
        assert_eq!(input.location(10).line, 1);
        assert_eq!(output.location(10).line, 2);
        assert_eq!(input.completed_line_count(), 0);
        assert_eq!(output.completed_line_count(), 1);
    }
}
