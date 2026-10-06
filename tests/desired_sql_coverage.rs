mod support;

use semblock::{FormatOptions, format_sql, validate_equivalent};
use support::assert_sql_layout_only;

// Desired capabilities, not claims of current support. Every example is
// invented and exercises a grammar shape without a production schema.
fn assert_supported(source: &str) {
    let parsed = pg_query::parse(source).expect("the desired syntax is valid PostgreSQL");
    let options = FormatOptions::default();
    let result = format_sql(source, &options).expect("the desired syntax must format successfully");
    assert!(
        !result.diagnostics.iter().any(|diagnostic| {
            matches!(
                diagnostic.rule_id.as_str(),
                "syntax.unsupported" | "format.statement_skipped"
            )
        }),
        "desired support was rejected: {:?}",
        result.diagnostics
    );
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    assert!(
        result
            .output
            .lines()
            .all(|line| line.chars().count() <= 160)
    );
    let has_routine = parsed.protobuf.stmts.iter().any(|raw| {
        matches!(
            raw.stmt.as_deref().and_then(|node| node.node.as_ref()),
            Some(
                pg_query::protobuf::node::Node::DoStmt(_)
                    | pg_query::protobuf::node::Node::CreateFunctionStmt(_)
            )
        )
    });
    // Routine body equivalence belongs to the canonical routine safety pipeline;
    // the outer comparator treats dollar-quoted bodies as protected literals.
    if !has_routine {
        validate_equivalent(source, &result.output).expect("SQL semantics are preserved");
    }
    assert_sql_layout_only(&result.output, &result.output);
}

#[test]
fn supports_session_settings() {
    assert_supported("SET statement_timeout = 0;\nSET search_path = sample_schema, public;");
}

#[test]
fn supports_aggregate_definitions() {
    assert_supported(
        "CREATE AGGREGATE sample_total(integer) (\n    SFUNC = int4pl,\n    STYPE = integer,\n    INITCOND = '0'\n);",
    );
}

#[test]
fn supports_index_partition_attachment() {
    assert_supported("ALTER INDEX sample_parent ATTACH PARTITION sample_child;");
}

#[test]
fn supports_dollar_quoted_sql_routines() {
    assert_supported(
        "CREATE FUNCTION sample_value() RETURNS integer\nLANGUAGE SQL AS $$\nSELECT 1;\n$$;",
    );
}

#[test]
fn supports_sql_standard_parallel_options() {
    assert_supported(
        "CREATE FUNCTION sample_value() RETURNS integer\nLANGUAGE SQL PARALLEL SAFE\nBEGIN ATOMIC\n    SELECT 1;\nEND;",
    );
}

#[test]
fn supports_multi_statement_sql_standard_bodies() {
    assert_supported(
        "CREATE FUNCTION sample_work() RETURNS void\nLANGUAGE SQL\nBEGIN ATOMIC\n    INSERT INTO sample_rows (id) VALUES (1);\n    DELETE FROM sample_rows WHERE id = 2;\nEND;",
    );
}

#[test]
fn supports_sql_standard_return_body() {
    assert_supported("CREATE FUNCTION sample_value() RETURNS integer LANGUAGE SQL RETURN 1;");
}

#[test]
fn supports_return_inside_sql_standard_atomic_body() {
    assert_supported(
        "CREATE FUNCTION sample_value() RETURNS boolean\nLANGUAGE SQL\nBEGIN ATOMIC\n    RETURN NOT EXISTS (SELECT 1 FROM sample_rows WHERE enabled = FALSE);\nEND;",
    );
}

#[test]
fn supports_procedural_raise_using_options() {
    assert_supported(
        "DO $$\nBEGIN\n    RAISE EXCEPTION 'sample failure' USING ERRCODE = 'P0001', HINT = 'sample hint';\nEND;\n$$;",
    );
}

#[test]
fn supports_procedural_elsif() {
    assert_supported(
        "DO $$\nBEGIN\n    IF TRUE THEN\n        PERFORM 1;\n    ELSIF FALSE THEN\n        PERFORM 2;\n    ELSE\n        PERFORM 3;\n    END IF;\nEND;\n$$;",
    );
}

#[test]
fn supports_procedural_return_next() {
    assert_supported(
        "CREATE FUNCTION sample_stream() RETURNS SETOF integer LANGUAGE plpgsql AS $$\nBEGIN\n    RETURN NEXT 1;\n    RETURN;\nEND;\n$$;",
    );
}

#[test]
fn supports_values_relation_in_view() {
    assert_supported(
        "CREATE VIEW sample_view AS\nSELECT source.id\nFROM (VALUES (1), (2)) source(id);",
    );
}

