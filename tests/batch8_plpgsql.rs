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
fn formats_assert_and_compact_bodies() {
    assert_fixture(
        "DO $$ BEGIN ASSERT active, 'must be active'; PERFORM 1; END; $$;",
        "DO $$\nBEGIN\n    ASSERT active, 'must be active';\n    PERFORM 1;\nEND;\n$$;",
    );
}

#[test]
fn rejects_non_plpgsql_bodies() {
    let source = "CREATE FUNCTION f() RETURNS int LANGUAGE SQL AS $$ SELECT 1 $$;";
    let result = format_sql_result(source, &FormatOptions::default());
    assert_eq!(result.output, source);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Warning)
    );
}
