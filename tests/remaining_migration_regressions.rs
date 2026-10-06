mod support;

fn assert_supported(source: &str) {
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
            .lines()
            .all(|line| line.chars().count() <= 160)
    );
    support::assert_sql_layout_only(&result.output, &result.output);
}

#[test]
fn identity_sequence_options_have_their_own_layout() {
    let source = "ALTER TABLE sample_schema.sample_rows ALTER COLUMN id ADD GENERATED ALWAYS AS IDENTITY (\n    SEQUENCE NAME sample_schema.sample_rows_identifier_sequence\n    START WITH 1\n    INCREMENT BY 1\n    NO MINVALUE\n    NO MAXVALUE\n    CACHE 1\n);";
    assert_supported(source);
}

#[test]
fn array_subquery_targets_bind_their_select_ownership() {
    assert_supported(
        "CREATE FUNCTION sample_array(input_value jsonb) RETURNS text[] LANGUAGE SQL BEGIN ATOMIC\nSELECT CASE WHEN input_value = 'null'::jsonb THEN NULL::text[] ELSE ARRAY(SELECT value FROM jsonb_array_elements_text(input_value) AS source(value)) END;\nEND;",
    );
}

#[test]
fn composite_function_relation_definitions_bind_with_cte_queries() {
    assert_supported(
        "WITH seed AS MATERIALIZED (SELECT source.id, source.label FROM jsonb_to_recordset('[]'::jsonb) AS source(id integer, label text)) SELECT id FROM seed;",
    );
}

#[test]
fn nested_function_arguments_keep_breakable_lines_within_width() {
    let expression = (0..20).fold("input_value".to_owned(), |value, index| {
        format!("replace(\n{value},\n'old_{index}', 'new_{index}')")
    });
    assert_supported(&format!(
        "CREATE FUNCTION sample_replace(input_value text) RETURNS text LANGUAGE SQL AS $$\nSELECT {expression};\n$$;"
    ));
}

#[test]
fn procedural_parameter_type_references_preserve_semantics() {
    assert_supported(
        "CREATE FUNCTION sample_stream(anyarray) RETURNS SETOF anyarray LANGUAGE plpgsql AS $$\nDECLARE\n    part $1%TYPE;\nBEGIN\n    FOREACH part SLICE 1 IN ARRAY $1 LOOP\n        RETURN NEXT part;\n    END LOOP;\n    RETURN;\nEND;\n$$;",
    );
}

#[test]
fn procedural_into_targets_are_separate_from_sql_tables() {
    assert_supported(
        "DO $$\nDECLARE\n    sample_id integer;\n    sample_label text;\nBEGIN\n    SELECT id FROM sample_rows WHERE enabled INTO STRICT sample_id;\n    SELECT id, label INTO sample_id, sample_label FROM sample_rows;\n    INSERT INTO sample_rows (id) VALUES (1) RETURNING id INTO sample_id;\nEND;\n$$;",
    );
}

#[test]
fn procedural_transaction_nodes_have_reviewed_ownership() {
    assert_supported(
        "CREATE PROCEDURE sample_work() LANGUAGE plpgsql AS $$\nBEGIN\n    COMMIT;\n    ROLLBACK;\n    COMMIT AND CHAIN;\n    ROLLBACK AND NO CHAIN;\nEND;\n$$;",
    );
}

#[test]
fn values_relation_order_and_limit_suffixes_are_owned() {
    assert_supported(
        "SELECT source.id FROM (VALUES (3), (1), (2) ORDER BY 1 LIMIT 2 OFFSET 1) AS source(id);",
    );
}

#[test]
fn external_language_declarations_preserve_body_literals() {
    assert_supported(
        "CREATE FUNCTION sample_extension(integer) RETURNS text LANGUAGE C IMMUTABLE STRICT AS '$libdir/sample_extension', 'sample_entry';",
    );
}
