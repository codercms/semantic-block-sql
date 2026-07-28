use std::env;
use std::path::Path;

use semblock::Severity;
use semblock::source::FormattedSource;

#[derive(Debug, Clone, Copy, Default)]
pub(super) struct CheckOutput {
    pub(super) list_different: bool,
    pub(super) summary: bool,
}

pub(super) fn emit_diagnostics(
    path: &Path,
    formatted: &FormattedSource,
    quiet: bool,
    include_style_errors: bool,
) {
    if quiet {
        return;
    }
    for diagnostic in &formatted.diagnostics {
        if !include_style_errors && diagnostic.severity == Severity::Error {
            continue;
        }
        let severity = match diagnostic.severity {
            Severity::Error => "error",
            Severity::Warning => "warning",
        };
        eprintln!(
            "{}:{}-{}: {severity}[{}]: {}",
            path.display(),
            diagnostic.source_range.start,
            diagnostic.source_range.end,
            diagnostic.rule_id,
            diagnostic.message
        );
    }
}

pub(super) fn emit_check_path(
    path: &Path,
    formatted: &FormattedSource,
    output: CheckOutput,
    quiet: bool,
) {
    if !formatted.changed {
        return;
    }
    if output.list_different {
        println!("{}", display_path(path));
    } else if !quiet && formatted.diagnostics.is_empty() {
        eprintln!("Would reformat: {}", path.display());
    }
}

pub(super) fn emit_check_summary(formatted: &[FormattedSource]) {
    let checked = formatted.len();
    let changed = formatted.iter().filter(|item| item.changed).count();
    let unchanged = checked - changed;
    eprintln!("Checked {checked} input(s): {changed} would change, {unchanged} unchanged");
}

pub(super) fn emit_check_summary_refs<'a>(
    formatted: impl IntoIterator<Item = &'a FormattedSource>,
) {
    let mut checked = 0;
    let mut changed = 0;
    for item in formatted {
        checked += 1;
        changed += usize::from(item.changed);
    }
    let unchanged = checked - changed;
    eprintln!("Checked {checked} input(s): {changed} would change, {unchanged} unchanged");
}

pub(super) fn display_path(path: &Path) -> String {
    let current = env::current_dir().ok();
    current
        .as_deref()
        .and_then(|current| path.strip_prefix(current).ok())
        .unwrap_or(path)
        .to_string_lossy()
        .trim_start_matches("./")
        .to_string()
}
