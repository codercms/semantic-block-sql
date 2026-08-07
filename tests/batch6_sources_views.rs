mod support;

use pretty_assertions::assert_eq;
use semblock::{FormatOptions, format_sql_result};
use support::assert_sql as assert_format;

#[test]
fn formats_multiple_update_and_delete_sources() {
    assert_format(
        "update public.items i set title=s.title,updated_at=now() from public.source_items s, public.batches b where s.item_id=i.id and b.id=s.batch_id returning i.id;",
        "UPDATE public.items i\nSET title = s.title, updated_at = NOW()\nFROM\n    public.source_items s,\n    public.batches b\nWHERE s.item_id = i.id AND b.id = s.batch_id\nRETURNING i.id;",
    );

    assert_format(
        "delete from public.items i using public.source_items s, public.batches b where i.id=s.item_id and b.id=s.batch_id;",
        "DELETE FROM public.items i\nUSING\n    public.source_items s,\n    public.batches b\nWHERE i.id = s.item_id AND b.id = s.batch_id;",
    );
}

#[test]
fn formats_joined_dml_sources_and_owns_join_predicates() {
    assert_format(
        "update public.items i set title=s.title from public.source_items s join public.batches b on b.id=s.batch_id where i.id=s.item_id;",
        "UPDATE public.items i\nSET title = s.title\nFROM public.source_items s\nJOIN public.batches b ON b.id = s.batch_id\nWHERE i.id = s.item_id;",
    );

    assert_format(
        "delete from public.items i using public.source_items s left join public.batches b using(batch_id) where i.id=s.item_id;",
        "DELETE FROM public.items i\nUSING public.source_items s\nLEFT JOIN public.batches b USING (batch_id)\nWHERE i.id = s.item_id;",
    );

    assert_format(
        "update items i set title=s.title from source_items s cross join batches b where i.id=s.item_id;",
        "UPDATE items i\nSET title = s.title\nFROM source_items s\nCROSS JOIN batches b\nWHERE i.id = s.item_id;",
    );

    assert_format(
        "delete from items i using source_items s natural left join batches b where i.id=s.item_id;",
        "DELETE FROM items i\nUSING source_items s\nNATURAL LEFT JOIN batches b\nWHERE i.id = s.item_id;",
    );

    assert_format(
        "update items i set title=s.title from source_items s right join batches b using(id,batch_id) where i.id=s.item_id;",
        "UPDATE items i\nSET title = s.title\nFROM source_items s\nRIGHT JOIN batches b USING (id, batch_id)\nWHERE i.id = s.item_id;",
    );

    assert_format(
        "delete from items i using source_items s full join batches b on b.id=s.batch_id where i.id=s.item_id;",
        "DELETE FROM items i\nUSING source_items s\nFULL JOIN batches b ON b.id = s.batch_id\nWHERE i.id = s.item_id;",
    );
}

#[test]
fn formats_derived_and_function_dml_sources() {
    assert_format(
        "delete from public.items i using (select id from staging.items where active) s where i.id=s.id;",
        "DELETE FROM public.items i\nUSING (\n    SELECT id\n    FROM staging.items\n    WHERE active\n) s\nWHERE i.id = s.id;",
    );

    assert_format(
        "update public.items i set title=f.value from lateral jsonb_each_text(i.payload) with ordinality as f where f.key='title';",
        "UPDATE public.items i\nSET title = f.value\nFROM LATERAL jsonb_each_text(i.payload) WITH ORDINALITY AS f\nWHERE f.key = 'title';",
    );

    assert_format(
        "update public.items i set title=s.title from (select id,title,batch_id from staging.items where active) s join public.batches b on b.id=s.batch_id where i.id=s.id;",
        "UPDATE public.items i\nSET title = s.title\nFROM (\n    SELECT id, title, batch_id\n    FROM staging.items\n    WHERE active\n) s\nJOIN public.batches b ON b.id = s.batch_id\nWHERE i.id = s.id;",
    );

    assert_format(
        "update public.items i set title=s.title from (public.source_items s join public.batches b on b.id=s.batch_id) source where i.id=s.item_id;",
        "UPDATE public.items i\nSET title = s.title\nFROM (\n    public.source_items s\n    JOIN public.batches b ON b.id = s.batch_id\n) source\nWHERE i.id = s.item_id;",
    );

    assert_format(
        "update public.items i set title=s.title from (select s.id,s.title from staging.items s join public.batches b on b.id=s.batch_id where s.active) s where i.id=s.id;",
        "UPDATE public.items i\nSET title = s.title\nFROM (\n    SELECT s.id, s.title\n    FROM staging.items s\n    JOIN public.batches b ON b.id = s.batch_id\n    WHERE s.active\n) s\nWHERE i.id = s.id;",
    );
}

