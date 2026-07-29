use semblock::{FormatDiagnostic, FormatOptions, Severity, format_sql, format_sql_result};

#[test]
fn rejects_unreviewed_sql_json_expression_families_without_rewriting() {
    for (source, feature) in [
        (
            "select json_query(payload, '$.a');",
            "JSON query/value/exists expression",
        ),
        (
            "select json_value(payload, '$.a');",
            "JSON query/value/exists expression",
        ),
        (
            "select json_exists(payload, '$.a');",
            "JSON query/value/exists expression",
        ),
        (
            "select json_serialize(payload returning text);",
            "JSON serialization expression",
        ),
        ("select json_scalar(value);", "JSON scalar expression"),
        ("select payload is json;", "IS JSON predicate"),
        (
            "select json_object('a': value returning jsonb);",
            "advanced JSON_OBJECT constructor",
        ),
        (
            "select json_object('a': value with unique keys);",
            "advanced JSON_OBJECT constructor",
        ),
        (
            "select json_array(value null on null);",
            "advanced JSON_ARRAY constructor",
        ),
        (
            "select json_objectagg(key: value);",
            "advanced SQL/JSON expression",
        ),
        (
            "select json_arrayagg(value);",
            "advanced SQL/JSON expression",
        ),
        (
            "select json_array(select id from items);",
            "advanced SQL/JSON expression",
        ),
        ("select json(value);", "JSON parse expression"),
        (
            "select * from json_table(payload, '$[*]' columns (value text path '$.value')) jt;",
            "JSON_TABLE expression",
        ),
    ] {
        let formatted = format_sql_result(source, &FormatOptions::default());
        assert_eq!(formatted.output, source, "{source}");
        assert!(!formatted.changed, "{source}");
        assert_eq!(formatted.diagnostics.len(), 1, "{source}");
        assert_eq!(
            formatted.diagnostics[0].rule_id, "syntax.unsupported",
            "{source}"
        );
        assert_eq!(
            formatted.diagnostics[0].severity,
            Severity::Error,
            "{source}"
        );
        assert!(
            formatted.diagnostics[0].message.contains(feature),
            "diagnostic {:?} should mention {feature:?}",
            formatted.diagnostics[0]
        );
        assert!(matches!(
            format_sql(source, &FormatOptions::default()),
            Err(FormatDiagnostic::UnsupportedSyntax { .. })
        ));
    }
}
