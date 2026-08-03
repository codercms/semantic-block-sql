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
fn compact_delete_stays_inline() {
    assert_format(
        "delete from public.items where id=$1 returning id;",
        "DELETE FROM public.items WHERE id = $1 RETURNING id;",
    );
}

#[test]
fn delete_using_separates_source_predicate_and_returning() {
    let source = "delete from public.items item using staging.items source where item.id=source.id and source.expired=true returning item.id;";
    let expected = "DELETE FROM public.items item\nUSING staging.items source\nWHERE item.id = source.id AND source.expired = TRUE\nRETURNING item.id;";

    assert_format(source, expected);
}

#[test]
fn delete_comments_remain_attached() {
    let source = "delete from public.items item using staging.items source -- source rows\nwhere item.id=source.id;\n";
    let expected = "DELETE FROM public.items item\nUSING staging.items source -- source rows\nWHERE item.id = source.id;\n";

    assert_format(source, expected);
}

#[test]
fn delete_supports_expanded_sources_and_subqueries() {
    assert_format(
        "DELETE FROM items USING staging.items source TABLESAMPLE SYSTEM (10) WHERE items.id = source.id;",
        "DELETE FROM items
USING staging.items source TABLESAMPLE SYSTEM (10)
WHERE items.id = source.id;",
    );
    assert_format(
        "DELETE FROM items USING ROWS FROM (jsonb_each_text(payload)) source;",
        "DELETE FROM items
USING ROWS FROM (jsonb_each_text(payload)) source;",
    );
    assert_format(
        "DELETE FROM items WHERE id IN (SELECT id FROM staging.items);",
        "DELETE FROM items WHERE id IN (SELECT id FROM staging.items);",
    );
    assert_format(
        "DELETE FROM items RETURNING (SELECT 1);",
        "DELETE FROM items RETURNING (SELECT 1);",
    );
}

#[test]
fn unsupported_delete_variants_remain_unchanged() {
    for source in [
        "DELETE FROM ONLY items WHERE id = 1;",
        "DELETE FROM items USING ONLY staging.items source WHERE items.id = source.id;",
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
