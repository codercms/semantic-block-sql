mod support;

use semblock::FormatOptions;
use semblock::config::GoConfig;
use semblock::source::{Language, format_source};
use support::{SqlCase, assert_cases, assert_fixture_pair};

#[test]
fn preserves_authored_count_query() {
    assert_fixture_pair("authored_clauses", "count-query");
}

#[test]
fn preserves_clause_breaks_without_expanding_inline_siblings() {
    assert_cases(&[
        SqlCase::new(
            "only FROM starts a line",
            "select id\nfrom items where id=1;",
            "SELECT id\nFROM items WHERE id = 1;",
        ),
        SqlCase::new(
            "only WHERE starts a line",
            "select id from items\nwhere id=1;",
            "SELECT id FROM items\nWHERE id = 1;",
        ),
        SqlCase::new(
            "blank clause boundary",
            "SELECT id\n\nFROM items\n\nWHERE id = 1;",
            "SELECT id\n\nFROM items\n\nWHERE id = 1;",
        ),
        SqlCase::new(
            "query suffixes",
            "SELECT id\nFROM items\nORDER BY id\nLIMIT 5\nOFFSET 1\nFOR UPDATE;",
            "SELECT id\nFROM items\nORDER BY id\nLIMIT 5\nOFFSET 1\nFOR UPDATE;",
        ),
        SqlCase::new(
            "grouping and having",
            "SELECT id, COUNT(*)\nFROM items\nGROUP BY id\nHAVING COUNT(*) > 1;",
            "SELECT id, COUNT(*)\nFROM items\nGROUP BY id\nHAVING COUNT(*) > 1;",
        ),
        SqlCase::new(
            "SELECT INTO",
            "SELECT id\nINTO saved_items\nFROM items;",
            "SELECT id\nINTO saved_items\nFROM items;",
        ),
        SqlCase::new(
            "FETCH",
            "SELECT id\nFROM items\nFETCH FIRST 5 ROWS ONLY;",
            "SELECT id\nFROM items\nFETCH FIRST 5 ROWS ONLY;",
        ),
        SqlCase::new(
            "nested query",
            "SELECT\n    (\n        SELECT id\n\n        FROM items\n        WHERE id = 1\n    );",
            "SELECT\n    (\n        SELECT id\n\n        FROM items\n        WHERE id = 1\n    );",
        ),
        SqlCase::new(
            "standalone and inline comments",
            "SELECT id\nFROM items -- Source.\n-- Filter.\nWHERE id = 1;",
            "SELECT id\nFROM items -- Source.\n-- Filter.\nWHERE id = 1;",
        ),
        SqlCase::new(
            "one-line query",
            "select count(*) from items where id between 1 and 5;",
            "SELECT COUNT(*) FROM items WHERE id BETWEEN 1 AND 5;",
        ),
        SqlCase::new(
            "literal newline is not a clause boundary",
            "SELECT 'first\nsecond' FROM items;",
            "SELECT 'first\nsecond' FROM items;",
        ),
    ]);
}

#[test]
fn preserves_compliant_clause_layout_in_go_and_rust_literals() {
    let sql = include_str!("fixtures/authored_clauses/count-query.input.sql");
    let sources = [
        (
            Language::Go,
            format!("package fixture\n//language=postgresql\nconst SQL = `\n{sql}`\n"),
        ),
        (
            Language::Rust,
            format!("//language=postgresql\n    const SQL: &str = r#\"\n{sql}\"#;\n"),
        ),
    ];
    for (language, source) in sources {
        let result = format_source(
            &source,
            language,
            &FormatOptions::default(),
            &GoConfig::default(),
        )
        .unwrap();
        assert_eq!(result.output, source, "{language:?}");
        assert!(!result.changed, "{language:?}");
        assert!(
            result.diagnostics.is_empty(),
            "{language:?}: {:?}",
            result.diagnostics
        );
    }
}
