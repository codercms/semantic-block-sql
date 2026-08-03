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
fn preserves_leading_comment_before_update_with_clause() {
    assert_format(
        "-- header\nWITH source AS (\n    SELECT 1 AS id\n)\nUPDATE items\nSET value = source.id\nFROM source\nWHERE items.id = source.id;",
        "-- header\nWITH source AS (\n    SELECT 1 AS id\n)\nUPDATE items\nSET value = source.id\nFROM source\nWHERE items.id = source.id;",
        &FormatOptions::default(),
    );
}

#[test]
fn formats_nested_update_and_predicates_inside_cte() {
    assert_format(
        "WITH claimed AS (UPDATE queue q SET status='processing', claimed_revision=q.revision, claim_token=gen_random_uuid(), processing_started_at=NOW(), available_at=NOW()+make_interval(secs=>$2::int), attempts=q.attempts+1, updated_at=NOW() WHERE q.id IN (SELECT c.id FROM queue c WHERE c.status='pending' AND c.available_at<=NOW() AND EXISTS (SELECT 1 FROM cards k WHERE k.id=c.id AND k.source_present) ORDER BY c.id LIMIT $1 FOR UPDATE SKIP LOCKED) RETURNING q.id, q.claimed_revision, q.claim_token, gen_random_uuid() AS request_id) SELECT * FROM claimed;",
        "WITH claimed AS (\n    UPDATE queue q\n    SET\n        status = 'processing',\n        claimed_revision = q.revision,\n        claim_token = gen_random_uuid(),\n        processing_started_at = NOW(),\n        available_at = NOW() + make_interval(secs => $2::int),\n        attempts = q.attempts + 1,\n        updated_at = NOW()\n    WHERE\n        q.id IN (\n            SELECT c.id\n            FROM queue c\n            WHERE\n                c.status = 'pending'\n                AND c.available_at <= NOW()\n                AND EXISTS (\n                    SELECT 1\n                    FROM cards k\n                    WHERE\n                        k.id = c.id\n                        AND k.source_present\n                )\n            ORDER BY c.id\n            LIMIT $1\n            FOR UPDATE SKIP LOCKED\n        )\n    RETURNING q.id, q.claimed_revision, q.claim_token, gen_random_uuid() AS request_id\n)\nSELECT *\nFROM claimed;",
        &FormatOptions::default(),
    );
}

