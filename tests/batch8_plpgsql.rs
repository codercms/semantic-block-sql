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
fn formats_long_return_expressions_at_safe_sql_boundaries() {
    let source = r#"CREATE FUNCTION synthetic_return_width()
RETURNS jsonb
LANGUAGE plpgsql
AS $$
BEGIN
    RETURN compare_record(
        (SELECT snapshot FROM (
            SELECT
                1 AS alpha_attribute,
                2 AS beta_attribute,
                3 AS gamma_attribute,
                4 AS delta_attribute,
                5 AS epsilon_attribute,
                6 AS zeta_attribute,
                7 AS eta_attribute,
                8 AS theta_attribute
        ) snapshot)
    );
END;
$$;"#;
    let expected = r#"CREATE FUNCTION synthetic_return_width()
RETURNS jsonb
LANGUAGE plpgsql
AS $$
BEGIN
    RETURN compare_record(
        (
            SELECT snapshot
            FROM (
            SELECT
            1 AS alpha_attribute,
            2 AS beta_attribute,
            3 AS gamma_attribute,
            4 AS delta_attribute,
            5 AS epsilon_attribute,
            6 AS zeta_attribute,
            7 AS eta_attribute,
            8 AS theta_attribute
        ) snapshot
        )
    );
END;
$$;"#;

    assert_fixture(source, expected);
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

#[test]
fn scopes_returns_casing_to_the_plpgsql_routine_clause() {
    assert_fixture(
        "CREATE FUNCTION echo_returns(returns returns) returns returns LANGUAGE plpgsql AS $$\nBEGIN\n    RETURN returns;\nEND;\n$$;",
        "CREATE FUNCTION echo_returns(returns returns) RETURNS returns LANGUAGE plpgsql AS $$\nBEGIN\n    RETURN returns;\nEND;\n$$;",
    );
}
