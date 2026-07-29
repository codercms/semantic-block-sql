use pretty_assertions::assert_eq;
use semblock::{FormatOptions, check_sql, format_sql, format_sql_result, validate_equivalent};

fn assert_fixture(source: &str, expected: &str) {
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
fn formats_common_migration_statements() {
    assert_fixture(
        include_str!("fixtures/batch9/migrations.input.sql"),
        include_str!("fixtures/batch9/migrations.expected.sql"),
    );
}

#[test]
fn migration_comments_and_literals_remain_byte_identical() {
    let source = "COMMENT ON TABLE public.items IS 'mixed CASE -- literal';\nCREATE TYPE public.status AS ENUM ('new', 'in progress', 'done'); -- enum values\n";
    let expected = "COMMENT ON TABLE public.items IS 'mixed CASE -- literal';\nCREATE TYPE public.status AS ENUM ('new', 'in progress', 'done'); -- enum values\n";
    assert_fixture(source, expected);
}

#[test]
fn unreviewed_migration_neighbors_remain_fail_safe() {
    for source in [
        "DROP FUNCTION public.refresh_item(bigint);",
        "GRANT SELECT ON ALL TABLES IN SCHEMA public TO app_user;",
        "CREATE TYPE public.amount_range AS RANGE (subtype = numeric);",
        "ALTER SEQUENCE public.item_id_seq RESTART WITH 100;",
        "CREATE TRIGGER t AFTER INSERT ON items REFERENCING NEW TABLE AS inserted FOR EACH STATEMENT EXECUTE FUNCTION audit_items();",
    ] {
        let result = format_sql_result(source, &FormatOptions::default());
        assert_eq!(result.output, source);
        assert!(!result.changed);
        assert_eq!(result.diagnostics.len(), 1, "{source}");
        assert_eq!(
            result.diagnostics[0].rule_id, "syntax.unsupported",
            "{source}"
        );
    }
}
