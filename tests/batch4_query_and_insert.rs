mod support;

use semblock::FormatOptions;
use support::assert_sql_with as assert_format;

#[test]
fn supports_insert_overriding_and_default_values() {
    let options = FormatOptions::default();
    assert_format(
        "insert into items (id) overriding system value values (1);",
        "INSERT INTO items (id) OVERRIDING SYSTEM VALUE VALUES (1);",
        &options,
    );
    assert_format(
        "insert into items default values;",
        "INSERT INTO items DEFAULT VALUES;",
        &options,
    );
}

#[test]
fn formats_insert_select_and_returning_as_owned_sibling_clauses() {
    let source = "insert into archive.items (id, title) select item.id,item.title from public.items item where item.deleted_at is null returning id;";
    let options = FormatOptions {
        soft_line_width: 72,
        hard_line_width: 96,
        ..FormatOptions::default()
    };
    let expected = "INSERT INTO archive.items (id, title)\nSELECT item.id, item.title\nFROM public.items item\nWHERE item.deleted_at IS NULL\nRETURNING id;";

    assert_format(source, expected, &options);
}

#[test]
fn shares_with_ownership_between_ctes_and_insert_select() {
    let source = "with source as (select id,title from staging.items) insert into public.items (id,title) select source.id,source.title from source returning id;";
    let expected = "WITH source AS (\n    SELECT id, title\n    FROM staging.items\n)\nINSERT INTO public.items (id, title)\nSELECT source.id, source.title\nFROM source\nRETURNING id;";

    assert_format(source, expected, &FormatOptions::default());
}

#[test]
fn formats_distinct_group_having_order_limit_and_offset() {
    let source = "select distinct item.category,count(*) as item_count from public.items item where item.deleted_at is null group by item.category having count(*) > 1 order by item.category limit 10 offset 2;";
    let options = FormatOptions {
        soft_line_width: 72,
        hard_line_width: 96,
        ..FormatOptions::default()
    };
    let expected = "SELECT DISTINCT item.category, COUNT(*) AS item_count\nFROM public.items item\nWHERE item.deleted_at IS NULL\nGROUP BY item.category\nHAVING COUNT(*) > 1\nORDER BY item.category\nLIMIT 10\nOFFSET 2;";

    assert_format(source, expected, &options);
}

#[test]
fn formats_fetch_and_general_set_operations() {
    assert_format(
        "select id from items order by id fetch first 5 rows only;",
        "SELECT id FROM items ORDER BY id FETCH FIRST 5 ROWS ONLY;",
        &FormatOptions::default(),
    );

    assert_format(
        "select 1 as value union all select 2 as value intersect select 2 as value;",
        "SELECT 1 AS value\n\nUNION ALL\n\nSELECT 2 AS value\n\nINTERSECT\n\nSELECT 2 AS value;",
        &FormatOptions::default(),
    );
}

#[test]
fn set_operation_branches_own_their_from_clauses() {
    assert_format(
        "select id, created_at from active_items where ready = true union all select id, created_at from archived_items where ready = true order by created_at desc;",
        "SELECT id, created_at FROM active_items WHERE ready = TRUE\n\nUNION ALL\n\nSELECT id, created_at FROM archived_items WHERE ready = TRUE\nORDER BY created_at DESC;",
        &FormatOptions::default(),
    );
}

#[test]
fn set_operation_branches_own_their_named_window_clauses() {
    assert_format(
        "select row_number() over w from active_items window w as (order by id) union all select row_number() over w from archived_items window w as (order by id);",
        "SELECT row_number() OVER w FROM active_items WINDOW w AS (ORDER BY id)\n\nUNION ALL\n\nSELECT row_number() OVER w FROM archived_items WINDOW w AS (ORDER BY id);",
        &FormatOptions::default(),
    );
}

#[test]
fn select_expressions_do_not_treat_is_distinct_from_as_a_from_clause() {
    assert_format(
        "select old_value is distinct from new_value as changed;",
        "SELECT old_value IS DISTINCT FROM new_value AS changed;",
        &FormatOptions::default(),
    );
    assert_format(
        "select item.old_value is distinct from item.new_value as changed from items item;",
        "SELECT item.old_value IS DISTINCT FROM item.new_value AS changed FROM items item;",
        &FormatOptions::default(),
    );
    assert_format(
        "select 1 where old_value is not distinct from new_value;",
        "SELECT 1 WHERE old_value IS NOT DISTINCT FROM new_value;",
        &FormatOptions::default(),
    );
}

#[test]
fn shares_with_ownership_with_update_and_delete() {
    assert_format(
        "with source as (select id,title from staging.items) update public.items set title=source.title from source where public.items.id=source.id returning public.items.id;",
        "WITH source AS (
    SELECT id, title
    FROM staging.items
)
UPDATE public.items
SET title = source.title
FROM source
WHERE public.items.id = source.id
RETURNING public.items.id;",
        &FormatOptions::default(),
    );

    assert_format(
        "with stale as (select id from staging.stale_items) delete from public.items using stale where public.items.id=stale.id returning public.items.id;",
        "WITH stale AS (
    SELECT id
    FROM staging.stale_items
)
DELETE FROM public.items
USING stale
WHERE public.items.id = stale.id
RETURNING public.items.id;",
        &FormatOptions::default(),
    );
}
