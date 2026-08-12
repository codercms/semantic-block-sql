mod support;

use semblock::FormatOptions;
use support::{SqlCase, assert_cases, assert_sql_with as assert_format};

#[test]
fn formats_top_level_values_as_owned_rows() {
    assert_format(
        "values (1,'one'),(2,'two'),(3,'three');",
        "VALUES\n    (1, 'one'),\n    (2, 'two'),\n    (3, 'three');",
        &FormatOptions::default(),
    );

    assert_format(
        "values (1, 'one');",
        "VALUES (1, 'one');",
        &FormatOptions::default(),
    );
}

#[test]
fn formats_filtered_ordered_and_window_aggregates() {
    let source = "select department_id,sum(amount order by created_at) filter(where status='paid') over(partition by department_id order by created_at rows between unbounded preceding and current row exclude ties) as running_total from payments window recent as(partition by department_id order by created_at);";
    let expected = "SELECT\n    department_id,\n    SUM(amount ORDER BY created_at) FILTER (WHERE status = 'paid') OVER (\n        PARTITION BY department_id\n        ORDER BY created_at\n        ROWS BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW EXCLUDE TIES\n    ) AS running_total\nFROM payments\nWINDOW recent AS (\n    PARTITION BY department_id\n    ORDER BY created_at\n);";

    assert_format(source, expected, &FormatOptions::default());
}

#[test]
fn preserves_nested_query_and_window_indentation() {
    assert_cases(&[
        SqlCase::new(
            "CTE query",
            "WITH numbered_offers AS (\n    SELECT\n        ROW_NUMBER() OVER (PARTITION BY o.seller_id ORDER BY o.dt)::VARCHAR AS id,\n        o.doc_dt\n    FROM offers o\n)\nSELECT * FROM numbered_offers;",
            "WITH numbered_offers AS (\n    SELECT\n        row_number() OVER (\n            PARTITION BY o.seller_id\n            ORDER BY o.dt\n        )::varchar AS id,\n        o.doc_dt\n    FROM offers o\n)\nSELECT *\nFROM numbered_offers;",
        ),
        SqlCase::new(
            "derived relation query",
            "SELECT *\nFROM (\n    SELECT\n        ROW_NUMBER() OVER (PARTITION BY o.seller_id ORDER BY o.dt)::VARCHAR AS id,\n        o.doc_dt\n    FROM offers o\n) numbered_offers;",
            "SELECT *\nFROM (\n    SELECT\n        row_number() OVER (\n            PARTITION BY o.seller_id\n            ORDER BY o.dt\n        )::varchar AS id,\n        o.doc_dt\n    FROM offers o\n) numbered_offers;",
        ),
        SqlCase::new(
            "scalar subquery",
            "SELECT\n    (\n        SELECT\n            ROW_NUMBER() OVER (PARTITION BY o.seller_id ORDER BY o.dt)::VARCHAR\n        FROM offers o\n        LIMIT 1\n    ) AS id,\n    CURRENT_DATE;",
            "SELECT\n    (\n        SELECT\n            row_number() OVER (\n                PARTITION BY o.seller_id\n                ORDER BY o.dt\n            )::varchar\n        FROM offers o\n        LIMIT 1\n    ) AS id,\n    CURRENT_DATE;",
        ),
        SqlCase::new(
            "nested derived relation in CTE",
            "WITH numbered_offers AS (\n    SELECT *\n    FROM (\n        SELECT\n            ROW_NUMBER() OVER (PARTITION BY o.seller_id ORDER BY o.dt)::VARCHAR AS id,\n            o.doc_dt\n        FROM offers o\n    ) ranked\n)\nSELECT * FROM numbered_offers;",
            "WITH numbered_offers AS (\n    SELECT *\n    FROM (\n        SELECT\n            row_number() OVER (\n                PARTITION BY o.seller_id\n                ORDER BY o.dt\n            )::varchar AS id,\n            o.doc_dt\n        FROM offers o\n    ) ranked\n)\nSELECT *\nFROM numbered_offers;",
        ),
    ]);
}

#[test]
fn formats_lateral_derived_tables_with_owned_query_wrappers() {
    let source = "select item.id,latest.title from items item left join lateral (select title from titles where titles.item_id=item.id order by created_at desc limit 1) latest on true;";
    let expected = "SELECT item.id, latest.title\nFROM items item\nLEFT JOIN LATERAL (\n    SELECT title\n    FROM titles\n    WHERE titles.item_id = item.id\n    ORDER BY created_at DESC\n    LIMIT 1\n) latest ON TRUE;";
    let options = FormatOptions {
        soft_line_width: 72,
        hard_line_width: 96,
        ..FormatOptions::default()
    };

    assert_format(source, expected, &options);
}

#[test]
fn supports_lateral_functions_and_preserves_alias_identifiers() {
    assert_format(
        "select * from lateral generate_series(1,10) with ordinality as value(n,ordinality);",
        "SELECT *
FROM LATERAL generate_series(1, 10) WITH ORDINALITY AS value (n, ordinality);",
        &FormatOptions::default(),
    );
}

#[test]
fn supports_window_functions_inside_insert_select() {
    assert_format(
        "insert into public.items (id) select row_number() over () from staging.items;",
        "INSERT INTO public.items (id)\nSELECT row_number() OVER ()\nFROM staging.items;",
        &FormatOptions::default(),
    );
}

#[test]
fn supports_nested_data_modifying_ctes_in_derived_relations() {
    assert_format(
        "SELECT * FROM LATERAL (WITH moved AS (DELETE FROM items RETURNING id) SELECT * FROM moved) source;",
        "SELECT *
FROM LATERAL (WITH moved AS (DELETE FROM items RETURNING id) SELECT * FROM moved) source;",
        &FormatOptions::default(),
    );
}
