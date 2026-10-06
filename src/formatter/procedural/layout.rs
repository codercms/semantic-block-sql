use super::super::{Diagnostic, FormatDiagnostic, FormatOptions, Severity};
use super::format_leaf;
use super::ir::{BodyNode, BodyNodeKind, RoutineBody};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FormattedBody {
    pub output: String,
    pub diagnostics: Vec<Diagnostic>,
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

pub(super) fn format(
    body: &RoutineBody<'_>,
    options: &FormatOptions,
) -> Result<FormattedBody, FormatDiagnostic> {
    let mut frames = Vec::new();
    let mut lines = Vec::new();
    let mut diagnostics = Vec::new();
    let mut in_declare = false;

    for node in &body.nodes {
        let separate_exception_handler =
            node.kind == BodyNodeKind::When && frames.last() == Some(&Frame::ExceptionBranch);
        let (indent, push_after) = layout_node(node, &mut frames, &mut in_declare, body.source)?;
        let text = if node.kind.is_opaque() {
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
            let (output, leaf_diagnostics) = format_leaf(
                node.kind,
                node.text,
                options,
                indent,
                node.capability.as_ref(),
            )?;
            diagnostics.extend(
                leaf_diagnostics
                    .into_iter()
                    .map(|diagnostic| diagnostic.shifted(node.range.start)),
            );
            output
        };
        let mut rendered = text;
        if let Some(comment) = node.trailing_comment {
            rendered.push(' ');
            rendered.push_str(comment);
        }
        let mut first = true;
        for part in rendered.lines() {
            lines.push(LayoutLine {
                indent,
                relative_indent: part
                    .chars()
                    .take_while(|character| *character == ' ')
                    .count(),
                text: part.trim().to_owned(),
                blank_before: first && (node.blank_before || separate_exception_handler),
            });
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
        if line.blank_before
            && rendered
                .last()
                .is_some_and(|line: &String| !line.is_empty())
        {
            rendered.push(String::new());
        }
        rendered.push(if line.text.is_empty() {
            String::new()
        } else {
            format!(
                "{}{}",
                " ".repeat(line.indent * 4 + line.relative_indent),
                line.text
            )
        });
    }
    while rendered.first().is_some_and(|line| line.is_empty()) {
        rendered.remove(0);
    }
    while rendered.last().is_some_and(|line| line.is_empty()) {
        rendered.pop();
    }
    let output = format!(
        "{}{}{}",
        body.newline,
        rendered.join(body.newline),
        body.newline
    );
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
        !body.nodes.iter().any(|node| {
            node.kind.is_opaque()
                && node.range.start <= diagnostic.source_range.start
                && diagnostic.source_range.end <= node.range.end
        })
    });
    diagnostics.extend(style_diagnostics);
    Ok(FormattedBody {
        output,
        diagnostics,
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
