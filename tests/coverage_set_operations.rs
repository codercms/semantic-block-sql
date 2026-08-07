mod support;

use support::{SqlCase, assert_cases};

#[test]
fn formats_every_set_operator_and_modifier() {
    assert_cases(&[
        SqlCase::new(
            "UNION",
            "select 1 union select 2;",
            "SELECT 1\n\nUNION\n\nSELECT 2;",
        ),
        SqlCase::new(
            "UNION ALL",
            "select 1 union all select 2;",
            "SELECT 1\n\nUNION ALL\n\nSELECT 2;",
        ),
        SqlCase::new(
            "UNION DISTINCT",
            "select 1 union distinct select 2;",
            "SELECT 1\n\nUNION DISTINCT\n\nSELECT 2;",
        ),
        SqlCase::new(
            "INTERSECT ALL",
            "select 1 intersect all select 2;",
            "SELECT 1\n\nINTERSECT ALL\n\nSELECT 2;",
        ),
        SqlCase::new(
            "EXCEPT ALL",
            "select 1 except all select 2;",
            "SELECT 1\n\nEXCEPT ALL\n\nSELECT 2;",
        ),
    ]);
}

#[test]
fn owns_wrapped_and_nested_operation_trees_recursively() {
    assert_cases(&[
        SqlCase::new(
            "wrapped left branch",
            "(select 1 union select 2) intersect select 3;",
            "(\n    SELECT 1\n\n    UNION\n\n    SELECT 2\n)\n\nINTERSECT\n\nSELECT 3;",
        ),
        SqlCase::new(
            "wrapped right branch",
            "select 1 union (select 2 intersect select 3);",
            "SELECT 1\n\nUNION\n\n(\n    SELECT 2\n\n    INTERSECT\n\n    SELECT 3\n);",
        ),
        SqlCase::new(
            "branch-local sorting and root sorting",
            "(select 1 order by 1 limit 1) union all (select 2 order by 1 limit 1) order by 1;",
            "(SELECT 1 ORDER BY 1 LIMIT 1)\n\nUNION ALL\n\n(SELECT 2 ORDER BY 1 LIMIT 1)\nORDER BY 1;",
        ),
    ]);
}

#[test]
fn owns_set_operations_in_every_query_container() {
    assert_cases(&[
        SqlCase::new(
            "CTE body",
            "with combined_ids as(select 1 as id union all select 2) select id from combined_ids;",
            "WITH combined_ids AS (\n    SELECT 1 AS id\n\n    UNION ALL\n\n    SELECT 2\n)\nSELECT id\nFROM combined_ids;",
        ),
        SqlCase::new(
            "derived source",
            "select id from(select 1 as id union all select 2) combined_ids;",
            "SELECT id\nFROM (\n    SELECT 1 AS id\n\n    UNION ALL\n\n    SELECT 2\n) combined_ids;",
        ),
        SqlCase::new(
            "INSERT query source",
            "insert into target(id) select 1 union all select 2 returning id;",
            "INSERT INTO target (id)\nSELECT 1\n\nUNION ALL\n\nSELECT 2\nRETURNING id;",
        ),
        SqlCase::new(
            "view query",
            "create view ids as select 1 as id union all select 2;",
            "CREATE VIEW ids AS\nSELECT 1 AS id\n\nUNION ALL\n\nSELECT 2;",
        ),
        SqlCase::new(
            "SQL-standard routine body",
            "CREATE FUNCTION ids()\nRETURNS TABLE (id int)\nLANGUAGE SQL\nBEGIN ATOMIC\n    select 1 union all select 2;\nEND;",
            "CREATE FUNCTION ids()\nRETURNS TABLE (id int)\nLANGUAGE SQL\nBEGIN ATOMIC\n    SELECT 1\n\n    UNION ALL\n\n    SELECT 2;\nEND;",
        ),
    ]);
}

#[test]
fn keeps_comments_attached_to_their_operator_or_branch() {
    assert_cases(&[
        SqlCase::new(
            "comment before following branch",
            "select 1\nunion all\n-- Archived branch.\nselect 2;",
            "SELECT 1\n\nUNION ALL\n\n-- Archived branch.\nSELECT 2;",
        ),
        SqlCase::new(
            "comment before operator",
            "select 1\n-- Preserve source ordering.\nunion all\nselect 2;",
            "SELECT\n    1\n    -- Preserve source ordering.\n\nUNION ALL\n\nSELECT 2;",
        ),
    ]);
}

#[test]
fn qualified_suffix_words_remain_branch_identifiers() {
    assert_cases(&[
        SqlCase::new(
            "qualified ORDER identifier",
            "select 1 union select t.order from t;",
            "SELECT 1\n\nUNION\n\nSELECT t.order FROM t;",
        ),
        SqlCase::new(
            "qualified LIMIT identifier",
            "select 1 union select t.limit from t;",
            "SELECT 1\n\nUNION\n\nSELECT t.limit FROM t;",
        ),
        SqlCase::new(
            "qualified OFFSET identifier",
            "select 1 union select t.offset from t;",
            "SELECT 1\n\nUNION\n\nSELECT t.offset FROM t;",
        ),
        SqlCase::new(
            "qualified FETCH identifier",
            "select 1 union select t.fetch from t;",
            "SELECT 1\n\nUNION\n\nSELECT t.fetch FROM t;",
        ),
        SqlCase::new(
            "qualified FOR identifier",
            "select 1 union select t.for from t;",
            "SELECT 1\n\nUNION\n\nSELECT t.for FROM t;",
        ),
    ]);
}
