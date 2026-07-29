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
fn formats_merge_branches_as_owned_statement_blocks() {
    let source = "merge into public.items item using staging.items source on source.id = item.id when matched and source.deleted_at is not null then delete when matched then update set title = source.title, updated_at = now() when not matched then insert (id, title) values (source.id, source.title) returning item.id;";
    let expected = "MERGE INTO public.items item
USING staging.items source ON source.id = item.id

WHEN MATCHED AND source.deleted_at IS NOT NULL THEN DELETE

WHEN MATCHED THEN UPDATE SET
    title = source.title,
    updated_at = NOW()

WHEN NOT MATCHED THEN INSERT (id, title)
    VALUES (source.id, source.title)
RETURNING item.id;";

    assert_format(source, expected, &FormatOptions::default());
}

#[test]
fn supports_merge_do_nothing_and_insert_overriding() {
    let source = "merge into items target using incoming source on source.id = target.id when not matched by source then do nothing when not matched by target then insert (id) overriding user value values (source.id);";
    let expected = "MERGE INTO items target
USING incoming source ON source.id = target.id

WHEN NOT MATCHED BY SOURCE THEN DO NOTHING

WHEN NOT MATCHED BY TARGET THEN INSERT (id) OVERRIDING USER VALUE
    VALUES (source.id);";

    assert_format(source, expected, &FormatOptions::default());
}

#[test]
fn shares_with_ownership_with_merge() {
    let source = "with incoming as (select id,title from staging.items) merge into public.items target using incoming source on source.id = target.id when matched then update set title = source.title when not matched then insert (id,title) values (source.id,source.title) returning target.id;";
    let expected = "WITH incoming AS (\n    SELECT id, title\n    FROM staging.items\n)\nMERGE INTO public.items target\nUSING incoming source ON source.id = target.id\n\nWHEN MATCHED THEN UPDATE SET\n    title = source.title\n\nWHEN NOT MATCHED THEN INSERT (id, title)\n    VALUES (source.id, source.title)\nRETURNING target.id;";

    assert_format(source, expected, &FormatOptions::default());
}

#[test]
fn preserves_merge_comments_and_authored_branch_groups() {
    let source = "MERGE INTO items target\nUSING incoming source ON source.id = target.id\n\n-- remove stale rows\nWHEN MATCHED AND source.deleted_at IS NOT NULL THEN DELETE\n\n-- update live rows\nWHEN MATCHED THEN UPDATE SET\n    title = source.title, -- visible title\n    updated_at = NOW();\n";

    assert_format(source, source, &FormatOptions::default());
}
