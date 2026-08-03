use pretty_assertions::assert_eq;
use semblock::{FormatOptions, check_sql, format_sql, format_sql_result, validate_equivalent};

fn assert_format(source: &str, expected: &str, options: &FormatOptions) {
    let formatted = format_sql(source, options).expect("format succeeds");
    assert_eq!(formatted.output, expected);
    validate_equivalent(source, expected).expect("semantic equivalence");
    assert_eq!(
        format_sql(expected, options)
            .expect("second format succeeds")
            .output,
        expected,
        "formatting must be idempotent"
    );
    let checked = check_sql(expected, options);
    assert!(
        checked.compliant,
        "formatted SQL must pass check: {:?}",
        checked.diagnostics
    );
}

#[test]
fn formats_compact_insert_values_returning() {
    assert_format(
        "insert into public.items (id,title) values ($1,$2) returning id;",
        "INSERT INTO public.items (id, title) VALUES ($1, $2) RETURNING id;",
        &FormatOptions::default(),
    );
}

#[test]
fn formats_authored_multirow_insert_without_losing_groups() {
    assert_format(
        include_str!("fixtures/batch3/insert-values.input.sql"),
        include_str!("fixtures/batch3/insert-values.expected.sql"),
        &FormatOptions::default(),
    );
}

#[test]
fn long_ungrouped_insert_lists_expand_one_item_per_line() {
    let source = "insert into public.items (first_identifier, second_identifier, third_identifier) values ($1, $2, $3) returning first_identifier, second_identifier;";
    let options = FormatOptions {
        soft_line_width: 64,
        hard_line_width: 80,
        ..FormatOptions::default()
    };
    let expected = "INSERT INTO public.items (\n    first_identifier,\n    second_identifier,\n    third_identifier\n)\nVALUES ($1, $2, $3)\nRETURNING\n    first_identifier,\n    second_identifier;";

    assert_format(source, expected, &options);
}

#[test]
fn insert_supports_data_modifying_ctes_and_locked_query_sources() {
    assert_format(
        "WITH changed AS (DELETE FROM staging.items RETURNING id) INSERT INTO public.items (id) SELECT id FROM changed;",
        "WITH changed AS (
    DELETE FROM staging.items RETURNING id
)
INSERT INTO public.items (id)
SELECT id
FROM changed;",
        &FormatOptions::default(),
    );
    assert_format(
        "INSERT INTO public.items (id) SELECT id FROM staging.items FOR UPDATE;",
        "INSERT INTO public.items (id)
SELECT id
FROM staging.items
FOR UPDATE;",
        &FormatOptions::default(),
    );
}

#[test]
fn short_insert_target_list_ignores_leading_cte_comments() {
    assert_format(
        "WITH accepted AS (\n    -- Deduplicate using the external event identifier before applying the payload.\n    -- Repeated events must not modify any downstream records.\n    INSERT INTO public.bb_events (event_kind, event_id, entity_uuid, payload )\n    VALUES ('content_batch', $1::text, $2::uuid, $3::jsonb)\n    ON CONFLICT (event_id) DO NOTHING\n    RETURNING entity_uuid\n)\nSELECT entity_uuid FROM accepted;",
        "WITH accepted AS (\n    -- Deduplicate using the external event identifier before applying the payload.\n    -- Repeated events must not modify any downstream records.\n    INSERT INTO public.bb_events (event_kind, event_id, entity_uuid, payload)\n    VALUES ('content_batch', $1::text, $2::uuid, $3::jsonb)\n    ON CONFLICT (event_id) DO NOTHING\n    RETURNING entity_uuid\n)\nSELECT entity_uuid\nFROM accepted;",
        &FormatOptions::default(),
    );
}

#[test]
fn unsupported_insert_variants_remain_unchanged() {
    let source = "INSERT INTO public.items (payload) VALUES (JSON_QUERY(payload, '$.a'));";
    let formatted = format_sql_result(source, &FormatOptions::default());
    assert_eq!(formatted.output, source);
    assert!(!formatted.changed);
    assert_eq!(formatted.diagnostics.len(), 1);
    assert_eq!(formatted.diagnostics[0].rule_id, "syntax.unsupported");
}

#[test]
fn complex_values_rows_expand_independently() {
    let source = "insert into items (id, value) values (1, coalesce($1, $2, $3, $4)), (2, $5);";
    let options = FormatOptions {
        soft_line_width: 35,
        hard_line_width: 80,
        ..FormatOptions::default()
    };
    let expected = "INSERT INTO items (id, value)\nVALUES\n    (\n        1,\n        COALESCE($1, $2, $3, $4)\n    ),\n    (2, $5);";

    let checked = check_sql(source, &options);
    assert!(
        checked
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.rule_id == "layout.values")
    );
    assert_format(source, expected, &options);
}

#[test]
fn insert_comments_remain_attached_byte_for_byte() {
    let source = "insert into public.items (\n    id, -- identifier\n    title\n)\nvalues\n    (1, 'one'), -- first row\n    (2, 'two')\nreturning id;\n";
    let expected = "INSERT INTO public.items (\n    id, -- identifier\n    title\n)\nVALUES\n    (1, 'one'), -- first row\n    (2, 'two')\nRETURNING id;\n";

    assert_format(source, expected, &FormatOptions::default());
}

#[test]
fn insert_without_an_explicit_column_list_stays_compact() {
    assert_format(
        "insert into public.items values (1, 'one');",
        "INSERT INTO public.items VALUES (1, 'one');",
        &FormatOptions::default(),
    );
}
