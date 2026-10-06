mod support;

use support::{assert_sql_layout_only, assert_unsupported};

#[test]
fn formats_sql_standard_routine_with_default_argument() {
    let source = include_str!("fixtures/sql_standard_routine/function.input.sql");
    let expected = include_str!("fixtures/sql_standard_routine/function.expected.sql");
    assert_sql_layout_only(source, expected);
}

#[test]
fn preserves_unreviewed_multi_statement_sql_routines() {
    assert_unsupported(
        "CREATE FUNCTION f()\nRETURNS void\nLANGUAGE SQL\nBEGIN ATOMIC\nSELECT 1;\nCALL sample_proc();\nEND;",
    );
}

#[test]
fn preserves_language_identifiers_in_sql_standard_routine_signatures() {
    assert_sql_layout_only(
        "CREATE FUNCTION echo_language(language language)\nRETURNS language\nLANGUAGE SQL\nBEGIN ATOMIC\n    SELECT language;\nEND;",
        "CREATE FUNCTION echo_language(language language)\nRETURNS language\nLANGUAGE SQL\nBEGIN ATOMIC\n    SELECT language;\nEND;",
    );
}

#[test]
fn scopes_returns_casing_to_the_sql_routine_clause() {
    assert_sql_layout_only(
        "CREATE FUNCTION echo_returns(returns returns)\nreturns returns\nLANGUAGE SQL\nBEGIN ATOMIC\n    SELECT returns;\nEND;",
        "CREATE FUNCTION echo_returns(returns returns)\nRETURNS returns\nLANGUAGE SQL\nBEGIN ATOMIC\n    SELECT returns;\nEND;",
    );
}

#[test]
fn formats_reviewed_sql_body_shapes_and_parallel_modes() {
    for mode in ["SAFE", "RESTRICTED", "UNSAFE"] {
        let source = format!(
            "CREATE FUNCTION sample_value() RETURNS integer\nLANGUAGE SQL PARALLEL {mode}\nBEGIN ATOMIC\n    -- first group\n    INSERT INTO sample_rows (id) VALUES (1);\n\n    -- result group\n    RETURN (SELECT id FROM sample_rows LIMIT 1);\nEND;"
        );
        let result = semblock::format_sql(&source, &semblock::FormatOptions::default()).unwrap();
        assert!(
            result
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.fix_available)
        );
        semblock::validate_equivalent(&source, &result.output).unwrap();
        assert_sql_layout_only(&result.output, &result.output);
        assert!(result.output.contains(";\n\n    -- result group"));
    }
    let source = "CREATE FUNCTION sample_value() RETURNS integer\nLANGUAGE SQL\n\n    RETURN 1;";
    let expected = "CREATE FUNCTION sample_value() RETURNS integer\nLANGUAGE SQL\n\nRETURN 1;";
    assert_sql_layout_only(source, expected);
}

#[test]
fn unsupported_return_expression_keeps_the_whole_routine_and_its_local_range() {
    let source = "CREATE FUNCTION sample_value() RETURNS text LANGUAGE SQL\nBEGIN ATOMIC\n    SELECT 1;\n    RETURN json_value(payload, '$.id');\nEND;";
    let result = semblock::format_sql_result(source, &semblock::FormatOptions::default());
    assert_eq!(result.output, source);
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.rule_id == "syntax.unsupported")
        .unwrap();
    assert_eq!(
        diagnostic.source_range.start,
        source.find("RETURN json").unwrap()
    );
    let options = semblock::FormatOptions {
        unsupported_policy: semblock::UnsupportedPolicy::Error,
        ..Default::default()
    };
    let strict = semblock::format_sql_result(source, &options);
    assert_eq!(strict.output, source);
    assert!(
        strict
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == semblock::Severity::Error)
    );
}

#[test]
fn atomic_bodies_reuse_each_reviewed_dml_formatter() {
    for statement in [
        "UPDATE sample_rows SET amount = 2 WHERE id = 1;",
        "DELETE FROM sample_rows WHERE id = 2;",
        "MERGE INTO sample_rows USING sample_updates ON sample_rows.id = sample_updates.id WHEN MATCHED THEN UPDATE SET amount = sample_updates.amount;",
    ] {
        let source = format!(
            "CREATE FUNCTION sample_work() RETURNS void LANGUAGE SQL BEGIN ATOMIC {statement} END;"
        );
        let result = semblock::format_sql(&source, &semblock::FormatOptions::default()).unwrap();
        assert!(
            result
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.fix_available)
        );
        semblock::validate_equivalent(&source, &result.output).unwrap();
        assert_sql_layout_only(&result.output, &result.output);
    }
}
