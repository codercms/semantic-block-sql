use pretty_assertions::assert_eq;
use semblock::{FormatOptions, Severity, check_sql, format_sql, format_sql_result};

fn assert_fixture(source: &str, expected: &str) {
    let options = FormatOptions::default();
    let formatted = format_sql(source, &options).expect("format succeeds");
    assert_eq!(formatted.output, expected);
    assert_eq!(
        format_sql(expected, &options)
            .expect("second format succeeds")
            .output,
        expected,
        "formatting must be idempotent",
    );
    let checked = check_sql(expected, &options);
    assert!(checked.compliant, "diagnostics: {:?}", checked.diagnostics);
}

#[test]
fn formats_loops_foreach_and_labeled_exit_continue() {
    assert_fixture(
        include_str!("fixtures/batch12/loops.input.sql"),
        include_str!("fixtures/batch12/loops.expected.sql"),
    );
}

#[test]
fn formats_procedural_case_and_dynamic_execute() {
    assert_fixture(
        include_str!("fixtures/batch12/case-execute.input.sql"),
        include_str!("fixtures/batch12/case-execute.expected.sql"),
    );
}

#[test]
fn formats_cursor_declaration_open_fetch_move_and_close() {
    assert_fixture(
        include_str!("fixtures/batch12/cursors.input.sql"),
        include_str!("fixtures/batch12/cursors.expected.sql"),
    );
}

#[test]
fn unreviewed_procedural_neighbors_remain_fail_safe() {
    for source in [
        "DO $$\nBEGIN\nASSERT active, 'must be active';\nEND;\n$$;",
        "CREATE FUNCTION stream_items() RETURNS SETOF bigint LANGUAGE plpgsql AS $$\nBEGIN\nRETURN QUERY SELECT id FROM items;\nEND;\n$$;",
        "DO $$\nBEGIN\nCOMMIT;\nEND;\n$$;",
    ] {
        let result = format_sql_result(source, &FormatOptions::default());
        assert_eq!(result.output, source, "{source}");
        assert!(!result.changed, "{source}");
        assert!(
            result.diagnostics.iter().any(|diagnostic| {
                diagnostic.rule_id == "syntax.unsupported" && diagnostic.severity == Severity::Error
            }),
            "{source}: {:?}",
            result.diagnostics,
        );
    }
}
