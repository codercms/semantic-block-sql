mod support;

use semblock::FormatOptions;
use support::{SqlCase, assert_cases, assert_cases_with};

#[test]
fn preserves_authored_groups_across_clause_and_list_owners() {
    assert_cases(&[
        SqlCase::new(
            "SELECT list",
            "select\n    a,\n    b\nfrom items;",
            "SELECT\n    a,\n    b\nFROM items;",
        ),
        SqlCase::new(
            "RETURNING list",
            "insert into items(a,b) values(1,2) returning\n    a,\n    b;",
            "INSERT INTO items (a, b)\nVALUES (1, 2)\nRETURNING\n    a,\n    b;",
        ),
        SqlCase::new(
            "UPDATE SET list",
            "update items set\n    a=1,\n    b=2\nwhere id=1;",
            "UPDATE items\nSET\n    a = 1,\n    b = 2\nWHERE id = 1;",
        ),
        SqlCase::new(
            "VALUES row",
            "insert into items(a,b) values (\n    1,\n    2\n);",
            "INSERT INTO items (a, b)\nVALUES (\n    1,\n    2\n);",
        ),
        SqlCase::new(
            "GROUP BY and ORDER BY lists",
            "select a,b from items group by\n    a,\n    b\norder by\n    a,\n    b;",
            "SELECT a, b\nFROM items\nGROUP BY\n    a,\n    b\nORDER BY\n    a,\n    b;",
        ),
        SqlCase::new(
            "function arguments",
            "select calculate(\n    a,\n    b\n) from items;",
            "SELECT\n    calculate(\n        a,\n        b\n    )\nFROM items;",
        ),
        SqlCase::new(
            "WHERE predicate",
            "select * from items where\n    a=1\n    and b=2;",
            "SELECT *\nFROM items\nWHERE\n    a = 1\n    AND b = 2;",
        ),
        SqlCase::new(
            "HAVING predicate",
            "select a,count(*) from items group by a having\n    count(*)>1\n    and bool_or(active);",
            "SELECT a, COUNT(*)\nFROM items\nGROUP BY a\nHAVING\n    COUNT(*) > 1\n    AND bool_or(active);",
        ),
        SqlCase::new(
            "CHECK predicate",
            "create table items(a int,b int,check(\n    a>0\n    and b>0\n));",
            "CREATE TABLE items (\n    a int,\n    b int,\n\n    CHECK (\n        a > 0\n        AND b > 0\n    )\n);",
        ),
        SqlCase::new(
            "ON CONFLICT predicate and SET list",
            "insert into items(a,b) values(1,2) on conflict(a) where\n    a>0\n    and b>0\ndo update set\n    a=excluded.a,\n    b=excluded.b;",
            "INSERT INTO items (a, b)\nVALUES (1, 2)\nON CONFLICT (a) WHERE\n    a > 0\n    AND b > 0\nDO UPDATE\nSET\n    a = EXCLUDED.a,\n    b = EXCLUDED.b;",
        ),
        SqlCase::new(
            "MERGE ON predicate and action list",
            "merge into target using source on\n    target.id=source.id\n    and target.tenant_id=source.tenant_id\nwhen matched then update set\n    value=source.value,\n    updated_at=now();",
            "MERGE INTO target\nUSING source ON\n    target.id = source.id\n    AND target.tenant_id = source.tenant_id\n\nWHEN MATCHED THEN UPDATE SET\n    value = source.value,\n    updated_at = NOW();",
        ),
        SqlCase::new(
            "window PARTITION BY and ORDER BY",
            "select sum(value) over(partition by\n    tenant_id,\n    category\norder by\n    created_at,\n    id\n) from events;",
            "SELECT SUM(value) OVER (\n        PARTITION BY\n            tenant_id,\n            category\n        ORDER BY\n            created_at,\n            id\n    )\nFROM events;",
        ),
        SqlCase::new(
            "named WINDOW definition",
            "select sum(value) over w from events window w as (partition by\n    tenant_id,\n    category\norder by\n    created_at,\n    id\n);",
            "SELECT SUM(value) OVER w\nFROM events\nWINDOW w AS (\n    PARTITION BY\n        tenant_id,\n        category\n    ORDER BY\n        created_at,\n        id\n);",
        ),
    ]);
}

#[test]
fn keeps_short_inline_groups_compact() {
    assert_cases(&[
        SqlCase::new(
            "short SELECT",
            "select a,b from items;",
            "SELECT a, b FROM items;",
        ),
        SqlCase::new(
            "short function arguments",
            "select calculate(a,b) from items;",
            "SELECT calculate(a, b) FROM items;",
        ),
        SqlCase::new(
            "short predicate",
            "select * from items where a=1 and b=2;",
            "SELECT * FROM items WHERE a = 1 AND b = 2;",
        ),
        SqlCase::new(
            "short UPDATE SET",
            "update items set a=1,b=2 where id=1;",
            "UPDATE items SET a = 1, b = 2 WHERE id = 1;",
        ),
        SqlCase::new(
            "short RETURNING",
            "insert into items(a,b) values(1,2) returning a,b;",
            "INSERT INTO items (a, b) VALUES (1, 2) RETURNING a, b;",
        ),
        SqlCase::new(
            "short window",
            "select sum(value) over(partition by tenant_id) from events;",
            "SELECT SUM(value) OVER (PARTITION BY tenant_id) FROM events;",
        ),
    ]);
}

#[test]
fn expands_width_overflow_at_owner_safe_boundaries() {
    let options = FormatOptions {
        soft_line_width: 32,
        hard_line_width: 80,
        ..FormatOptions::default()
    };
    assert_cases_with(
        &[
            SqlCase::new(
                "SELECT overflow",
                "select alpha,beta,gamma from items;",
                "SELECT alpha, beta, gamma\nFROM items;",
            ),
            SqlCase::new(
                "function-call overflow",
                "select calculate(alpha,beta,gamma) from items;",
                "SELECT\n    calculate(alpha, beta, gamma)\nFROM items;",
            ),
            SqlCase::new(
                "predicate overflow",
                "select * from items where alpha=1 and beta=2 and gamma=3;",
                "SELECT *\nFROM items\nWHERE\n    alpha = 1\n    AND beta = 2\n    AND gamma = 3;",
            ),
            SqlCase::new(
                "SET overflow",
                "update items set alpha=1,beta=2,gamma=3 where id=1;",
                "UPDATE items\nSET\n    alpha = 1,\n    beta = 2,\n    gamma = 3\nWHERE id = 1;",
            ),
            SqlCase::new(
                "INSERT and RETURNING overflow",
                "insert into items(alpha,beta,gamma) values(1,2,3) returning alpha,beta,gamma;",
                "INSERT INTO items (\n    alpha,\n    beta,\n    gamma\n)\nVALUES (1, 2, 3)\nRETURNING\n    alpha,\n    beta,\n    gamma;",
            ),
        ],
        &options,
    );
}

#[test]
fn comments_and_blank_lines_remain_hard_group_boundaries() {
    assert_cases(&[
        SqlCase::new(
            "commented SELECT groups",
            "select\n    a,\n\n    -- Display metadata.\n    b,c\nfrom items;",
            "SELECT\n    a,\n\n    -- Display metadata.\n    b, c\nFROM items;",
        ),
        SqlCase::new(
            "commented Boolean branch",
            "select * from items where\n    a=1\n    -- Tenant boundary.\n    and tenant_id=2;",
            "SELECT *\nFROM items\nWHERE\n    a = 1\n    -- Tenant boundary.\n    AND tenant_id = 2;",
        ),
    ]);
}
