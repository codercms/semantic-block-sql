mod support;

use semblock::{FormatOptions, check_sql, format_sql, validate_equivalent};
use support::assert_sql;

#[derive(Debug)]
struct Keyword<'a> {
    name: &'a str,
    category: &'a str,
    bare_label: &'a str,
}

// Copied from PostgreSQL 17's kwlist.h as vendored by the pinned pg_query
// 6.1.1 dependency. The completeness test makes version drift explicit.
fn postgresql_keywords() -> Vec<Keyword<'static>> {
    include_str!("fixtures/postgresql17-keywords.tsv")
        .lines()
        .skip(1)
        .map(|line| {
            let mut fields = line.split('\t');
            Keyword {
                name: fields.next().expect("keyword"),
                category: fields.next().expect("category"),
                bare_label: fields.next().expect("bare-label status"),
            }
        })
        .collect()
}

fn unquoted_relation_alias_keywords() -> Vec<&'static str> {
    postgresql_keywords()
        .into_iter()
        .filter(|keyword| matches!(keyword.category, "UNRESERVED_KEYWORD" | "COL_NAME_KEYWORD"))
        .map(|keyword| keyword.name)
        .collect()
}

#[test]
fn preserves_every_postgresql_keyword_allowed_as_an_unquoted_relation_alias() {
    let aliases = unquoted_relation_alias_keywords();
    let source = aliases
        .iter()
        .enumerate()
        .map(|(index, alias)| format!("SELECT 1 FROM relation_{index} {alias};"))
        .collect::<Vec<_>>()
        .join("\n");
    let formatted = format_sql(&source, &FormatOptions::default()).expect("format succeeds");

    for (index, alias) in aliases.iter().enumerate() {
        assert!(
            formatted
                .output
                .contains(&format!("relation_{index} {alias}")),
            "relation alias {alias:?} was changed:\n{}",
            formatted.output
        );
    }
    validate_equivalent(&source, &formatted.output)
        .expect("aliases remain semantically equivalent");
    assert_eq!(
        format_sql(&formatted.output, &FormatOptions::default())
            .expect("second format succeeds")
            .output,
        formatted.output
    );
    assert!(check_sql(&formatted.output, &FormatOptions::default()).compliant);
}

#[test]
fn preserves_all_alias_keywords_across_supported_relation_and_output_owners() {
    for (index, alias) in unquoted_relation_alias_keywords().iter().enumerate() {
        let source = format!(
            "SELECT 1 FROM from_{index} {alias};\n\
             SELECT 1 FROM left_{index} l JOIN join_{index} {alias} ON TRUE;\n\
             SELECT 1 FROM (SELECT 1) {alias};\n\
             SELECT 1 FROM generate_series(1, 1) {alias};\n\
             UPDATE update_{index} AS {alias} SET id = 1;\n\
             UPDATE update_from_{index} SET id = 1 FROM source_{index} {alias};\n\
             DELETE FROM delete_{index} AS {alias};\n\
             DELETE FROM delete_using_{index} USING source_{index} {alias};\n\
             MERGE INTO merge_{index} AS {alias} USING source_{index} s ON TRUE WHEN MATCHED THEN DELETE;\n\
             MERGE INTO merge_source_{index} t USING source_{index} {alias} ON TRUE WHEN MATCHED THEN DELETE;\n\
             INSERT INTO insert_{index} AS {alias} (id) VALUES (1) RETURNING id AS {alias};\n\
             SELECT 1 AS {alias};"
        );
        let formatted = format_sql(&source, &FormatOptions::default())
            .unwrap_or_else(|error| panic!("alias {alias:?} must format: {error:?}"));
        for expected in [
            format!("FROM from_{index} {alias}"),
            format!("JOIN join_{index} {alias} ON"),
            format!(") {alias};"),
            format!("generate_series(1, 1) {alias}"),
            format!("UPDATE update_{index} AS {alias} SET"),
            format!("FROM source_{index} {alias}"),
            format!("DELETE FROM delete_{index} AS {alias}"),
            format!("USING source_{index} {alias}"),
            format!("MERGE INTO merge_{index} AS {alias}"),
            format!("USING source_{index} {alias} ON"),
            format!("INSERT INTO insert_{index} AS {alias}"),
            format!("RETURNING id AS {alias}"),
            format!("SELECT 1 AS {alias}"),
        ] {
            assert!(
                formatted.output.contains(&expected),
                "alias {alias:?} changed in {expected:?}:\n{}",
                formatted.output
            );
        }
        validate_equivalent(&source, &formatted.output)
            .unwrap_or_else(|error| panic!("alias {alias:?} changed semantics: {error:?}"));
    }
}

#[test]
fn pinned_postgresql_keyword_metadata_is_complete() {
    let keywords = postgresql_keywords();
    assert_eq!(keywords.len(), 491);
    assert_eq!(unquoted_relation_alias_keywords().len(), 390);
    assert_eq!(
        keywords
            .iter()
            .filter(|keyword| keyword.bare_label == "BARE_LABEL")
            .count(),
        452
    );
    assert_eq!(
        keywords
            .iter()
            .filter(|keyword| keyword.bare_label == "AS_LABEL")
            .count(),
        39
    );
}

