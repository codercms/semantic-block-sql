use pretty_assertions::assert_eq;
use semblock::{FormatOptions, NotEqualPolicy, SemicolonPolicy, format_sql};

fn format(source: &str, options: &FormatOptions) -> String {
    format_sql(source, options).expect("format succeeds").output
}

#[test]
fn applies_the_exact_builtin_casing_whitelist() {
    let cases = [
        ("select count(*);", "SELECT COUNT(*);"),
        (
            "select sum(value), avg(value), min(value), max(value);",
            "SELECT SUM(value), AVG(value), MIN(value), MAX(value);",
        ),
        (
            "select coalesce(value, 0), nullif(value, 0);",
            "SELECT COALESCE(value, 0), NULLIF(value, 0);",
        ),
        (
            "select greatest(a, b), least(a, b);",
            "SELECT GREATEST(a, b), LEAST(a, b);",
        ),
        (
            "select now(), extract(year from created_at);",
            "SELECT NOW(), EXTRACT(YEAR FROM created_at);",
        ),
        (
            "select date_trunc('day', created_at);",
            "SELECT date_trunc('day', created_at);",
        ),
        (
            "select substring(value from 1 for 2), trim(value), position('x' in value), overlay(value placing 'x' from 1);",
            "SELECT substring(value FROM 1 FOR 2), trim(value), position('x' IN value), overlay(value PLACING 'x' FROM 1);",
        ),
        (
            "select normalize(value), xmlconcat(value);",
            "SELECT normalize(value), xmlconcat(value);",
        ),
        (
            "select json_object('a': value), json_array(value);",
            "SELECT json_object('a' : value), json_array(value);",
        ),
        (
            "select current_date, current_time, current_timestamp;",
            "SELECT CURRENT_DATE, CURRENT_TIME, CURRENT_TIMESTAMP;",
        ),
        (
            "select current_user, current_role, current_schema, session_user;",
            "SELECT CURRENT_USER, CURRENT_ROLE, CURRENT_SCHEMA, SESSION_USER;",
        ),
        (
            "select localtime, localtimestamp;",
            "SELECT LOCALTIME, LOCALTIMESTAMP;",
        ),
    ];

    for (source, expected) in cases {
        assert_eq!(
            format(source, &FormatOptions::default()),
            expected,
            "{source}"
        );
    }
}

#[test]
fn distinguishes_interval_literals_from_interval_types() {
    assert_eq!(
        format(
            "select interval '5 minutes', value::INTERVAL, value::VARCHAR(255), cast(value as NUMERIC(10, 2));",
            &FormatOptions::default()
        ),
        "SELECT INTERVAL '5 minutes', value::interval, value::varchar(255), CAST(value AS numeric(10, 2));"
    );
}

#[test]
fn preserves_not_equal_by_default_and_normalizes_only_when_configured() {
    let source = "select id from items where status <> 'deleted';";

    assert_eq!(
        format(source, &FormatOptions::default()),
        "SELECT id FROM items WHERE status <> 'deleted';"
    );

    let prefer_bang = FormatOptions {
        not_equal_policy: NotEqualPolicy::PreferBang,
        ..FormatOptions::default()
    };
    assert_eq!(
        format(source, &prefer_bang),
        "SELECT id FROM items WHERE status != 'deleted';"
    );
}

#[test]
fn applies_terminal_semicolon_policies() {
    let preserve = FormatOptions::default();
    assert_eq!(format("select 1", &preserve), "SELECT 1");
    assert_eq!(format("select 1;", &preserve), "SELECT 1;");

    let require = FormatOptions {
        semicolon_policy: SemicolonPolicy::Require,
        ..FormatOptions::default()
    };
    assert_eq!(format("select 1", &require), "SELECT 1;");
    assert_eq!(
        format("select 1 -- result\n", &require),
        "SELECT 1; -- result\n"
    );

    let omit = FormatOptions {
        semicolon_policy: SemicolonPolicy::Omit,
        ..FormatOptions::default()
    };
    assert_eq!(format("select 1;", &omit), "SELECT 1");
    assert_eq!(
        format("select 1; -- result\n", &omit),
        "SELECT 1 -- result\n"
    );
}

#[test]
fn preserves_final_newline_presence() {
    assert_eq!(format("select 1;", &FormatOptions::default()), "SELECT 1;");
    assert_eq!(
        format("select 1;\n", &FormatOptions::default()),
        "SELECT 1;\n"
    );
}
