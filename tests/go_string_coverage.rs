use semblock::config::{Config, GoMultilineStringStyle};
use semblock::source::{Language, format_source};
use semblock::{FormatOptions, Severity, UnsupportedPolicy};

#[test]
fn formats_interpreted_strings_across_real_expression_contexts() {
    let source = r#"package sample

import "context"

type Config struct {
    Query string `json:"query"`
}

func nested(query string) string { return query }
func use(values ...any) {}

func run(ctx context.Context) {
    const oneLine = "select 1;"
    use(ctx, "select id,name from users where active=true and id=$1;", 1)
    use(nested("select id,name from users;"))
    defer use("select id from audit_log;")
    go use("insert into events(name) values($1);", "created")

    config := Config{Query: "select id,name from users;"}
    queries := map[string]string{"SELECT label": "select id,name from users;"}
    tests := []struct{name, query string}{{"SELECT label", "select id from users;"}}
    _ = oneLine
    _ = config
    _ = queries
    _ = tests
}
"#;

    let formatted = format_source(
        source,
        Language::Go,
        &FormatOptions::default(),
        &Config::default().go,
    )
    .expect("format Go source");

    assert!(formatted.output.contains("const oneLine = \"SELECT 1;\""));
    assert!(formatted.output.contains(
        "use(ctx, `SELECT id, name\nFROM users\nWHERE\n    active = TRUE\n    AND id = $1;`, 1)"
    ));
    assert!(
        formatted
            .output
            .contains("nested(\"SELECT id, name FROM users;\")")
    );
    assert!(
        formatted
            .output
            .contains("defer use(\"SELECT id FROM audit_log;\")")
    );
    assert!(
        formatted
            .output
            .contains("go use(\"INSERT INTO events (name) VALUES ($1);\", \"created\")")
    );
    assert!(
        formatted
            .output
            .contains("Config{Query: \"SELECT id, name FROM users;\"}")
    );
    assert!(
        formatted
            .output
            .contains("\"SELECT label\": \"SELECT id, name FROM users;\"")
    );
    assert!(
        formatted
            .output
            .contains("{\"SELECT label\", \"SELECT id FROM users;\"}")
    );
    assert!(formatted.output.contains("import \"context\""));
    assert!(formatted.output.contains("`json:\"query\"`"));
}

#[test]
fn supports_static_literal_concatenation_but_preserves_dynamic_expressions() {
    let source = r#"package sample

func queries(columns string) {
    static := "SELECT " + "id,name " + "FROM users WHERE active=true " + "AND id>0;"
    parenthesized := ("insert into events(name) " + "values($1);")
    dynamic := "SELECT " + columns + " FROM users"
    _ = static
    _ = parenthesized
    _ = dynamic
}
"#;

    let formatted = format_source(
        source,
        Language::Go,
        &FormatOptions::default(),
        &Config::default().go,
    )
    .expect("format Go source");

    assert!(formatted.output.contains(
        "static := `SELECT id, name\nFROM users\nWHERE\n    active = TRUE\n    AND id > 0;`"
    ));
    assert!(
        formatted
            .output
            .contains("parenthesized := \"INSERT INTO events (name) VALUES ($1);\"")
    );
    assert!(
        formatted
            .output
            .contains("dynamic := \"SELECT \" + columns + \" FROM users\"")
    );
}

#[test]
fn multiline_policy_prefers_raw_and_falls_back_or_preserves_when_required() {
    let source = r#"package sample

const query = "select '`' as marker from users where active=true and id=$1;"
const multiline = "select id,name from users where active=true and id=$1;"
"#;
    let mut config = Config::default().go;
    let formatted = format_source(source, Language::Go, &FormatOptions::default(), &config)
        .expect("format preferred raw strings");
    assert!(formatted.output.contains(
        "const query = \"SELECT '`' AS marker\\nFROM users\\nWHERE\\n    active = TRUE\\n    AND id = $1;\""
    ));
    assert!(formatted.output.contains(
        "const multiline = `SELECT id, name\nFROM users\nWHERE\n    active = TRUE\n    AND id = $1;`"
    ));

    config.multiline_string_style = GoMultilineStringStyle::Preserve;
    let preserved = format_source(source, Language::Go, &FormatOptions::default(), &config)
        .expect("preserve interpreted strings");
    assert!(preserved.output.contains(
        "const multiline = \"SELECT id, name\\nFROM users\\nWHERE\\n    active = TRUE\\n    AND id = $1;\""
    ));
}

#[test]
fn explicit_dynamic_expression_is_warning_by_default_and_error_in_strict_mode() {
    let source = r#"package sample

func queries(columns string) {
    // semblock:sql
    values := []string{
        "SELECT " + columns + " FROM users",
        "select id from users where active=true and id>0;",
    }
    _ = values
}
"#;

    let formatted = format_source(
        source,
        Language::Go,
        &FormatOptions::default(),
        &Config::default().go,
    )
    .expect("default unsupported policy");
    assert!(
        formatted
            .output
            .contains("\"SELECT \" + columns + \" FROM users\"")
    );
    assert!(
        formatted
            .output
            .contains("`SELECT id\nFROM users\nWHERE\n    active = TRUE\n    AND id > 0;`")
    );
    assert!(formatted.diagnostics.iter().any(|diagnostic| {
        diagnostic.rule_id == "syntax.unsupported" && diagnostic.severity == Severity::Warning
    }));

    let strict = FormatOptions {
        unsupported_policy: UnsupportedPolicy::Error,
        ..FormatOptions::default()
    };
    let strict_result = format_source(source, Language::Go, &strict, &Config::default().go)
        .expect("strict unsupported policy returns diagnostics");
    assert_eq!(strict_result.output, source);
    assert!(strict_result.diagnostics.iter().any(|diagnostic| {
        diagnostic.rule_id == "syntax.unsupported" && diagnostic.severity == Severity::Error
    }));
}

#[test]
fn auto_detected_parse_failures_are_skipped_but_explicit_malformed_sql_is_fatal() {
    let automatic = r#"package sample

const malformed = "select from;"
const valid = "select id from users where active=true and id>0;"
"#;
    let formatted = format_source(
        automatic,
        Language::Go,
        &FormatOptions::default(),
        &Config::default().go,
    )
    .expect("skip auto-detected parse failure");
    assert!(
        formatted
            .output
            .contains("const malformed = \"select from;\"")
    );
    assert!(formatted.output.contains(
        "const valid = `SELECT id\nFROM users\nWHERE\n    active = TRUE\n    AND id > 0;`"
    ));

    let explicit = r#"package sample

// semblock:sql
const malformed = "select from;"
"#;
    assert!(
        format_source(
            explicit,
            Language::Go,
            &FormatOptions::default(),
            &Config::default().go,
        )
        .is_err()
    );
}
