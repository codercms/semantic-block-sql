use std::borrow::Cow;
use std::path::Path;

use clap::ValueEnum;
use thiserror::Error;

use crate::config::GoConfig;
use crate::directives::{DirectiveError, format_sql_document};
use crate::host::go::{GoError, format_go_source};
use crate::{FormatOptions, FormatWarning};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Language {
    Auto,
    Sql,
    Go,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormattedSource {
    pub output: String,
    pub changed: bool,
    pub warnings: Vec<FormatWarning>,
}

#[derive(Debug, Error)]
pub enum SourceError {
    #[error("cannot infer language from {0}")]
    UnknownLanguage(String),
    #[error("Go formatting is disabled by configuration")]
    GoDisabled,
    #[error(transparent)]
    SqlDirective(#[from] DirectiveError),
    #[error(transparent)]
    Go(#[from] GoError),
    #[error("source formatting is not idempotent")]
    NotIdempotent,
}

pub fn infer_language(path: &Path, requested: Language) -> Result<Language, SourceError> {
    if requested != Language::Auto {
        return Ok(requested);
    }
    match path.extension().and_then(|extension| extension.to_str()) {
        Some(extension) if extension.eq_ignore_ascii_case("sql") => Ok(Language::Sql),
        Some(extension) if extension.eq_ignore_ascii_case("go") => Ok(Language::Go),
        _ => Err(SourceError::UnknownLanguage(path.display().to_string())),
    }
}

pub fn format_source(
    source: &str,
    language: Language,
    options: &FormatOptions,
    go: &GoConfig,
) -> Result<FormattedSource, SourceError> {
    let first = format_source_once(source, language, options, go)?;
    let second = format_source_once(&first.output, language, options, go)?;
    if first.output != second.output {
        return Err(SourceError::NotIdempotent);
    }
    Ok(FormattedSource {
        changed: first.output != source,
        output: first.output,
        warnings: first.warnings,
    })
}

fn format_source_once(
    source: &str,
    language: Language,
    options: &FormatOptions,
    go: &GoConfig,
) -> Result<FormattedSource, SourceError> {
    match language {
        Language::Auto => Err(SourceError::UnknownLanguage("<source>".into())),
        Language::Sql => {
            let (normalized, newline) = normalize_newlines(source);
            let formatted = format_sql_document(&normalized, options)?;
            let output = restore_newlines(formatted.output, newline);
            Ok(FormattedSource {
                changed: output != source,
                output,
                warnings: formatted.warnings,
            })
        }
        Language::Go if !go.enabled => Err(SourceError::GoDisabled),
        Language::Go => {
            let formatted = format_go_source(source, options, go)?;
            Ok(FormattedSource {
                changed: formatted.output != source,
                output: formatted.output,
                warnings: formatted.warnings,
            })
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Newline {
    Lf,
    CrLf,
}

fn normalize_newlines(source: &str) -> (Cow<'_, str>, Newline) {
    let without_crlf = source.replace("\r\n", "");
    if source.contains("\r\n") && !without_crlf.contains('\n') {
        (Cow::Owned(source.replace("\r\n", "\n")), Newline::CrLf)
    } else {
        (Cow::Borrowed(source), Newline::Lf)
    }
}

fn restore_newlines(source: String, newline: Newline) -> String {
    match newline {
        Newline::Lf => source,
        Newline::CrLf => source.replace('\n', "\r\n"),
    }
}