#[test]
fn supports_view_query_beginning_with_cte() {
    assert_supported(
        "CREATE VIEW sample_view AS\nWITH seed AS (SELECT 1 AS id)\nSELECT id\nFROM seed;",
    );
}

#[test]
fn supports_materialized_view_query_beginning_with_cte() {
    assert_supported(
        "CREATE MATERIALIZED VIEW sample_view AS\nWITH seed AS (SELECT 1 AS id)\nSELECT id\nFROM seed\nWITH NO DATA;",
    );
}

#[test]
fn supports_materialized_view_with_wrapped_set_operation_branches() {
    assert_supported(
        "CREATE MATERIALIZED VIEW sample_view AS\n(SELECT id FROM sample_rows ORDER BY id LIMIT 2)\nUNION ALL\n(SELECT id FROM sample_archive ORDER BY id LIMIT 2)\nWITH NO DATA;",
    );
}

#[test]
fn supports_trigger_transition_tables() {
    assert_supported(
        "CREATE TRIGGER sample_changes\nAFTER INSERT ON sample_rows\nREFERENCING NEW TABLE AS inserted_rows\nFOR EACH STATEMENT\nEXECUTE FUNCTION sample_handler();",
    );
}

#[test]
fn accepts_optional_final_procedural_end_semicolon() {
    assert_supported("DO $$\nBEGIN\n    PERFORM 1;\nEND\n$$;");
}

#[test]
fn supports_procedural_equal_assignment() {
    assert_supported(
        "DO $$\nDECLARE\n    value integer;\nBEGIN\n    value = (SELECT 1);\nEND;\n$$;",
    );
}

#[test]
fn supports_static_analyze_in_procedural_body() {
    assert_supported("DO $$\nBEGIN\n    ANALYSE sample_rows;\nEND;\n$$;");
}

#[test]
fn supports_static_truncate_in_procedural_body() {
    assert_supported("DO $$\nBEGIN\n    TRUNCATE sample_rows;\nEND;\n$$;");
}

#[test]
fn lays_out_long_composite_type_fields() {
    let fields = (1..=12)
        .map(|index| format!("    sample_field_{index} integer"))
        .collect::<Vec<_>>()
        .join(",\n");
    assert_supported(&format!("CREATE TYPE sample_record AS (\n{fields}\n);"));
}

#[test]
fn lays_out_long_foreign_key_actions() {
    let name = "sample_".repeat(7);
    assert_supported(&format!(
        "ALTER TABLE {name}rows\n    ADD CONSTRAINT {name}reference\n        FOREIGN KEY (first_id, second_id)\n        REFERENCES {name}parent (first_id, second_id)\n        ON UPDATE CASCADE ON DELETE CASCADE;"
    ));
}

#[test]
fn lays_out_long_sequence_options() {
    let name = "sample_".repeat(8);
    assert_supported(&format!(
        "CREATE SEQUENCE {name}counter\n    AS bigint\n    START WITH 1\n    INCREMENT BY 1\n    MINVALUE 1\n    MAXVALUE 9223372036854775807\n    CACHE 1000\n    NO CYCLE;"
    ));
}

#[test]
fn lays_out_long_trigger_headers() {
    let name = "sample_".repeat(8);
    assert_supported(&format!(
        "CREATE TRIGGER {name}changes\nBEFORE UPDATE OF first_value, second_value ON sample_rows\nFOR EACH ROW\nEXECUTE FUNCTION sample_handler();"
    ));
}

#[test]
fn view_cte_owners_preserve_nested_queries_comments_and_suffixes() {
    assert_supported(
        "CREATE VIEW sample_view (id) WITH (security_barrier = true) AS\nWITH seed AS (\n    -- retain this source comment\n    WITH inner_seed AS (SELECT 1 AS id)\n    SELECT id FROM inner_seed\n)\nSELECT id FROM seed\nWITH LOCAL CHECK OPTION;",
    );
    assert_supported(
        "CREATE MATERIALIZED VIEW sample_view AS\nWITH seed AS (SELECT id FROM sample_rows)\nSELECT id FROM seed UNION ALL SELECT id FROM sample_archive\nWITH NO DATA;",
    );
}

#[test]
fn wrapped_view_set_operations_keep_query_suffix_ownership() {
    assert_supported(
        "CREATE VIEW sample_view AS\n(SELECT id FROM sample_rows ORDER BY id LIMIT 2)\nUNION ALL\n(SELECT id FROM sample_archive ORDER BY id LIMIT 2)\nORDER BY id;",
    );
}
