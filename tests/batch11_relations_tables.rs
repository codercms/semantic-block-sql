use pretty_assertions::assert_eq;
use semblock::{FormatOptions, check_sql, format_sql, format_sql_result, validate_equivalent};

fn assert_format(source: &str, expected: &str) {
    let options = FormatOptions::default();
    let formatted = format_sql(source, &options).expect("format succeeds");
    assert_eq!(formatted.output, expected);
    assert!(
        formatted.warnings.is_empty(),
        "warnings: {:?}",
        formatted.warnings
    );
    validate_equivalent(source, expected).expect("semantic equivalence");
    assert_eq!(
        format_sql(expected, &options)
            .expect("second format succeeds")
            .output,
        expected,
        "formatting must be idempotent",
    );
    let checked = check_sql(expected, &options);
    assert!(checked.compliant, "diagnostics: {:?}", checked.diagnostics);
}

#[test]
fn formats_rows_from_tablesample_and_relation_column_lists() {
    assert_format(
        "select * from rows from (jsonb_each_text(payload) as (entry_key text,entry_value text),generate_series(1,3)) as rf(entry_key,entry_value,n);",
        "SELECT *\nFROM ROWS FROM (jsonb_each_text(payload) AS (entry_key text, entry_value text), generate_series(1, 3)) AS rf(entry_key, entry_value, n);",
    );
    assert_format(
        "select * from items as i(id,title) tablesample system(10) repeatable(42);",
        "SELECT *\nFROM items AS i(id, title) TABLESAMPLE system (10) REPEATABLE (42);",
    );
    assert_format(
        "select * from json_to_record(payload) as item(id bigint,title text);",
        "SELECT *\nFROM json_to_record(payload) AS item(id bigint, title text);",
    );
    assert_format(
        "merge into items target using lateral jsonb_each_text(target.payload) as source(key,value) on true when matched then delete;",
        "MERGE INTO items target\nUSING LATERAL jsonb_each_text(target.payload) AS source (KEY, value)\nON TRUE\n\nWHEN MATCHED THEN DELETE;",
    );
}

#[test]
fn formats_derived_with_queries_and_alias_column_lists() {
    assert_format(
        "select * from (with recent as (select id from items) select id from recent) as src(id);",
        "SELECT *\nFROM (WITH recent AS (SELECT id FROM items) SELECT id FROM recent) AS src(id);",
    );
    assert_format(
        "update items target set title=source.title from (with recent as (select id,title from staging) select id,title from recent) as source(id,title) where target.id=source.id;",
        "UPDATE items target\nSET\n    title = source.title\nFROM (WITH recent AS (\n        SELECT id, title\n        FROM staging\n    )\n    SELECT id, title\n    FROM recent\n) AS source (id, title)\nWHERE target.id = source.id;",
    );
}

#[test]
fn formats_partitioned_inherited_and_typed_tables() {
    assert_format(
        "create table events (id bigint,created_at timestamptz) partition by range(created_at) using heap with(fillfactor=80) tablespace fastspace;",
        "CREATE TABLE events (\n    id bigint,\n    created_at timestamptz\n)\nPARTITION BY RANGE (created_at)\nUSING heap\nWITH (fillfactor = 80)\nTABLESPACE fastspace;",
    );
    assert_format(
        "create table child (extra text) inherits(parent_a,parent_b) with(fillfactor=70) on commit preserve rows tablespace fastspace;",
        "CREATE TABLE child (\n    extra text\n)\nINHERITS (parent_a, parent_b)\nWITH (fillfactor = 70)\nON COMMIT PRESERVE ROWS\nTABLESPACE fastspace;",
    );
    assert_format(
        "create table typed_items of public.item_type (id with options not null,title with options default 'untitled') using heap;",
        "CREATE TABLE typed_items OF public.item_type (\n    id WITH OPTIONS NOT NULL,\n    title WITH OPTIONS DEFAULT 'untitled'\n)\nUSING heap;",
    );
}

#[test]
fn formats_every_partition_bound_strategy() {
    assert_format(
        "create table events_2026 partition of events for values from ('2026-01-01') to ('2027-01-01') tablespace fastspace;",
        "CREATE TABLE events_2026 PARTITION OF events\nFOR VALUES FROM ('2026-01-01') TO ('2027-01-01')\nTABLESPACE fastspace;",
    );
    assert_format(
        "create table events_us partition of events for values in ('us','ca');",
        "CREATE TABLE events_us PARTITION OF events\nFOR VALUES IN ('us', 'ca');",
    );
    assert_format(
        "create table events_h0 partition of events for values with (modulus 4,remainder 0);",
        "CREATE TABLE events_h0 PARTITION OF events\nFOR VALUES WITH (modulus 4, remainder 0);",
    );
    assert_format(
        "create table events_default partition of events default;",
        "CREATE TABLE events_default PARTITION OF events DEFAULT;",
    );
    assert_format(
        "create table list_events (region text) partition by list(region);",
        "CREATE TABLE list_events (\n    region text\n)\nPARTITION BY LIST (region);",
    );
    assert_format(
        "create table hash_events (tenant_id bigint) partition by hash(tenant_id);",
        "CREATE TABLE hash_events (\n    tenant_id bigint\n)\nPARTITION BY HASH (tenant_id);",
    );
}

#[test]
fn comments_and_literals_survive_the_new_boundaries() {
    let source = "SELECT *\nFROM items AS item(id, title) TABLESAMPLE system (10) -- stable sample\nREPEATABLE (42);\n\nCREATE TABLE events_2026 PARTITION OF events\nFOR VALUES FROM ('2026-01-01') TO ('2027-01-01') -- literal bounds\nTABLESPACE fastspace;\n";
    assert_format(source, source);
}

#[test]
fn unreviewed_relation_and_table_neighbors_remain_fail_safe() {
    for source in [
        "SELECT * FROM XMLTABLE('/items/item' PASSING payload COLUMNS id bigint PATH '@id') AS item;",
        "CREATE TABLE copied (LIKE source INCLUDING ALL);",
        "CREATE TABLE copied AS SELECT id FROM items;",
    ] {
        let result = format_sql_result(source, &FormatOptions::default());
        assert_eq!(result.output, source, "{source}");
        assert!(!result.changed, "{source}");
        assert!(
            result.diagnostics.iter().any(|diagnostic| {
                matches!(
                    diagnostic.rule_id.as_str(),
                    "syntax.unsupported" | "syntax.parse_failure"
                )
            }),
            "{source}: {:?}",
            result.diagnostics,
        );
    }
}
