mod support;

use pretty_assertions::assert_eq;
use semblock::{FormatOptions, Severity, UnsupportedPolicy, format_sql};
use support::assert_sql_layout_only as assert_fixture;

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
fn formats_assert_and_return_query() {
    assert_fixture(
        "CREATE FUNCTION stream_items() RETURNS SETOF bigint LANGUAGE plpgsql AS $$ BEGIN ASSERT active, 'must be active'; RETURN QUERY select id from items where active=true; END; $$;",
        "CREATE FUNCTION stream_items() RETURNS SETOF bigint LANGUAGE plpgsql AS $$\nBEGIN\n    ASSERT active, 'must be active';\n    RETURN QUERY SELECT id FROM items WHERE active = TRUE;\nEND;\n$$;",
    );
}

#[test]
fn preserves_unsupported_transaction_control_while_formatting_siblings() {
    let source =
        "CREATE PROCEDURE p() LANGUAGE plpgsql AS $$ BEGIN perform 1; COMMIT; perform 2; END; $$;";
    let expected = "CREATE PROCEDURE p() LANGUAGE plpgsql AS $$\nBEGIN\n    PERFORM 1;\n    COMMIT;\n    PERFORM 2;\nEND;\n$$;";
    let result = format_sql(source, &FormatOptions::default()).expect("format succeeds");
    assert_eq!(result.output, expected);
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.rule_id == "syntax.unsupported" && diagnostic.severity == Severity::Warning
    }));

    let strict = FormatOptions {
        unsupported_policy: UnsupportedPolicy::Error,
        ..FormatOptions::default()
    };
    let result = format_sql(source, &strict).expect("strict policy returns a result");
    assert_eq!(result.output, source);
    assert!(!result.changed);
    assert!(result.diagnostics.iter().any(|diagnostic| {
        diagnostic.rule_id == "syntax.unsupported" && diagnostic.severity == Severity::Error
    }));
}
