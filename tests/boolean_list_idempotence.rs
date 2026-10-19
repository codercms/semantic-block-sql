mod support;

#[test]
fn compact_case_owns_its_condition_inside_a_cte() {
    support::assert_sql(
        "WITH batch AS (\n    SELECT CASE WHEN p.image_path IS NULL AND (s.image).src IS NOT NULL THEN 'remote'::public.image_source ELSE p.image_src END AS image_src\n    FROM items p, staging s\n)\nSELECT * FROM batch;\n",
        "WITH batch AS (\n    SELECT\n        CASE WHEN p.image_path IS NULL AND (s.image).src IS NOT NULL THEN 'remote'::public.image_source ELSE p.image_src END AS image_src\n    FROM\n        items p,\n        staging s\n)\nSELECT *\nFROM batch;\n",
    );
}

#[test]
fn mixed_boolean_target_expands_its_select_list_on_the_first_pass() {
    support::assert_sql(
        "SELECT id, flag AND (a OR b) AS accepted FROM items;\n",
        "SELECT\n    id,\n    flag\n    AND (\n        a\n        OR b\n    ) AS accepted\nFROM items;\n",
    );
}

#[test]
fn long_boolean_comparison_breaks_at_its_operator() {
    let options = semblock::FormatOptions {
        soft_line_width: 40,
        hard_line_width: 60,
        ..Default::default()
    };
    support::assert_sql_with(
        "SELECT id FROM items WHERE score_for(current_value) >= COALESCE(score_for(previous_value), 0);\n",
        "SELECT id\nFROM items\nWHERE\n    score_for(current_value)\n        >= COALESCE(score_for(previous_value), 0);\n",
        &options,
    );
}

#[test]
fn case_children_and_sibling_boolean_owners_remain_independent() {
    let options = semblock::FormatOptions::default();
    for source in [
        "SELECT id, a AND b AS ready, c OR d AS fallback FROM items;",
        "SELECT CASE WHEN a AND (b OR c) THEN 1 ELSE 0 END AS value FROM items;",
        "SELECT CASE WHEN a THEN CASE WHEN b AND c THEN 1 ELSE 2 END ELSE 3 END FROM items;",
        "SELECT COALESCE(flag AND (a OR b), FALSE) FROM items;",
        "SELECT * FROM items WHERE CASE WHEN a AND b THEN c ELSE d END AND (e OR f);",
        "SELECT * FROM items WHERE (CASE WHEN a OR b THEN c ELSE d END AND e) OR f;",
        "UPDATE items SET ready = a AND (b OR c) RETURNING id, ready;",
        "DELETE FROM items RETURNING id, a AND (b OR c) AS ready;",
        "VALUES (1, a AND (b OR c));",
        "SELECT id,\n\n-- keep with the flag\nflag AND (a OR b) AS ready FROM items;",
        "SELECT CASE WHEN a /* condition */ AND b THEN 1 ELSE 0 END FROM items;",
    ] {
        let result = semblock::format_sql(source, &options).unwrap();
        assert!(
            result
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.fix_available),
            "{source}: {:?}",
            result.diagnostics,
        );
        support::assert_sql(source, &result.output);
    }
}
