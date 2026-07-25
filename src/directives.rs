use thiserror::Error;

use crate::{FormatDiagnostic, FormatOptions, FormatWarning, format_sql};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DocumentFormat {
    pub output: String,
    pub warnings: Vec<FormatWarning>,
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum DirectiveError {
    #[error("misplaced semblock:file-ignore at line {line}")]
    MisplacedFileIgnore { line: usize },
    #[error("nested semblock:off at line {line}")]
    NestedOff { line: usize },
    #[error("unmatched semblock:on at line {line}")]
    UnmatchedOn { line: usize },
    #[error("unmatched semblock:off at line {line}")]
    UnmatchedOff { line: usize },
    #[error(transparent)]
    Format(#[from] FormatDiagnostic),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Directive {
    FileIgnore,
    Off,
    On,
}

pub fn format_sql_document(
    source: &str,
    options: &FormatOptions,
) -> Result<DocumentFormat, DirectiveError> {
    let lines = lines_with_offsets(source);
    let first_nonempty = lines
        .iter()
        .find(|line| !line.text.trim().is_empty())
        .map(|line| line.number);

    if lines.iter().any(|line| {
        line.number == first_nonempty.unwrap_or(0)
            && directive(line.text) == Some(Directive::FileIgnore)
    }) {
        return Ok(DocumentFormat {
            output: source.into(),
            warnings: Vec::new(),
        });
    }

    if let Some(line) = lines
        .iter()
        .find(|line| directive(line.text) == Some(Directive::FileIgnore))
    {
        return Err(DirectiveError::MisplacedFileIgnore { line: line.number });
    }

    let mut output = String::with_capacity(source.len());
    let mut warnings = Vec::new();
    let mut active_start = 0;
    let mut ignored_start = None;
    let mut off_line = None;

    for line in &lines {
        match directive(line.text) {
            Some(Directive::Off) if ignored_start.is_some() => {
                return Err(DirectiveError::NestedOff { line: line.number });
            }
            Some(Directive::Off) => {
                append_formatted(
                    &source[active_start..line.start],
                    options,
                    &mut output,
                    &mut warnings,
                )?;
                output.push_str(&source[line.start..line.end]);
                ignored_start = Some(line.end);
                off_line = Some(line.number);
            }
            Some(Directive::On) => {
                let start = ignored_start
                    .take()
                    .ok_or(DirectiveError::UnmatchedOn { line: line.number })?;
                output.push_str(&source[start..line.start]);
                output.push_str(&source[line.start..line.end]);
                active_start = line.end;
                off_line = None;
            }
            Some(Directive::FileIgnore) | None => {}
        }
    }

    if ignored_start.is_some() {
        return Err(DirectiveError::UnmatchedOff {
            line: off_line.unwrap_or(lines.len().max(1)),
        });
    }

    append_formatted(&source[active_start..], options, &mut output, &mut warnings)?;
    Ok(DocumentFormat { output, warnings })
}

fn append_formatted(
    source: &str,
    options: &FormatOptions,
    output: &mut String,
    warnings: &mut Vec<FormatWarning>,
) -> Result<(), FormatDiagnostic> {
    if source.trim().is_empty() {
        output.push_str(source);
        return Ok(());
    }
    let formatted = format_sql(source, options)?;
    output.push_str(&formatted.output);
    warnings.extend(formatted.warnings);
    Ok(())
}

fn directive(line: &str) -> Option<Directive> {
    match line.trim() {
        "-- semblock:file-ignore" => Some(Directive::FileIgnore),
        "-- semblock:off" => Some(Directive::Off),
        "-- semblock:on" => Some(Directive::On),
        _ => None,
    }
}

struct Line<'a> {
    number: usize,
    start: usize,
    end: usize,
    text: &'a str,
}

fn lines_with_offsets(source: &str) -> Vec<Line<'_>> {
    let mut offset = 0;
    source
        .split_inclusive('\n')
        .enumerate()
        .map(|(index, text)| {
            let start = offset;
            offset += text.len();
            Line {
                number: index + 1,
                start,
                end: offset,
                text,
            }
        })
        .collect()
}