#[test]
fn preserves_every_postgresql_keyword_as_an_explicit_output_alias() {
    let keywords = postgresql_keywords();
    let source = keywords
        .iter()
        .enumerate()
        .map(|(index, keyword)| format!("SELECT {index} AS {};", keyword.name))
        .collect::<Vec<_>>()
        .join("\n");
    let formatted = format_sql(&source, &FormatOptions::default()).expect("format succeeds");

    for (index, keyword) in keywords.iter().enumerate() {
        assert!(
            formatted
                .output
                .contains(&format!("SELECT {index} AS {};", keyword.name)),
            "explicit output alias {:?} was changed:\n{}",
            keyword.name,
            formatted.output
        );
    }
    validate_equivalent(&source, &formatted.output).expect("aliases remain equivalent");
}

#[test]
fn preserves_every_postgresql_bare_label_as_an_implicit_output_alias() {
    let keywords = postgresql_keywords()
        .into_iter()
        .filter(|keyword| keyword.bare_label == "BARE_LABEL")
        .collect::<Vec<_>>();
    let source = keywords
        .iter()
        .enumerate()
        .map(|(index, keyword)| format!("SELECT {index} {};", keyword.name))
        .collect::<Vec<_>>()
        .join("\n");
    let formatted = format_sql(&source, &FormatOptions::default()).expect("format succeeds");

    for (index, keyword) in keywords.iter().enumerate() {
        assert!(
            formatted
                .output
                .contains(&format!("SELECT {index} {};", keyword.name)),
            "implicit output alias {:?} was changed:\n{}",
            keyword.name,
            formatted.output
        );
    }
}

#[test]
fn preserves_keyword_aliases_in_every_supported_relation_shape() {
    let source = "SELECT * FROM a no, b filter;
SELECT * FROM a no INNER JOIN b filter ON TRUE LEFT JOIN c range ON TRUE RIGHT JOIN d values ON TRUE FULL JOIN e update ON TRUE CROSS JOIN f comment NATURAL JOIN g key;
SELECT * FROM (a no JOIN b filter ON TRUE) values;
SELECT * FROM ROWS FROM (jsonb_each_text(key) AS (key text, value text), generate_series(1, 2)) AS no(key, value, range);
SELECT * FROM items AS no(key, value) TABLESAMPLE system (10);
SELECT * FROM jsonb_each_text(key) AS no(key text, value text);
CREATE VIEW alias_view(no, filter, range) AS SELECT 1, 2, 3;";
    let formatted = format_sql(source, &FormatOptions::default()).expect("format succeeds");

    for expected in [
        "a no,",
        "b filter;",
        "FROM a no",
        "JOIN b filter ON",
        "LEFT JOIN c range",
        "RIGHT JOIN d values",
        "FULL JOIN e update",
        "CROSS JOIN f comment",
        "NATURAL JOIN g key",
        ") values;",
        "AS (key text, value text)",
        "AS no (key, value, range)",
        "AS no (key, value) TABLESAMPLE",
        "AS no (key text, value text)",
        "alias_view (no, filter, range)",
    ] {
        assert!(
            formatted.output.contains(expected),
            "alias shape {expected:?} changed:\n{}",
            formatted.output
        );
    }
    validate_equivalent(source, &formatted.output).expect("aliases remain equivalent");
}

#[test]
fn alias_roles_do_not_suppress_keyword_casing_in_grammar_positions() {
    assert_sql(
        "select count(*) filter (where true), sum(value) over (order by value range between unbounded preceding and current row) from data for no key update;",
        "SELECT COUNT(*) FILTER (WHERE TRUE), SUM(value) OVER (\n        ORDER BY value\n        RANGE BETWEEN UNBOUNDED PRECEDING AND CURRENT ROW\n    )\nFROM data\nFOR NO KEY UPDATE;",
    );
    assert_sql(
        "insert into target (value) values (1) on conflict do update set value=excluded.value returning value;",
        "INSERT INTO target (value)\nVALUES (1)\nON CONFLICT\nDO UPDATE\nSET\n    value = EXCLUDED.value\nRETURNING value;",
    );
}

#[test]
fn preserves_keyword_aliases_in_supported_alias_positions() {
    assert_sql(
        "with no(filter) as (select id as range from offers) select no.filter as comment from no join users update on update.offer_id=no.filter join (select offer_id from flags) values on values.offer_id=no.filter join generate_series(1, 1) filter on true window range as (partition by no.filter);",
        "WITH no (filter) AS (\n    SELECT id AS range\n    FROM offers\n)\nSELECT no.filter AS comment\nFROM no\nJOIN users update ON update.offer_id = no.filter\nJOIN (SELECT offer_id FROM flags) values ON values.offer_id = no.filter\nJOIN generate_series(1, 1) filter ON TRUE\nWINDOW range AS (PARTITION BY no.filter);",
    );

    assert_sql(
        "update numbered_offers no set offer_id=no.offer_id returning no.offer_id as comment;",
        "UPDATE numbered_offers no SET offer_id = no.offer_id RETURNING no.offer_id AS comment;",
    );
    assert_sql(
        "delete from numbered_offers no using users filter where filter.offer_id=no.offer_id returning no.offer_id as comment;",
        "DELETE FROM numbered_offers no\nUSING users filter\nWHERE filter.offer_id = no.offer_id\nRETURNING no.offer_id AS comment;",
    );
    assert_sql(
        "merge into numbered_offers no using users filter on filter.offer_id=no.offer_id when matched then delete returning no.offer_id as comment;",
        "MERGE INTO numbered_offers no\nUSING users filter ON filter.offer_id = no.offer_id\n\nWHEN MATCHED THEN DELETE\nRETURNING no.offer_id AS comment;",
    );
    assert_sql(
        "insert into numbered_offers as no (offer_id) values (1) returning no.offer_id as comment;",
        "INSERT INTO numbered_offers AS no (offer_id) VALUES (1) RETURNING no.offer_id AS comment;",
    );
}
