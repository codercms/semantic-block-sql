use pretty_assertions::assert_eq;
use semblock::{FormatOptions, check_sql, format_sql, validate_equivalent};

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
