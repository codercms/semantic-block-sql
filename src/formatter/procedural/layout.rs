use super::super::result::LeafOutcome;
use super::super::{Diagnostic, FormatDiagnostic, FormatOptions, Severity};
use super::format_leaf;
use super::ir::{BodyNode, BodyNodeKind, RoutineBody};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FormattedBody {
    pub output: String,
    pub diagnostics: Vec<Diagnostic>,
    pub protected_source_ranges: Vec<super::super::SourceRange>,
    pub protected_output_ranges: Vec<super::super::SourceRange>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Frame {
    Begin,
    If,
    Loop,
    Case,
    CaseBranch,
    Exception,
    ExceptionBranch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LayoutLine {
    indent: usize,
    relative_indent: usize,
    text: String,
    blank_before: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum BodyLayout {
    Line(LayoutLine),
    Protected {
        range: super::super::SourceRange,
        indent: usize,
        blank_before: bool,
    },
}

pub(super) fn format(
    body: &RoutineBody<'_>,
    options: &FormatOptions,
) -> Result<FormattedBody, FormatDiagnostic> {
    let mut frames = Vec::new();
    let mut lines = Vec::new();
    let mut diagnostics = Vec::new();
    let mut protected_ranges = Vec::new();
    let mut in_declare = false;

    for node in &body.nodes {
        let separate_exception_handler =
            node.kind == BodyNodeKind::When && frames.last() == Some(&Frame::ExceptionBranch);
        let (indent, push_after) = layout_node(node, &mut frames, &mut in_declare, body.source)?;
        let mut protected = node.kind.is_opaque();
        let text = if protected {
            diagnostics.push(Diagnostic {
                rule_id: "syntax.unsupported".into(),
                severity: match options.unsupported_policy {
                    super::super::UnsupportedPolicy::Skip => Severity::Warning,
                    super::super::UnsupportedPolicy::Error => Severity::Error,
                },
                message: "unsupported PL/pgSQL statement preserved".into(),
                source_range: node.range,
                fix_available: false,
            });
            node.text.to_owned()
        } else if node.kind == BodyNodeKind::Comment {
            node.text.to_owned()
        } else {
            let leaf = format_leaf(
                node.kind,
                node.text,
                options,
                indent,
                node.capability.as_ref(),
            )?;
            diagnostics.extend(
                leaf.diagnostics
                    .into_iter()
                    .map(|diagnostic| diagnostic.shifted(node.range.start)),
            );
            match leaf.outcome {
                LeafOutcome::Formatted(output) => output,
                LeafOutcome::Preserved => {
                    protected = true;
                    String::new()
                }
            }
        };
        if protected {
            let mut range = node.range;
            if let Some(comment) = node.trailing_comment {
                let suffix = &body.source[range.end..];
                range.end += suffix.len() - suffix.trim_start().len() + comment.len();
            }
            // The leaf starts at its first token. Keep its authored line prefix
            // when it occupies a line; inline leaves acquire contextual indentation
            // once, and their source span remains untouched on subsequent passes.
            let prefix = body.source[..range.start].rsplit('\n').next().unwrap_or("");
            let indent = if prefix
                .chars()
                .all(|character| character == ' ' || character == '\t')
            {
                range.start -= prefix.len();
                0
            } else {
                indent * 4
            };
            lines.push(BodyLayout::Protected {
                range,
                indent,
                blank_before: node.blank_before || separate_exception_handler,
            });
            protected_ranges.push(range);
            if let Some(frame) = push_after {
                frames.push(frame);
            }
            continue;
        }
        let mut rendered = text;
        if let Some(comment) = node.trailing_comment {
            rendered.push(' ');
            rendered.push_str(comment);
        }
        let mut first = true;
        for part in rendered.lines() {
            lines.push(BodyLayout::Line(LayoutLine {
                indent,
                relative_indent: part
                    .chars()
                    .take_while(|character| *character == ' ')
                    .count(),
                text: part.trim().to_owned(),
                blank_before: first && (node.blank_before || separate_exception_handler),
            }));
            first = false;
        }
        if let Some(frame) = push_after {
            frames.push(frame);
        }
    }

    if !frames.is_empty() || in_declare {
        return Err(FormatDiagnostic::Ownership(
            "unbalanced PL/pgSQL layout IR".into(),
        ));
    }

    let mut rendered = Vec::new();
    for line in lines {
        let (blank_before, text, protected) = match line {
            BodyLayout::Protected {
                range,
                indent,
                blank_before,
            } => (
                blank_before,
                format!(
                    "{}{}",
                    " ".repeat(indent),
                    &body.source[range.start..range.end]
                ),
                true,
            ),
            BodyLayout::Line(line) => (
                line.blank_before,
                if line.text.is_empty() {
                    String::new()
                } else {
                    format!(
                        "{}{}",
                        " ".repeat(line.indent * 4 + line.relative_indent),
                        line.text
                    )
                },
                false,
            ),
        };
        if blank_before
            && rendered
                .last()
                .is_some_and(|(line, _): &(String, bool)| !line.is_empty())
        {
            rendered.push((String::new(), false));
        }
        rendered.push((text, protected));
    }
    while rendered.first().is_some_and(|(line, _)| line.is_empty()) {
        rendered.remove(0);
    }
    while rendered.last().is_some_and(|(line, _)| line.is_empty()) {
        rendered.pop();
    }
    let mut output = body.newline.to_owned();
    let mut protected_output_ranges = Vec::new();
    for (text, protected) in rendered {
        let start = output.len();
        output.push_str(&text);
        if protected {
            protected_output_ranges.push(super::super::SourceRange::new(start, output.len()));
        }
        output.push_str(body.newline);
    }
    let mut body_options = options.clone();
    body_options.semicolon_policy = super::super::SemicolonPolicy::Preserve;
    // Style diagnostics compare layout before optional alias changes, which can
    // change scanner token kinds and cardinality (varchar -> character varying).
    let style_output = if body_options.type_aliases.is_empty() {
        output.clone()
    } else {
        body_options.type_aliases.clear();
        format(body, &body_options)?.output
    };
    let mut style_diagnostics =
        super::super::diagnostics::style_diagnostics(body.source, &style_output, &body_options)?;
    style_diagnostics.retain(|diagnostic| {
        !protected_ranges.iter().any(|range| {
            range.start <= diagnostic.source_range.start && diagnostic.source_range.end <= range.end
        })
    });
    diagnostics.extend(style_diagnostics);
    Ok(FormattedBody {
        output,
        diagnostics,
        protected_source_ranges: protected_ranges,
        protected_output_ranges,
    })
}

fn layout_node(
    node: &BodyNode<'_>,
    frames: &mut Vec<Frame>,
    in_declare: &mut bool,
    source: &str,
) -> Result<(usize, Option<Frame>), FormatDiagnostic> {
    use BodyNodeKind as K;
    let mut indent = frames.len() + usize::from(*in_declare);
    let mut push = None;
    match node.kind {
        K::Declare => {
            indent = frames.len();
            *in_declare = true;
        }
        K::Begin => {
            indent = frames.len();
            *in_declare = false;
            push = Some(Frame::Begin);
        }
        K::If => push = Some(Frame::If),
        K::Elsif => {
            require_last(frames, Frame::If, source)?;
            indent = frames.len().saturating_sub(1);
        }
        K::Else => {
            if frames.last() == Some(&Frame::CaseBranch) {
                frames.pop();
                indent = frames.len();
                push = Some(Frame::CaseBranch);
            } else {
                require_last(frames, Frame::If, source)?;
                indent = frames.len().saturating_sub(1);
            }
        }
        K::EndIf => {
            pop_expected(frames, Frame::If, source)?;
            indent = frames.len();
        }
        K::Loop => push = Some(Frame::Loop),
        K::EndLoop => {
            pop_expected(frames, Frame::Loop, source)?;
            indent = frames.len();
        }
        K::Case => push = Some(Frame::Case),
        K::When => {
            pop_optional(frames, Frame::CaseBranch);
            pop_optional(frames, Frame::ExceptionBranch);
            indent = frames.len();
            push = match frames.last() {
                Some(Frame::Case) => Some(Frame::CaseBranch),
                Some(Frame::Exception) => Some(Frame::ExceptionBranch),
                _ => return Err(unbalanced(source)),
            };
        }
        K::EndCase => {
            pop_optional(frames, Frame::CaseBranch);
            pop_expected(frames, Frame::Case, source)?;
            indent = frames.len();
        }
        K::Exception => {
            pop_optional(frames, Frame::ExceptionBranch);
            match frames.last_mut() {
                Some(frame @ Frame::Begin) => *frame = Frame::Exception,
                _ => return Err(unbalanced(source)),
            }
            indent = frames.len().saturating_sub(1);
        }
        K::EndBlock => {
            pop_optional(frames, Frame::ExceptionBranch);
            match frames.pop() {
                Some(Frame::Begin | Frame::Exception) => {}
                _ => return Err(unbalanced(source)),
            }
            indent = frames.len();
        }
        K::Comment => indent = frames.len() + usize::from(*in_declare),
        K::Label => indent = frames.len(),
        _ => {}
    }
    Ok((indent, push))
}

fn require_last(frames: &[Frame], expected: Frame, source: &str) -> Result<(), FormatDiagnostic> {
    if frames.last() == Some(&expected) {
        Ok(())
    } else {
        Err(unbalanced(source))
    }
}

fn pop_optional(frames: &mut Vec<Frame>, expected: Frame) {
    if frames.last() == Some(&expected) {
        frames.pop();
    }
}

fn pop_expected(
    frames: &mut Vec<Frame>,
    expected: Frame,
    source: &str,
) -> Result<(), FormatDiagnostic> {
    if frames.pop() == Some(expected) {
        Ok(())
    } else {
        Err(unbalanced(source))
    }
}

fn unbalanced(source: &str) -> FormatDiagnostic {
    FormatDiagnostic::UnsupportedSyntax {
        feature: "unbalanced PL/pgSQL control flow".into(),
        start: 0,
        end: source.len(),
    }
}
