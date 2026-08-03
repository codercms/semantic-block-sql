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
    assert!(
        stderr.contains("query.sql:3:1 (bytes ") && stderr.contains("): error[syntax.unsupported]"),
        "{stderr}"
    );
}
