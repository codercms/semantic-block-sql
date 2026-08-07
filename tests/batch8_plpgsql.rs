mod support;

use pretty_assertions::assert_eq;
use semblock::{FormatOptions, Severity, format_sql_result};
use support::assert_sql_layout_only as assert_fixture;

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

#[test]
fn preserves_language_identifiers_in_plpgsql_routine_signatures() {
    assert_fixture(
        "CREATE FUNCTION echo_language(language language) RETURNS language LANGUAGE plpgsql AS $$ BEGIN RETURN language; END; $$;",
        "CREATE FUNCTION echo_language(language language) RETURNS language LANGUAGE plpgsql AS $$\nBEGIN\n    RETURN language;\nEND;\n$$;",
    );
}