#[test]
fn keeps_short_cte_update_lists_compact_when_the_statement_expands() {
    assert_format(
        "WITH claimed AS (UPDATE public.events SET status='ready' WHERE id IN (SELECT id FROM public.events WHERE status='pending' FOR UPDATE SKIP LOCKED) RETURNING id,status) SELECT id,status FROM claimed ORDER BY id;",
        "WITH claimed AS (\n    UPDATE public.events\n    SET status = 'ready'\n    WHERE id IN (SELECT id FROM public.events WHERE status = 'pending' FOR UPDATE SKIP LOCKED)\n    RETURNING id, status\n)\nSELECT id, status\nFROM claimed\nORDER BY id;",
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
fn formats_boolean_result_targets_with_exists_branches() {
    assert_format(
        "WITH target AS (\n    SELECT w.card_id,\n        -- Conflicting external mappings prevent application.\n    ((c.bb_nm_id IS NOT NULL AND c.bb_nm_id <> $2::bigint) OR (o.bb_imt_id IS NOT NULL AND o.bb_imt_id <> $1::bigint) OR EXISTS (SELECT 1 FROM cards x WHERE x.bb_nm_id = $2::bigint AND x.id <> w.card_id) OR EXISTS (SELECT 1 FROM offers x WHERE x.bb_imt_id = $1::bigint AND x.offer_id <> w.offer_id)) AS has_conflict,\n        -- A newer external version is already stored.\n    (c.bb_version IS NOT NULL AND c.bb_version >= $3::bigint) AS is_stale\n    FROM waiting w\n    JOIN cards c ON c.offer_id = w.offer_id AND c.id = w.card_id\n    JOIN offers o ON o.offer_id = w.offer_id\n)\nSELECT has_conflict, is_stale FROM target;",
        "WITH target AS (\n    SELECT\n        w.card_id,\n        -- Conflicting external mappings prevent application.\n        (\n            (c.bb_nm_id IS NOT NULL AND c.bb_nm_id <> $2::bigint)\n            OR (o.bb_imt_id IS NOT NULL AND o.bb_imt_id <> $1::bigint)\n            OR EXISTS (\n                SELECT 1\n                FROM cards x\n                WHERE\n                    x.bb_nm_id = $2::bigint\n                    AND x.id <> w.card_id\n            )\n            OR EXISTS (\n                SELECT 1\n                FROM offers x\n                WHERE\n                    x.bb_imt_id = $1::bigint\n                    AND x.offer_id <> w.offer_id\n            )\n        ) AS has_conflict,\n        -- A newer external version is already stored.\n        (c.bb_version IS NOT NULL AND c.bb_version >= $3::bigint) AS is_stale\n    FROM waiting w\n    JOIN cards c ON\n        c.offer_id = w.offer_id\n        AND c.id = w.card_id\n    JOIN offers o ON o.offer_id = w.offer_id\n)\nSELECT has_conflict, is_stale\nFROM target;",
        &FormatOptions::default(),
    );
}

#[test]
fn formats_boolean_predicates_across_query_clauses() {
    assert_format(
        "SELECT a.id, count(*) FROM a JOIN b ON ((a.x IS NOT NULL AND a.x <> b.x) OR (a.y IS NOT NULL AND a.y <> b.y)) WHERE ((a.ready AND a.visible) OR EXISTS (SELECT 1 FROM c WHERE c.a_id = a.id AND c.active)) GROUP BY a.id HAVING ((count(*) > 1 AND bool_or(b.ready)) OR EXISTS (SELECT 1 FROM d WHERE d.a_id = a.id AND d.active));",
        "SELECT a.id, COUNT(*)\nFROM a\nJOIN b ON\n    (\n        (a.x IS NOT NULL AND a.x <> b.x)\n        OR (a.y IS NOT NULL AND a.y <> b.y)\n    )\nWHERE\n    (\n        (a.ready AND a.visible)\n        OR EXISTS (\n            SELECT 1\n            FROM c\n            WHERE\n                c.a_id = a.id\n                AND c.active\n        )\n    )\nGROUP BY a.id\nHAVING\n    (\n        (COUNT(*) > 1 AND bool_or(b.ready))\n        OR EXISTS (\n            SELECT 1\n            FROM d\n            WHERE\n                d.a_id = a.id\n                AND d.active\n        )\n    );",
        &FormatOptions::default(),
    );
}

#[test]
fn formats_boolean_values_across_owned_expression_contexts() {
    assert_format(
        "UPDATE cards SET conflicted = ((bb_nm_id IS NOT NULL AND bb_nm_id <> $2::bigint) OR EXISTS (SELECT 1 FROM other_cards x WHERE x.bb_nm_id = $2::bigint AND x.id <> cards.id)) WHERE id = $1 RETURNING ((bb_nm_id IS NOT NULL AND bb_nm_id <> $2::bigint) OR EXISTS (SELECT 1 FROM other_cards x WHERE x.bb_nm_id = $2::bigint AND x.id <> cards.id)) AS has_conflict;",
        "UPDATE cards\nSET\n    conflicted = (\n        (bb_nm_id IS NOT NULL AND bb_nm_id <> $2::bigint)\n        OR EXISTS (\n            SELECT 1\n            FROM other_cards x\n            WHERE\n                x.bb_nm_id = $2::bigint\n                AND x.id <> cards.id\n        )\n    )\nWHERE id = $1\nRETURNING\n    (\n        (bb_nm_id IS NOT NULL AND bb_nm_id <> $2::bigint)\n        OR EXISTS (\n            SELECT 1\n            FROM other_cards x\n            WHERE\n                x.bb_nm_id = $2::bigint\n                AND x.id <> cards.id\n        )\n    ) AS has_conflict;",
        &FormatOptions::default(),
    );
    assert_format(
        "INSERT INTO audit (card_id, conflicted) VALUES ($1, ((a.ready AND a.visible) OR (a.forced AND a.reviewed))) RETURNING ((a.ready AND a.visible) OR (a.forced AND a.reviewed)) AS has_conflict;",
        "INSERT INTO audit (card_id, conflicted)\nVALUES (\n    $1,\n    (\n        (a.ready AND a.visible)\n        OR (a.forced AND a.reviewed)\n    )\n)\nRETURNING\n    (\n        (a.ready AND a.visible)\n        OR (a.forced AND a.reviewed)\n    ) AS has_conflict;",
        &FormatOptions::default(),
    );
    assert_format(
        "SELECT CASE WHEN ((a.ready AND a.visible) OR EXISTS (SELECT 1 FROM b WHERE b.a_id = a.id AND b.active)) THEN ((a.x IS NOT NULL AND a.x <> 0) OR (a.y IS NOT NULL AND a.y <> 0)) ELSE false END AS allowed, COALESCE(((a.ready AND a.visible) OR (a.forced AND a.reviewed)), false) AS fallback FROM a;",
        "SELECT\n    CASE\n        WHEN (\n            (a.ready AND a.visible)\n            OR EXISTS (\n                SELECT 1\n                FROM b\n                WHERE\n                    b.a_id = a.id\n                    AND b.active\n            )\n        ) THEN (\n            (a.x IS NOT NULL AND a.x <> 0)\n            OR (a.y IS NOT NULL AND a.y <> 0)\n        )\n        ELSE FALSE\n    END AS allowed,\n    COALESCE(\n        (\n            (a.ready AND a.visible)\n            OR (a.forced AND a.reviewed)\n        ),\n        FALSE\n    ) AS fallback\nFROM a;",
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