#[test]
fn formats_merge_with_derived_and_joined_sources() {
    assert_format(
        "merge into public.items i using (select id,title from staging.items where active) s on i.id=s.id when matched then update set title=s.title;",
        "MERGE INTO public.items i\nUSING (\n    SELECT id, title\n    FROM staging.items\n    WHERE active\n) s\nON i.id = s.id\n\nWHEN MATCHED THEN UPDATE SET\n    title = s.title;",
    );

    assert_format(
        "merge into public.items i using staging.items s join public.batches b on b.id=s.batch_id on i.id=s.id when matched then delete;",
        "MERGE INTO public.items i\nUSING staging.items s\nJOIN public.batches b ON b.id = s.batch_id\nON i.id = s.id\n\nWHEN MATCHED THEN DELETE;",
    );
}

#[test]
fn formats_create_view_options_query_and_check_mode() {
    assert_format(
        "create or replace view public.active_items (id,title) with (security_barrier=true) as select id,title from public.items where active with local check option;",
        "CREATE OR REPLACE VIEW public.active_items (id, title) WITH (security_barrier = TRUE) AS\nSELECT id, title\nFROM public.items\nWHERE active\nWITH LOCAL CHECK OPTION;",
    );

    assert_format(
        "create view public.item_ids as select id from public.items with check option;",
        "CREATE VIEW public.item_ids AS\nSELECT id\nFROM public.items\nWITH CHECK OPTION;",
    );
}

#[test]
fn formats_materialized_view_storage_and_population_mode() {
    assert_format(
        "create materialized view if not exists public.item_counts (kind,total) using heap with (fillfactor=90) tablespace fast as select kind,count(*) as total from public.items group by kind with no data;",
        "CREATE MATERIALIZED VIEW IF NOT EXISTS public.item_counts (kind, total)\nUSING heap\nWITH (fillfactor = 90)\nTABLESPACE fast AS\nSELECT kind, COUNT(*) AS total\nFROM public.items\nGROUP BY kind\nWITH NO DATA;",
    );

    assert_format(
        "create materialized view public.item_ids as select id from public.items with data;",
        "CREATE MATERIALIZED VIEW public.item_ids AS\nSELECT id\nFROM public.items\nWITH DATA;",
    );

    assert_format(
        "create materialized view public.item_ids_default as select id from public.items;",
        "CREATE MATERIALIZED VIEW public.item_ids_default AS\nSELECT id\nFROM public.items;",
    );
}

#[test]
fn comments_remain_attached_across_source_and_view_boundaries() {
    let source = "UPDATE items i\nSET title = s.title\nFROM source_items s -- source ownership\nJOIN batches b ON b.id = s.batch_id\nWHERE i.id = s.item_id;\n\nCREATE VIEW active_items AS\nSELECT id -- public identifier\nFROM items\nWHERE active\nWITH LOCAL CHECK OPTION;\n";
    let expected = "UPDATE items i\nSET title = s.title\nFROM source_items s -- source ownership\nJOIN batches b ON b.id = s.batch_id\nWHERE i.id = s.item_id;\n\nCREATE VIEW active_items AS\nSELECT id -- public identifier\nFROM items\nWHERE active\nWITH LOCAL CHECK OPTION;\n";
    assert_format(source, expected);
}

#[test]
fn neighboring_unowned_source_and_view_shapes_remain_unchanged() {
    for source in [
        "UPDATE items SET title = source.title FROM source_items source JOIN batches batch USING (batch_id) AS matched WHERE items.id = source.item_id;",
        "CREATE VIEW v AS WITH source AS (SELECT 1 AS id) SELECT id FROM source;",
        "CREATE MATERIALIZED VIEW mv AS WITH source AS (SELECT 1 AS id) SELECT id FROM source WITH DATA;",
        "CREATE TABLE copy AS SELECT id FROM items;",
    ] {
        let formatted = format_sql_result(source, &FormatOptions::default());
        assert_eq!(formatted.output, source);
        assert!(!formatted.changed);
        assert_eq!(formatted.diagnostics.len(), 1);
        assert_eq!(formatted.diagnostics[0].rule_id, "syntax.unsupported");
    }
}
