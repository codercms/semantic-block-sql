use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use tempfile::TempDir;

fn semblock() -> Command {
    Command::new(env!("CARGO_BIN_EXE_semblock"))
}

fn run(root: &Path, args: &[&str], stdin: Option<&str>) -> Output {
    let mut command = semblock();
    command.current_dir(root).args(args);
    if let Some(stdin) = stdin {
        command.stdin(Stdio::piped());
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn semblock");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(stdin.as_bytes())
            .expect("write stdin");
        child.wait_with_output().expect("wait for semblock")
    } else {
        command.output().expect("run semblock")
    }
}

fn write(root: &Path, path: &str, contents: &str) {
    fs::write(root.join(path), contents).expect("write fixture");
}

fn warning_lines(output: &Output) -> Vec<String> {
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .filter(|line| line.contains(": warning["))
        .map(str::to_owned)
        .collect()
}

fn location(source: &str, needle: &str) -> (usize, usize, usize, usize) {
    let start = source.find(needle).expect("diagnostic slice");
    let prefix = &source[..start];
    let line_start = prefix.rfind('\n').map_or(0, |newline| newline + 1);
    (
        prefix.bytes().filter(|byte| *byte == b'\n').count() + 1,
        prefix[line_start..].chars().count() + 1,
        start,
        start + needle.len(),
    )
}

fn diagnostic_scenario() -> (&'static str, &'static str) {
    let literal = "'длинная строка, которая намеренно превышает настроенную жесткую границу и остается неделимой'";
    let source = "select 'я' as label,id from public.items where active=true and (title is not null or original_title is not null);\nSELECT 'длинная строка, которая намеренно превышает настроенную жесткую границу и остается неделимой';\nCREATE TABLE public.new_items (LIKE public.items INCLUDING ALL);\nALTER TABLE public.long_table_name ALTER COLUMN long_column_name SET DEFAULT 123;\n";
    (source, literal)
}

#[test]
fn style_errors_render_character_location_before_utf8_byte_range() {
    let project = TempDir::new().expect("temp project");
    let source = "SELECT 'я' as value;\n";
    write(project.path(), "query.sql", source);

    let output = run(project.path(), &["check", "query.sql"], None);

    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("query.sql:1:12 (bytes 12-14): error[casing.keyword]"),
        "{stderr}"
    );
}

#[test]
fn unsupported_warnings_render_statement_line_before_byte_range() {
    let project = TempDir::new().expect("temp project");
    let source = "-- префикс\nSELECT id FROM public.items;\n\nCREATE TABLE public.new_items (LIKE public.items INCLUDING ALL);\n";
    write(project.path(), "query.sql", source);

    let output = run(project.path(), &["fmt", "query.sql"], None);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("query.sql:4:1 (bytes ")
            && stderr.contains("): warning[syntax.unsupported]"),
        "{stderr}"
    );
}

#[test]
fn strict_stdin_errors_use_the_stdin_source_for_line_locations() {
    let project = TempDir::new().expect("temp project");
    let source = "SELECT id FROM public.items;\r\n\r\nCREATE TABLE public.new_items (LIKE public.items INCLUDING ALL);\r\n";

    let output = run(
        project.path(),
        &[
            "--strict-unsupported",
            "fmt",
            "--stdin",
            "--filename",
            "query.sql",
        ],
        Some(source),
    );

    assert_eq!(output.status.code(), Some(3), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    let unsupported = "CREATE TABLE public.new_items (LIKE public.items INCLUDING ALL);";
    let (_, _, start, end) = location(source, unsupported);
    assert!(
        stderr.contains(&format!(
            "query.sql:3:1 (bytes {start}-{end}): error[syntax.unsupported]"
        )),
        "{stderr}"
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout), source);
}

#[test]
fn successful_file_fmt_reports_the_rewritten_schema_coordinates_on_every_run() {
    let project = TempDir::new().expect("temp project");
    let (source, literal) = diagnostic_scenario();
    write(project.path(), "schema.sql", source);
    write(
        project.path(),
        "semblock.toml",
        "[layout]\nsoft_line_width = 32\nhard_line_width = 40\n",
    );

    let first = run(project.path(), &["fmt", "schema.sql"], None);
    assert_eq!(first.status.code(), Some(0), "{first:?}");
    let rewritten = fs::read_to_string(project.path().join("schema.sql")).expect("rewritten SQL");
    let first_warnings = warning_lines(&first);
    assert_eq!(first_warnings.len(), 3, "{first_warnings:#?}");

    let (line, column, start, end) = location(&rewritten, literal);
    assert!(
        first_warnings
            .iter()
            .any(|warning| warning.starts_with(&format!(
                "schema.sql:{line}:{column} (bytes {start}-{end}): warning[layout.hard_line_width]"
            ))),
        "{first_warnings:#?}"
    );
    for rule in [
        "layout.hard_line_width",
        "syntax.unsupported",
        "format.statement_skipped",
    ] {
        assert!(
            first_warnings
                .iter()
                .any(|warning| warning.contains(&format!("warning[{rule}]"))),
            "{first_warnings:#?}"
        );
    }

    let second = run(project.path(), &["fmt", "schema.sql"], None);
    assert_eq!(second.status.code(), Some(0), "{second:?}");
    assert_eq!(warning_lines(&second), first_warnings);
    assert_eq!(
        fs::read_to_string(project.path().join("schema.sql")).expect("idempotent SQL"),
        rewritten
    );
}

#[test]
fn check_and_diff_keep_input_coordinates_without_rewriting() {
    let project = TempDir::new().expect("temp project");
    let (source, literal) = diagnostic_scenario();
    write(project.path(), "schema.sql", source);
    write(
        project.path(),
        "semblock.toml",
        "[layout]\nsoft_line_width = 32\nhard_line_width = 40\n",
    );
    let (line, column, start, end) = location(source, literal);

    let checked = run(project.path(), &["check", "schema.sql"], None);
    assert_eq!(checked.status.code(), Some(1), "{checked:?}");
    let checked_warnings = warning_lines(&checked);
    assert!(
        checked_warnings
            .iter()
            .any(|warning| warning.starts_with(&format!(
                "schema.sql:{line}:{column} (bytes {start}-{end}): warning[layout.hard_line_width]"
            ))),
        "{checked_warnings:#?}"
    );
    assert_eq!(
        fs::read_to_string(project.path().join("schema.sql")).expect("checked SQL"),
        source
    );

    let diffed = run(project.path(), &["diff", "schema.sql"], None);
    assert_eq!(diffed.status.code(), Some(1), "{diffed:?}");
    assert_eq!(warning_lines(&diffed), checked_warnings);
    assert_eq!(
        fs::read_to_string(project.path().join("schema.sql")).expect("diffed SQL"),
        source
    );
}

#[test]
fn successful_stdin_fmt_reports_formatted_stdout_coordinates() {
    let project = TempDir::new().expect("temp project");
    let (source, literal) = diagnostic_scenario();
    write(
        project.path(),
        "semblock.toml",
        "[layout]\nsoft_line_width = 32\nhard_line_width = 40\n",
    );

    let output = run(
        project.path(),
        &["fmt", "--stdin", "--filename", "schema.sql"],
        Some(source),
    );
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let stdout = String::from_utf8(output.stdout.clone()).expect("UTF-8 stdout");
    let warnings = warning_lines(&output);
    let (line, column, start, end) = location(&stdout, literal);
    assert!(
        warnings.iter().any(|warning| warning.starts_with(&format!(
            "schema.sql:{line}:{column} (bytes {start}-{end}): warning[layout.hard_line_width]"
        ))),
        "{warnings:#?}"
    );
}
