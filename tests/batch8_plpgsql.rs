use pretty_assertions::assert_eq;
use semblock::{FormatOptions, Severity, check_sql, format_sql, format_sql_result};

fn assert_fixture(source: &str, expected: &str) {
    let formatted = format_sql(source, &FormatOptions::default()).expect("format succeeds");
    assert_eq!(formatted.output, expected);
    assert_eq!(
        format_sql(expected, &FormatOptions::default())
            .expect("idempotent format succeeds")
            .output,
        expected
    );
    assert!(check_sql(expected, &FormatOptions::default()).compliant);
}

#[test]
fn formats_parser_backed_plpgsql_and_preserves_dollar_tags() {
    assert_fixture(
        include_str!("fixtures/batch8/do.input.sql"),
        include_str!("fixtures/batch8/do.expected.sql"),
    );
    assert_fixture(
        include_str!("fixtures/batch8/function.input.sql"),
        include_str!("fixtures/batch8/function.expected.sql"),
    );
    assert_fixture(
        include_str!("fixtures/batch8/procedure.input.sql"),
        include_str!("fixtures/batch8/procedure.expected.sql"),
    );
}

#[test]
fn rejects_unsupported_procedural_nodes_without_changes() {
    let source = "DO $$\nBEGIN\n    FOR item_id IN 1..3 LOOP\n        PERFORM refresh_item(item_id);\n    END LOOP;\nEND;\n$$;";
    let result = format_sql_result(source, &FormatOptions::default());
    assert_eq!(result.output, source);
    assert!(!result.changed);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.rule_id == "syntax.unsupported")
    );
}

#[test]
fn rejects_non_plpgsql_and_compact_bodies() {
    for source in [
        "CREATE FUNCTION f() RETURNS int LANGUAGE SQL AS $$ SELECT 1 $$;",
        "DO $$ BEGIN PERFORM 1; END; $$;",
    ] {
        let result = format_sql_result(source, &FormatOptions::default());
        assert_eq!(result.output, source);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.severity == Severity::Error)
        );
    }
}
