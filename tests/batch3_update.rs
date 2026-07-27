use pretty_assertions::assert_eq;
use semblock::{FormatOptions, check_sql, format_sql, format_sql_result, validate_equivalent};

fn assert_format(source: &str, expected: &str) {
    let options = FormatOptions::default();
    let formatted = format_sql(source, &options).expect("format succeeds");
    assert_eq!(formatted.output, expected);
    validate_equivalent(source, expected).expect("semantic equivalence");
    assert_eq!(
        format_sql(expected, &options)
            .expect("second format succeeds")
            .output,
        expected,
        "formatting must be idempotent"
    );
    let checked = check_sql(expected, &options);
    assert!(
        checked.compliant,
        "formatted SQL must pass check: {:?}",
        checked.diagnostics
    );
}

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
    let expected = "UPDATE public.items item\nSET\n    title = source.title,\n    updated_at = NOW()\nFROM staging.items source\nWHERE\n    item.id = source.id\n    AND source.ready = TRUE\nRETURNING\n    item.id,\n    item.updated_at;";

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
fn unsupported_update_variants_remain_unchanged() {
    for source in [
        "UPDATE ONLY items SET title = 'x';",
        "UPDATE items SET (title, updated_at) = ('x', NOW());",
        "UPDATE items SET payload['title'] = 'x';",
        "UPDATE items SET title = source.title FROM ROWS FROM (jsonb_each_text(payload)) source;",
        "UPDATE items SET title = source.title FROM staging.items source TABLESAMPLE SYSTEM (10);",
        "UPDATE items SET title = (SELECT title FROM staging.items LIMIT 1);",
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
