use pretty_assertions::assert_eq;
use semblock::{
    FormatOptions, Severity, UnsupportedPolicy, check_sql, format_sql, format_sql_result,
};

#[test]
fn formats_supported_siblings_and_preserves_unsupported_statements() {
    let source = "select id,name from users;\n\nselect * from json_table(payload, '$[*]' columns (value text path '$.value')) jt;\n\nupdate users set active=true where id=$1;\n";
    let expected = "SELECT id, name FROM users;\n\nselect * from json_table(payload, '$[*]' columns (value text path '$.value')) jt;\n\nUPDATE users SET active = TRUE WHERE id = $1;\n";

    let formatted = format_sql(source, &FormatOptions::default()).expect("format succeeds");

    assert_eq!(formatted.output, expected);
    assert!(formatted.changed);
    let unsupported = formatted
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.rule_id == "syntax.unsupported")
        .expect("unsupported warning");
    assert_eq!(unsupported.severity, Severity::Warning);
    assert_eq!(
        &source[unsupported.source_range.start..unsupported.source_range.end],
        "select * from json_table(payload, '$[*]' columns (value text path '$.value')) jt;"
    );
    assert!(check_sql(expected, &FormatOptions::default()).compliant);
}

#[test]
fn strict_unsupported_preserves_the_complete_document_and_collects_errors() {
    let source = "select id,name from users;\n\nselect * from json_table(payload, '$[*]' columns (value text path '$.value')) jt;\n\ncreate table child (like parent including all);\n";
    let mut options = FormatOptions::default();
    options.unsupported_policy = UnsupportedPolicy::Error;

    let formatted = format_sql(source, &options).expect("strict policy is non-panicking");

    assert_eq!(formatted.output, source);
    assert!(!formatted.changed);
    let unsupported = formatted
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.rule_id == "syntax.unsupported")
        .collect::<Vec<_>>();
    assert_eq!(unsupported.len(), 2);
    assert!(
        unsupported
            .iter()
            .all(|diagnostic| diagnostic.severity == Severity::Error)
    );
}

#[test]
fn malformed_sql_remains_fatal_under_the_default_policy() {
    let source = "SELECT FROM;\nSELECT id FROM users;";
    let formatted = format_sql_result(source, &FormatOptions::default());

    assert_eq!(formatted.output, source);
    assert!(!formatted.changed);
    assert_eq!(formatted.diagnostics.len(), 1);
    assert_eq!(formatted.diagnostics[0].rule_id, "syntax.parse_failure");
    assert_eq!(formatted.diagnostics[0].severity, Severity::Error);
}
