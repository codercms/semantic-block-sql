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
        "formatting must be idempotent",
    );
    let checked = check_sql(expected, options);
    assert!(checked.compliant, "diagnostics: {:?}", checked.diagnostics);
}

#[test]
fn formats_select_into_and_every_row_lock_strength() {
    let options = FormatOptions {
        soft_line_width: 64,
        hard_line_width: 96,
        ..FormatOptions::default()
    };
    assert_format(
        "select id,name into temp active_users from public.users where active=true for update of users nowait;",
        "SELECT id, name\nINTO TEMP active_users\nFROM public.users\nWHERE active = TRUE\nFOR UPDATE OF users NOWAIT;",
        &options,
    );
    assert_format(
        "select item.id,related.id from items item join related on related.item_id=item.id for no key update of item skip locked for share of related;",
        "SELECT item.id, related.id\nFROM items item\nJOIN related ON related.item_id = item.id\nFOR NO KEY UPDATE OF item SKIP LOCKED\nFOR SHARE OF related;",
        &options,
    );
    assert_format(
        "select id from items for key share for update;",
        "SELECT id FROM items FOR KEY SHARE FOR UPDATE;",
        &FormatOptions::default(),
    );
}

#[test]
fn formats_data_modifying_ctes_and_search_cycle_clauses() {
    assert_format(
        "with moved as (delete from queue where processed=true returning id), refreshed as (update items set ready=true where id in (select id from moved) returning id) select id from refreshed;",
        "WITH moved AS (\n    DELETE FROM queue WHERE processed = TRUE RETURNING id\n),\nrefreshed AS (\n    UPDATE items SET ready = TRUE WHERE id IN (SELECT id FROM moved) RETURNING id\n)\nSELECT id\nFROM refreshed;",
        &FormatOptions::default(),
    );

    assert_format(
        "with recursive tree(id,parent_id) as (select id,parent_id from nodes where parent_id is null union all select node.id,node.parent_id from nodes node join tree parent on node.parent_id=parent.id) search breadth first by id set visit_order cycle id set is_cycle using visit_path select id from tree order by visit_order;",
        "WITH RECURSIVE tree(id, parent_id) AS (\n    SELECT id, parent_id\n    FROM nodes\n    WHERE parent_id IS NULL\n\n    UNION ALL\n\n    SELECT node.id, node.parent_id\n    FROM nodes node\n    JOIN tree parent ON node.parent_id = parent.id\n)\nSEARCH BREADTH FIRST BY id SET visit_order\nCYCLE id SET is_cycle USING visit_path\nSELECT id\nFROM tree\nORDER BY visit_order;",
        &FormatOptions::default(),
    );
}

#[test]
fn formats_scalar_and_predicate_subqueries_in_dml() {
    assert_format(
        "update users set active=(select count(*) > 0 from sessions where sessions.user_id=users.id) where id in (select user_id from pending);",
        "UPDATE users\nSET\n    active = (SELECT COUNT(*) > 0 FROM sessions WHERE sessions.user_id = users.id)\nWHERE id IN (SELECT user_id FROM pending);",
        &FormatOptions::default(),
    );
    assert_format(
        "delete from users where not exists (select 1 from sessions where sessions.user_id=users.id);",
        "DELETE FROM users WHERE NOT EXISTS (SELECT 1 FROM sessions WHERE sessions.user_id = users.id);",
        &FormatOptions::default(),
    );
    assert_format(
        "merge into users target using incoming source on target.id=source.id when matched and exists (select 1 from audit where audit.user_id=target.id) then update set active=(select source.active) when not matched then insert (id,active) values (source.id,(select source.active));",
        "MERGE INTO users target\nUSING incoming source ON target.id = source.id\n\nWHEN MATCHED AND EXISTS (SELECT 1 FROM audit WHERE audit.user_id = target.id) THEN UPDATE SET\n    active = (SELECT source.active)\n\nWHEN NOT MATCHED THEN INSERT (id, active)\n    VALUES (\n        source.id,\n        (SELECT source.active)\n    );",
        &FormatOptions::default(),
    );
}

#[test]
fn unreviewed_query_neighbors_remain_fail_safe() {
    for source in [
        "SELECT * INTO TEMP TABLE copied WITH NO DATA FROM items;",
        "WITH source AS (VACUUM items) SELECT 1;",
        "UPDATE items SET payload = JSON_QUERY(payload, '$.a');",
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
