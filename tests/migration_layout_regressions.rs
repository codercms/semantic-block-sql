mod support;

#[test]
fn query_loop_keeps_internal_lock_comment_and_stable_indentation() {
    support::assert_sql_layout_only(
        "DO $$ DECLARE row_data record; BEGIN LOOP FOR row_data IN\nSELECT id FROM demo.jobs\n-- retain lock explanation\nFOR NO KEY UPDATE SKIP LOCKED\nLOOP PERFORM row_data.id; END LOOP; EXIT; END LOOP; END; $$;",
        "DO $$\nDECLARE\n    row_data record;\nBEGIN\n    LOOP\n        FOR row_data IN\n            SELECT id FROM demo.jobs\n            -- retain lock explanation\n            FOR NO KEY UPDATE SKIP LOCKED\n        LOOP\n            PERFORM row_data.id;\n        END LOOP;\n        EXIT;\n    END LOOP;\nEND;\n$$;",
    );
}

#[test]
fn long_in_list_wraps_at_owned_item_boundaries() {
    let options = semblock::FormatOptions {
        soft_line_width: 50,
        hard_line_width: 80,
        ..Default::default()
    };
    support::assert_sql_with(
        "SELECT id FROM demo.catalog WHERE object_id IN (\n'demo.catalog_primary_lookup'::regclass,\n'demo.catalog_secondary_lookup'::regclass,\n'demo.catalog_archive_lookup'::regclass\n) AND ready;",
        "SELECT id\nFROM demo.catalog\nWHERE\n    object_id IN (\n        'demo.catalog_primary_lookup'::regclass,\n        'demo.catalog_secondary_lookup'::regclass,\n        'demo.catalog_archive_lookup'::regclass\n    )\n    AND ready;",
        &options,
    );
}

#[test]
fn in_lists_preserve_groups_comments_and_nested_expression_owners() {
    for source in [
        "SELECT id FROM demo.rows WHERE state IN ('a', 'b');",
        "SELECT id FROM demo.rows WHERE state NOT IN /* retained */ (\n'a', 'b',\n\n-- retained group\n'c'\n);",
        "SELECT CASE WHEN state IN ('a', 'b') THEN 1 ELSE 0 END FROM demo.rows;",
        "SELECT COALESCE(state IN ('a', 'b'), FALSE) FROM demo.rows;",
        "SELECT id FROM demo.rows WHERE (id, state) IN ((1, 'a'), (2, 'b'));",
        "SELECT id FROM demo.rows WHERE id IN (SELECT id FROM demo.allowed);",
        "SELECT id FROM demo.rows WHERE id IN (1, -- retained item\n2, 3);",
    ] {
        let result = semblock::format_sql(source, &Default::default()).unwrap();
        assert!(
            result.diagnostics.iter().all(|d| d.fix_available),
            "{source}: {:?}",
            result.diagnostics
        );
        support::assert_sql(source, &result.output);
    }
}

#[test]
fn in_lists_respect_default_width_inside_nested_predicates() {
    let values = (1..=5)
        .map(|n| format!("'demo.synthetic_catalog_lookup_number_{n}'::regclass"))
        .collect::<Vec<_>>()
        .join(", ");
    for source in [
        format!("SELECT id FROM demo.catalog WHERE object_id NOT IN ({values}) AND ready;"),
        format!(
            "WITH candidates AS (SELECT id FROM demo.catalog WHERE (object_id IN ({values}) OR ready)) SELECT * FROM candidates;"
        ),
    ] {
        let result = semblock::format_sql(&source, &Default::default()).unwrap();
        assert!(
            result.diagnostics.iter().all(|d| d.fix_available),
            "{:?}",
            result.diagnostics
        );
        assert!(
            result
                .output
                .lines()
                .all(|line| line.chars().count() <= 160)
        );
        support::assert_sql(&source, &result.output);
    }
}

#[test]
fn query_loops_keep_nested_queries_and_multiline_trivia() {
    for source in [
        "DO $$ DECLARE row_data record; BEGIN FOR row_data IN SELECT id FROM demo.jobs WHERE EXISTS (SELECT 1 FROM demo.details WHERE details.id = jobs.id AND details.ready) LOOP PERFORM row_data.id; END LOOP; END; $$;",
        "DO $$ DECLARE row_data record; BEGIN FOR row_data IN /* retained query */ WITH seed AS (SELECT 1 AS id) SELECT id FROM seed LOOP PERFORM row_data.id; END LOOP; END; $$;",
        "DO $$ DECLARE row_data record; BEGIN FOR row_data IN SELECT id FROM demo.jobs\n-- retained final query comment\nLOOP PERFORM row_data.id; END LOOP; END; $$;",
    ] {
        for source in [source.to_owned(), source.replace('\n', "\r\n")] {
            let result = semblock::format_sql(&source, &Default::default()).unwrap();
            assert!(
                result.diagnostics.iter().all(|d| d.fix_available),
                "{:?}",
                result.diagnostics
            );
            support::assert_sql_layout_only(&source, &result.output);
        }
    }
}
