use semblock::config::{Config, GoConfig, RustConfig};
use semblock::source::{Language, format_source, format_source_with_rust};
use semblock::{FormatOptions, Severity, UnsupportedPolicy};
use std::fs;
use std::process::Command;
use tempfile::TempDir;

#[test]
fn go_isolates_syntax_errors_in_explicit_sql_and_formats_valid_siblings() {
    let source = r#"package queries

// semblock:sql
const valid1 = "select id from users where active=true;"

// semblock:sql
const malformed = "select from;"

// semblock:sql
const valid2 = "select name from items where active=true;"
"#;

    let result = format_source(
        source,
        Language::Go,
        &FormatOptions::default(),
        &Config::default().go,
    )
    .expect("formatting with isolated syntax error must succeed");

    assert!(result.changed);
    assert!(
        result
            .output
            .contains(r#"const valid1 = "SELECT id FROM users WHERE active = TRUE;""#)
    );
    assert!(
        result
            .output
            .contains(r#"const valid2 = "SELECT name FROM items WHERE active = TRUE;""#)
    );
    assert!(
        result
            .output
            .contains(r#"const malformed = "select from;""#)
    );

    let syntax_error = result
        .diagnostics
        .iter()
        .find(|d| d.rule_id == "syntax.parse_failure")
        .expect("must contain syntax.parse_failure diagnostic");
    assert_eq!(syntax_error.severity, Severity::Error);
    assert!(
        syntax_error
            .message
            .contains("syntax error at or near \";\"")
    );

    // Under strict policy, no rewrite must happen
    let strict_options = FormatOptions {
        unsupported_policy: UnsupportedPolicy::Error,
        ..FormatOptions::default()
    };
    let strict_result = format_source(source, Language::Go, &strict_options, &Config::default().go)
        .expect("strict policy returns unchanged output with error diagnostic");

    assert_eq!(strict_result.output, source);
    assert!(!strict_result.changed);
    assert!(
        strict_result
            .diagnostics
            .iter()
            .any(|d| d.rule_id == "syntax.parse_failure" && d.severity == Severity::Error)
    );
}

#[test]
fn rust_isolates_syntax_errors_in_explicit_sql_and_formats_valid_siblings() {
    let source = r#"const VALID1: &str = "select id from users where active=true;";
// semblock:sql
const MALFORMED: &str = "select from;";
const VALID2: &str = "select name from items where active=true;";
"#;

    let result = format_source_with_rust(
        source,
        Language::Rust,
        &FormatOptions::default(),
        &GoConfig::default(),
        &RustConfig::default(),
    )
    .expect("formatting with isolated syntax error must succeed");

    assert!(result.changed);
    assert!(
        result
            .output
            .contains(r#"const VALID1: &str = "SELECT id FROM users WHERE active = TRUE;";"#)
    );
    assert!(
        result
            .output
            .contains(r#"const VALID2: &str = "SELECT name FROM items WHERE active = TRUE;";"#)
    );
    assert!(
        result
            .output
            .contains(r#"const MALFORMED: &str = "select from;";"#)
    );

    let syntax_error = result
        .diagnostics
        .iter()
        .find(|d| d.rule_id == "syntax.parse_failure")
        .expect("must contain syntax.parse_failure diagnostic");
    assert_eq!(syntax_error.severity, Severity::Error);
    assert!(
        syntax_error
            .message
            .contains("syntax error at or near \";\"")
    );

    // Under strict policy, no rewrite must happen
    let strict_options = FormatOptions {
        unsupported_policy: UnsupportedPolicy::Error,
        ..FormatOptions::default()
    };
    let strict_result = format_source_with_rust(
        source,
        Language::Rust,
        &strict_options,
        &GoConfig::default(),
        &RustConfig::default(),
    )
    .expect("strict policy returns unchanged output with error diagnostic");

    assert_eq!(strict_result.output, source);
    assert!(!strict_result.changed);
    assert!(
        strict_result
            .diagnostics
            .iter()
            .any(|d| d.rule_id == "syntax.parse_failure" && d.severity == Severity::Error)
    );
}

#[test]
fn cli_fmt_formats_valid_queries_in_mixed_go_file_and_reports_syntax_error() {
    let project = TempDir::new().unwrap();
    let source = r#"package queries

// semblock:sql
const valid = "select id from users where active=true;"

// semblock:sql
const broken = "select from;"
"#;
    let path = project.path().join("queries.go");
    fs::write(&path, source).unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_semblock"))
        .current_dir(project.path())
        .args(["fmt", "queries.go"])
        .output()
        .expect("run semblock fmt");

    assert_eq!(output.status.code(), Some(3));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("queries.go:7:"),
        "stderr should locate error at line 7: {stderr}"
    );
    assert!(
        stderr.contains("error[syntax.parse_failure]"),
        "stderr should contain syntax.parse_failure: {stderr}"
    );

    let unmodified = fs::read_to_string(&path).unwrap();
    assert_eq!(
        unmodified, source,
        "syntax error prevents partial disk write under preflight safety"
    );
}
