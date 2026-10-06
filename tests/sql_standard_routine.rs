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

#[test]
fn dollar_sql_bodies_preserve_delimiters_comments_and_multiline_literals() {
    let source = "CREATE FUNCTION sample_value(input_value int) RETURNS text\nLANGUAGE SQL IMMUTABLE STRICT PARALLEL SAFE SECURITY DEFINER COST 10\nAS $sample$\n-- retained literal\nSELECT 'first\n  second'::text;\n\nSELECT $1::text;\n$sample$;";
    let result = semblock::format_sql(source, &semblock::FormatOptions::default()).unwrap();
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.fix_available),
        "{:?}",
        result.diagnostics
    );
    assert!(result.output.contains("$sample$"));
    assert!(result.output.contains("'first\n  second'"));
    assert_sql_layout_only(&result.output, &result.output);
    let source = "CREATE FUNCTION sample_work() RETURNS void AS $$INSERT INTO sample_rows (id) VALUES (1); DELETE FROM sample_rows WHERE id = 2;$$ LANGUAGE SQL;";
    let result = semblock::format_sql(source, &semblock::FormatOptions::default()).unwrap();
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.fix_available)
    );
    assert_sql_layout_only(&result.output, &result.output);
}

#[test]
fn dollar_sql_body_unsupported_nodes_preserve_the_complete_routine() {
    let source = "CREATE FUNCTION sample_value() RETURNS text LANGUAGE SQL AS $$\nSELECT 1;\nSELECT json_value(payload, '$.id') FROM sample_rows;\n$$;";
    let result = semblock::format_sql_result(source, &semblock::FormatOptions::default());
    assert_eq!(result.output, source);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.rule_id == "syntax.unsupported")
    );
}

#[test]
fn routines_bind_locations_relative_to_their_own_document_statement() {
    let source = "SELECT 'café';\n\n-- routine group\nCREATE FUNCTION sample_value() RETURNS integer LANGUAGE SQL RETURN 1;\nCREATE FUNCTION sample_other() RETURNS integer LANGUAGE SQL RETURN 2;";
    let result = semblock::format_sql(source, &semblock::FormatOptions::default()).unwrap();
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.fix_available),
        "{:?}",
        result.diagnostics
    );
    semblock::validate_equivalent(source, &result.output).unwrap();
    assert_sql_layout_only(&result.output, &result.output);
}

#[test]
fn sql_routine_options_have_fixture_backed_ownership() {
    for option in [
        "VOLATILE CALLED ON NULL INPUT SECURITY INVOKER NOT LEAKPROOF",
        "STABLE RETURNS NULL ON NULL INPUT LEAKPROOF SUPPORT sample_support",
        "IMMUTABLE STRICT COST 10 ROWS 20 SET search_path TO sample_schema, public",
        "SET work_mem FROM CURRENT PARALLEL RESTRICTED",
    ] {
        let source = format!(
            "CREATE FUNCTION sample_options() RETURNS SETOF integer LANGUAGE SQL {option} AS $$SELECT 1;$$;"
        );
        let result = semblock::format_sql(&source, &semblock::FormatOptions::default()).unwrap();
        assert!(
            result
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.fix_available),
            "{option}: {:?}",
            result.diagnostics
        );
        assert_sql_layout_only(&result.output, &result.output);
    }
}

#[test]
fn unreviewed_sql_body_quoting_is_preserved() {
    for literal in ["'SELECT 1;'", "E'SELECT 1;'"] {
        let source =
            format!("CREATE FUNCTION sample_quoted() RETURNS integer LANGUAGE SQL AS {literal};");
        let result = semblock::format_sql_result(&source, &semblock::FormatOptions::default());
        assert_eq!(result.output, source);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.rule_id == "syntax.unsupported")
        );
    }
}

#[test]
fn shared_headers_cover_sql_bodies_table_columns_and_parameter_defaults() {
    let arguments = (1..=8)
        .map(|index| format!("argument_{index} integer DEFAULT (1 + 2)"))
        .collect::<Vec<_>>()
        .join(", ");
    for body in ["AS $$SELECT 1;$$", "BEGIN ATOMIC SELECT 1; END", "RETURN 1"] {
        let source = format!(
            "CREATE FUNCTION sample_header({arguments}) RETURNS integer LANGUAGE SQL {body};"
        );
        let result = semblock::format_sql(&source, &semblock::FormatOptions::default()).unwrap();
        assert!(
            result
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.fix_available),
            "{body}: {:?}",
            result.diagnostics
        );
        assert!(
            result
                .output
                .lines()
                .all(|line| line.chars().count() <= 160)
        );
        assert_sql_layout_only(&result.output, &result.output);
    }
    let fields = (1..=8)
        .map(|index| format!("column_{index} integer"))
        .collect::<Vec<_>>()
        .join(", ");
    let source = format!(
        "CREATE FUNCTION sample_header(language language, returns returns) RETURNS TABLE ({fields}) LANGUAGE SQL AS $$SELECT 1;$$;"
    );
    let result = semblock::format_sql(&source, &semblock::FormatOptions::default()).unwrap();
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.fix_available),
        "{:?}",
        result.diagnostics
    );
    assert!(result.output.contains("language language,"));
    assert!(result.output.contains("returns returns"));
    assert_sql_layout_only(&result.output, &result.output);
}

#[test]
fn routine_argument_comments_and_blank_groups_survive_header_expansion() {
    let source = "CREATE FUNCTION sample_groups(\n    first_value integer,\n\n    -- keep this parameter group\n    second_value integer DEFAULT 2\n) RETURNS integer LANGUAGE SQL RETURN first_value + second_value;";
    let result = semblock::format_sql(source, &semblock::FormatOptions::default()).unwrap();
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.fix_available),
        "{:?}",
        result.diagnostics
    );
    assert!(
        result
            .output
            .contains("first_value integer,\n\n    -- keep this parameter group\n    second_value")
    );
    semblock::validate_equivalent(source, &result.output).unwrap();
    assert_sql_layout_only(&result.output, &result.output);
}
