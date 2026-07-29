use semblock::{FormatOptions, Severity, UnsupportedPolicy, format_sql, format_sql_result};

#[test]
fn unsupported_statement_family_returns_original_source() {
    let source = "CREATE TABLE child (LIKE parent INCLUDING ALL);";
    let formatted = format_sql_result(source, &FormatOptions::default());

    assert_eq!(formatted.output, source);
    assert!(!formatted.changed);
    assert_eq!(formatted.diagnostics.len(), 1);
    assert_eq!(formatted.diagnostics[0].rule_id, "syntax.unsupported");
    assert_eq!(formatted.diagnostics[0].severity, Severity::Warning);
    assert_eq!(formatted.diagnostics[0].source_range.start, 0);
    assert_eq!(formatted.diagnostics[0].source_range.end, source.len());
    assert!(!formatted.diagnostics[0].fix_available);

    let direct = format_sql(source, &FormatOptions::default()).expect("unsupported is non-fatal");
    assert_eq!(direct.output, source);
    assert_eq!(direct.diagnostics[0].severity, Severity::Warning);

    let mut strict = FormatOptions::default();
    strict.unsupported_policy = UnsupportedPolicy::Error;
    let strict_result = format_sql(source, &strict).expect("strict policy is a result policy");
    assert_eq!(strict_result.output, source);
    assert_eq!(strict_result.diagnostics[0].severity, Severity::Error);
}

#[test]
fn unowned_select_layouts_are_explicitly_unsupported() {
    for source in [
        "SELECT id FROM ONLY items;",
        "SELECT * FROM XMLTABLE('/items/item' PASSING payload COLUMNS id bigint PATH '@id') AS item;",
    ] {
        let formatted = format_sql_result(source, &FormatOptions::default());
        assert_eq!(formatted.output, source, "{source}");
        assert!(!formatted.changed, "{source}");
        assert_eq!(formatted.diagnostics.len(), 1, "{source}");
        assert_eq!(formatted.diagnostics[0].rule_id, "syntax.unsupported");
    }
}

#[test]
fn fixture_backed_recursive_union_all_remains_supported() {
    let source = include_str!("fixtures/batch2/recursive-cte.input.sql");
    let formatted = format_sql_result(source, &FormatOptions::default());

    assert!(formatted.changed);
    assert_eq!(
        formatted.output,
        include_str!("fixtures/batch2/recursive-cte.expected.sql")
    );
    assert!(
        formatted
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.rule_id != "syntax.unsupported")
    );
}
