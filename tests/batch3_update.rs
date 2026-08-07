mod support;

use pretty_assertions::assert_eq;
use semblock::{FormatOptions, check_sql, format_sql_result};
use support::assert_sql as assert_format;

#[test]
fn compact_single_assignment_update_stays_inline() {
    assert_format(
        "update public.items set title=$1 where id=$2 returning id;",
        "UPDATE public.items SET title = $1 WHERE id = $2 RETURNING id;",
    );
}

#[test]
fn update_from_separates_set_source_predicate_and_returning() {
    let source = "update public.items item set title=source.title, updated_at=now() from staging.items source where item.id=source.id and source.ready=true returning item.id, item.updated_at;";
    let expected = "UPDATE public.items item\nSET title = source.title, updated_at = NOW()\nFROM staging.items source\nWHERE item.id = source.id AND source.ready = TRUE\nRETURNING item.id, item.updated_at;";

    assert_format(source, expected);

    let checked = check_sql(source, &FormatOptions::default());
    assert!(
        checked
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.rule_id == "layout.update_set"),
        "unformatted UPDATE SET should receive a rule-level diagnostic: {:?}",
        checked.diagnostics
    );
}

#[test]
fn update_set_comments_remain_attached() {
    let source = "update items set title=$1, -- chosen title\nupdated_at=now() from staging.items source where items.id=source.id;\n";
    let expected = "UPDATE items\nSET\n    title = $1, -- chosen title\n    updated_at = NOW()\nFROM staging.items source\nWHERE items.id = source.id;\n";

    assert_format(source, expected);
}

#[test]
fn update_lists_preserve_authored_groups_within_the_hard_limit() {
    let source = "UPDATE items\nSET\n    status='ready', claimed_revision=revision,\n    claim_token=gen_random_uuid(), updated_at=NOW()\nWHERE id=$1\nRETURNING\n    id,status,\n    claim_token;";
    let expected = "UPDATE items\nSET\n    status = 'ready', claimed_revision = revision,\n    claim_token = gen_random_uuid(), updated_at = NOW()\nWHERE id = $1\nRETURNING\n    id, status,\n    claim_token;";

    assert_format(source, expected);
}

#[test]
fn update_supports_expanded_sources_and_subqueries() {
    assert_format(
        "UPDATE items SET title = source.title FROM ROWS FROM (jsonb_each_text(payload)) source;",
        "UPDATE items
SET title = source.title
FROM ROWS FROM (jsonb_each_text(payload)) source;",
    );
    assert_format(
        "UPDATE items SET title = source.title FROM staging.items source TABLESAMPLE SYSTEM (10);",
        "UPDATE items
SET title = source.title
FROM staging.items source TABLESAMPLE SYSTEM (10);",
    );
    assert_format(
        "UPDATE items SET title = (SELECT title FROM staging.items LIMIT 1);",
        "UPDATE items SET title = (SELECT title FROM staging.items LIMIT 1);",
    );
}

#[test]
fn update_assignments_do_not_treat_is_distinct_from_as_a_from_clause() {
    assert_format(
        "update items set changed = old_value is distinct from new_value, updated_at = now() where id = $1;",
        "UPDATE items SET changed = old_value IS DISTINCT FROM new_value, updated_at = NOW() WHERE id = $1;",
    );
}

#[test]
fn update_predicates_do_not_treat_is_distinct_from_as_a_from_clause() {
    assert_format(
        "update items set value = $1 where old_value is distinct from $2 returning id;",
        "UPDATE items SET value = $1 WHERE old_value IS DISTINCT FROM $2 RETURNING id;",
    );
}

#[test]
fn update_from_is_owned_after_assignment_expressions() {
    assert_format(
        "update items item set changed = item.value is distinct from source.value, updated_at = now() from staging.items source where item.id = source.id;",
        "UPDATE items item\nSET changed = item.value IS DISTINCT FROM source.value, updated_at = NOW()\nFROM staging.items source\nWHERE item.id = source.id;",
    );
}

#[test]
fn unsupported_update_variants_remain_unchanged() {
    for source in [
        "UPDATE ONLY items SET title = 'x';",
        "UPDATE items SET (title, updated_at) = ('x', NOW());",
        "UPDATE items SET payload['title'] = 'x';",
    ] {
        let result = format_sql_result(source, &FormatOptions::default());
        assert_eq!(
            result.output, source,
            "unsupported source changed: {source}"
        );
        assert!(!result.changed);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.rule_id == "syntax.unsupported"),
            "missing unsupported diagnostic for {source}: {:?}",
            result.diagnostics
        );
    }
}
