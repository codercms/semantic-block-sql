use semblock::{FormatDiagnostic, FormatOptions, Severity, format_sql, format_sql_result};

#[test]
fn unsupported_statement_family_returns_original_source() {
    let source = "UPDATE items SET id = 1;";
    let formatted = format_sql_result(source, &FormatOptions::default());

    assert_eq!(formatted.output, source);
    assert!(!formatted.changed);
    assert_eq!(formatted.diagnostics.len(), 1);
    assert_eq!(formatted.diagnostics[0].rule_id, "syntax.unsupported");
    assert_eq!(formatted.diagnostics[0].severity, Severity::Error);
    assert_eq!(formatted.diagnostics[0].source_range.start, 0);
    assert_eq!(formatted.diagnostics[0].source_range.end, source.len());
    assert!(!formatted.diagnostics[0].fix_available);

    assert!(matches!(
        format_sql(source, &FormatOptions::default()),
        Err(FormatDiagnostic::UnsupportedSyntax { .. })
    ));
}

#[test]
fn unowned_select_layouts_are_explicitly_unsupported() {
    for source in [
        "SELECT DISTINCT id FROM items;",
        "SELECT id FROM items ORDER BY id;",
        "SELECT id FROM items LIMIT 1;",
        "SELECT COUNT(*) FILTER (WHERE active) FROM items;",
        "SELECT * FROM LATERAL (SELECT 1) source;",
        "SELECT 1 UNION ALL SELECT 2;",
        "SELECT * FROM (SELECT 1 UNION ALL SELECT 2) source;",
        "VALUES (1), (2);",
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
