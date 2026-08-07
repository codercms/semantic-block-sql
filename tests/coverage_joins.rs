mod support;

use support::{SqlCase, assert_cases, assert_sql, assert_unsupported_cases};

#[test]
fn formats_every_postgresql_join_header_without_splitting_it() {
    assert_cases(&[
        SqlCase::new(
            "bare inner join",
            "select * from a join b on a.id=b.id;",
            "SELECT *\nFROM a\nJOIN b ON a.id = b.id;",
        ),
        SqlCase::new(
            "explicit inner join",
            "select * from a inner join b on a.id=b.id;",
            "SELECT *\nFROM a\nINNER JOIN b ON a.id = b.id;",
        ),
        SqlCase::new(
            "left join",
            "select * from a left join b on a.id=b.id;",
            "SELECT *\nFROM a\nLEFT JOIN b ON a.id = b.id;",
        ),
        SqlCase::new(
            "left outer join",
            "select * from a left outer join b on a.id=b.id;",
            "SELECT *\nFROM a\nLEFT OUTER JOIN b ON a.id = b.id;",
        ),
        SqlCase::new(
            "right join",
            "select * from a right join b on a.id=b.id;",
            "SELECT *\nFROM a\nRIGHT JOIN b ON a.id = b.id;",
        ),
        SqlCase::new(
            "right outer join",
            "select * from a right outer join b on a.id=b.id;",
            "SELECT *\nFROM a\nRIGHT OUTER JOIN b ON a.id = b.id;",
        ),
        SqlCase::new(
            "full join",
            "select * from a full join b on a.id=b.id;",
            "SELECT *\nFROM a\nFULL JOIN b ON a.id = b.id;",
        ),
        SqlCase::new(
            "full outer join",
            "select * from a full outer join b on a.id=b.id;",
            "SELECT *\nFROM a\nFULL OUTER JOIN b ON a.id = b.id;",
        ),
        SqlCase::new(
            "cross join",
            "select * from a cross join b;",
            "SELECT *\nFROM a\nCROSS JOIN b;",
        ),
        SqlCase::new(
            "natural join",
            "select * from a natural join b;",
            "SELECT *\nFROM a\nNATURAL JOIN b;",
        ),
        SqlCase::new(
            "natural inner join",
            "select * from a natural inner join b;",
            "SELECT *\nFROM a\nNATURAL INNER JOIN b;",
        ),
        SqlCase::new(
            "natural left join",
            "select * from a natural left join b;",
            "SELECT *\nFROM a\nNATURAL LEFT JOIN b;",
        ),
        SqlCase::new(
            "natural left outer join",
            "select * from a natural left outer join b;",
            "SELECT *\nFROM a\nNATURAL LEFT OUTER JOIN b;",
        ),
        SqlCase::new(
            "natural right join",
            "select * from a natural right join b;",
            "SELECT *\nFROM a\nNATURAL RIGHT JOIN b;",
        ),
        SqlCase::new(
            "natural right outer join",
            "select * from a natural right outer join b;",
            "SELECT *\nFROM a\nNATURAL RIGHT OUTER JOIN b;",
        ),
        SqlCase::new(
            "natural full join",
            "select * from a natural full join b;",
            "SELECT *\nFROM a\nNATURAL FULL JOIN b;",
        ),
        SqlCase::new(
            "natural full outer join",
            "select * from a natural full outer join b;",
            "SELECT *\nFROM a\nNATURAL FULL OUTER JOIN b;",
        ),
    ]);
}

#[test]
fn preserves_authored_join_constraints_and_using_groups() {
    assert_cases(&[
        SqlCase::new(
            "authored ON group",
            "select * from a join b on\n    a.id=b.a_id\n    and a.tenant_id=b.tenant_id;",
            "SELECT *\nFROM a\nJOIN b ON\n    a.id = b.a_id\n    AND a.tenant_id = b.tenant_id;",
        ),
        SqlCase::new(
            "authored USING group",
            "select * from a join b using (\n    id,\n    tenant_id\n);",
            "SELECT *\nFROM a\nJOIN b USING (\n    id,\n    tenant_id\n);",
        ),
        SqlCase::new(
            "commented ON branch",
            "select * from a left join b on\n    a.id=b.a_id\n    -- Keep tenant isolation with the following branch.\n    and a.tenant_id=b.tenant_id;",
            "SELECT *\nFROM a\nLEFT JOIN b ON\n    a.id = b.a_id\n    -- Keep tenant isolation with the following branch.\n    AND a.tenant_id = b.tenant_id;",
        ),
    ]);
}

#[test]
fn owns_join_headers_in_every_relation_source_context() {
    assert_cases(&[
        SqlCase::new(
            "UPDATE FROM",
            "update target set value=source.value from source natural left outer join tenant where target.id=source.id;",
            "UPDATE target\nSET value = source.value\nFROM source\nNATURAL LEFT OUTER JOIN tenant\nWHERE target.id = source.id;",
        ),
        SqlCase::new(
            "DELETE USING",
            "delete from target using source natural right outer join tenant where target.id=source.id;",
            "DELETE FROM target\nUSING source\nNATURAL RIGHT OUTER JOIN tenant\nWHERE target.id = source.id;",
        ),
        SqlCase::new(
            "MERGE USING",
            "merge into target using (source natural full outer join tenant) joined on true when matched then delete;",
            "MERGE INTO target\nUSING (\n    source\n    NATURAL FULL OUTER JOIN tenant\n) joined\nON TRUE\n\nWHEN MATCHED THEN DELETE;",
        ),
    ]);
}

#[test]
fn formats_lateral_derived_and_parenthesized_join_sources() {
    assert_cases(&[
        SqlCase::new(
            "lateral derived source",
            "select * from a left join lateral (select * from b where b.a_id=a.id) b on true;",
            "SELECT *\nFROM a\nLEFT JOIN LATERAL (SELECT * FROM b WHERE b.a_id = a.id) b ON TRUE;",
        ),
        SqlCase::new(
            "parenthesized join tree",
            "select * from (a join b on a.id=b.id) ab join c on c.id=ab.id;",
            "SELECT *\nFROM (\n    a\n    JOIN b ON a.id = b.id\n) ab\nJOIN c ON c.id = ab.id;",
        ),
    ]);
}

#[test]
fn unsupported_join_neighbors_remain_byte_identical() {
    assert_unsupported_cases(&[
        (
            "JOIN USING alias",
            "SELECT * FROM a JOIN b USING (id) AS matched;",
        ),
        (
            "join alias column list",
            "SELECT * FROM (a JOIN b ON a.id = b.id) AS joined(id);",
        ),
    ]);
}

#[test]
fn inline_join_predicates_remain_inline() {
    assert_sql(
        "select * from a join b on a.id=b.a_id and a.tenant_id=b.tenant_id;",
        "SELECT *\nFROM a\nJOIN b ON a.id = b.a_id AND a.tenant_id = b.tenant_id;",
    );
}
