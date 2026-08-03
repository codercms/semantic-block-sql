use pretty_assertions::assert_eq;
use semblock::{FormatOptions, Severity, UnsupportedPolicy, format_sql};

fn options(policy: UnsupportedPolicy) -> FormatOptions {
    FormatOptions {
        soft_line_width: 32,
        hard_line_width: 40,
        unsupported_policy: policy,
        ..FormatOptions::default()
    }
}

#[test]
fn default_policy_skips_only_the_statement_that_cannot_be_formatted() {
    let failed =
        "ALTER TABLE public.long_table_name ALTER COLUMN long_column_name SET DEFAULT 123;";
    let source =
        format!("select id,name from users;\n\n{failed}\n\nselect id,created_at from audit_log;");
    let expected = format!(
        "SELECT id, name FROM users;\n\n{failed}\n\nSELECT id, created_at\nFROM audit_log;"
    );

    let result = format_sql(&source, &options(UnsupportedPolicy::Skip))
        .expect("a statement-level failure must not discard sibling formatting");

    assert_eq!(result.output, expected);
    let skipped = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.rule_id == "format.statement_skipped")
        .expect("the preserved statement must be diagnosed");
    assert_eq!(skipped.severity, Severity::Warning);
    assert_eq!(
        &source[skipped.source_range.start..skipped.source_range.end],
        failed
    );
    assert!(skipped.message.contains("line 3"), "{}", skipped.message);
}

#[test]
fn strict_policy_preserves_the_complete_document_after_a_statement_failure() {
    let failed =
        "ALTER TABLE public.long_table_name ALTER COLUMN long_column_name SET DEFAULT 123;";
    let source = format!("select id,name from users;\n\n{failed}\n\nselect id from audit_log;");

    let result = format_sql(&source, &options(UnsupportedPolicy::Error))
        .expect("a safely preserved document is still a formatter result");

    assert_eq!(result.output, source);
    assert!(!result.changed);
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.rule_id == "format.statement_skipped" && diagnostic.severity == Severity::Error
    }));
}
