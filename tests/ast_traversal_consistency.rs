use semblock::{FormatOptions, TypeAliasFamily, check_sql, format_sql_result, validate_equivalent};

#[test]
fn type_aliases_reach_every_nested_expression_owner() {
    let mut options = FormatOptions::default();
    options
        .type_aliases
        .insert(TypeAliasFamily::Integer, "int".into());
    options
        .type_aliases
        .insert(TypeAliasFamily::CharacterVarying, "varchar".into());
    for source in [
        "SELECT ARRAY[NULL::character varying];",
        "SELECT COUNT(*) FILTER (WHERE NULL::integer IS NULL) FROM rows;",
        "SELECT CASE NULL::integer WHEN 1 THEN 2 ELSE 3 END;",
        "CREATE TABLE sample (value integer DEFAULT NULL::character varying, CHECK (NULL::integer IS NULL));",
        "SELECT 1 LIMIT (SELECT NULL::integer);",
    ] {
        let result = format_sql_result(source, &options);
        assert!(
            result
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.fix_available),
            "{:?}",
            result.diagnostics
        );
        assert!(
            !result.output.contains("character varying") && !result.output.contains("integer"),
            "{}",
            result.output
        );
        validate_equivalent(source, &result.output).unwrap();
        assert_eq!(
            format_sql_result(&result.output, &options).output,
            result.output
        );
        assert!(check_sql(&result.output, &options).compliant);
    }
}

#[test]
fn unsupported_children_have_the_same_boundary_in_every_owner() {
    for source in [
        "SELECT json_value(payload, '$.id') FROM rows;",
        "SELECT COUNT(*) FILTER (WHERE json_value(payload, '$.id') IS NOT NULL) FROM rows;",
        "SELECT CASE json_value(payload, '$.id') WHEN '1' THEN 1 ELSE 0 END FROM rows;",
        "SELECT jsonb_agg(payload ORDER BY json_value(payload, '$.id')) FROM rows;",
        "UPDATE rows SET payload = NULL RETURNING json_value(payload, '$.id');",
    ] {
        let result = format_sql_result(source, &FormatOptions::default());
        assert_eq!(result.output, source);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.rule_id == "syntax.unsupported"),
            "{:?}",
            result.diagnostics
        );
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.rule_id == "format.statement_skipped"),
            "{:?}",
            result.diagnostics
        );
    }
}
