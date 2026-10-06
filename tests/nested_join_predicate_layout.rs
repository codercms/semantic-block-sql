use semblock::{FormatOptions, check_sql, format_sql_result, validate_equivalent};

#[test]
fn expanded_join_predicates_show_every_enclosing_wrapper_level() {
    for (wrappers, context) in [(2, 0), (3, 0), (2, 1), (3, 2)] {
        let source = format!(
            "SELECT *\nFROM (\nleft_rows a\nJOIN right_rows b ON\n{}(b.id > a.last_id)\nAND (b.id <= (\nSELECT id\nFROM upper_bound\n)){}\n);",
            "(".repeat(wrappers),
            ")".repeat(wrappers),
        );
        let source = match context {
            1 => format!(
                "WITH prepared AS NOT MATERIALIZED ({}) SELECT * FROM prepared;",
                source.trim_end_matches(';')
            ),
            2 => format!("INSERT INTO destination {source}"),
            _ => source,
        };
        let options = FormatOptions::default();
        let result = format_sql_result(&source, &options);
        assert!(
            result.diagnostics.iter().all(|d| d.fix_available),
            "{:?}",
            result.diagnostics
        );
        let lines = result.output.lines().collect::<Vec<_>>();
        let join = lines
            .iter()
            .position(|line| line.contains("JOIN right_rows b ON"))
            .unwrap();
        let predicate_indent = lines[join].len() - lines[join].trim_start().len();
        assert!(lines[join].ends_with("ON ("), "{}", result.output);
        for level in 1..wrappers {
            assert_eq!(
                lines[join + level],
                format!("{}(", " ".repeat(predicate_indent + level * 4)),
                "{}",
                result.output
            );
        }
        let comparison = lines
            .iter()
            .find(|line| line.trim_start().starts_with("(b.id >"))
            .unwrap();
        let connector = lines
            .iter()
            .find(|line| line.trim_start().starts_with("AND "))
            .unwrap();
        assert_eq!(connector.trim(), "AND (", "{}", result.output);
        let comparison_rhs = lines
            .iter()
            .find(|line| line.trim_start().starts_with("b.id <= ("))
            .unwrap();
        assert_eq!(
            comparison_rhs.len() - comparison_rhs.trim_start().len(),
            predicate_indent + (wrappers + 1) * 4,
            "{}",
            result.output
        );
        assert_eq!(
            comparison.len() - comparison.trim_start().len(),
            predicate_indent + wrappers * 4,
            "{}",
            result.output
        );
        assert_eq!(
            connector.len() - connector.trim_start().len(),
            predicate_indent + wrappers * 4,
            "{}",
            result.output
        );
        validate_equivalent(&source, &result.output).unwrap();
        assert_eq!(
            format_sql_result(&result.output, &options).output,
            result.output
        );
        assert!(check_sql(&result.output, &options).compliant);
    }
}

#[test]
fn subquery_comparison_wrappers_preserve_comments_and_compact_siblings() {
    let source = "SELECT * FROM rows a WHERE\n(a.id > 0)\nAND (a.id <= ( -- upper bound\nSELECT id\nFROM bounds\n)) -- comparison\nAND (a.enabled = TRUE);";
    let options = FormatOptions::default();
    let result = format_sql_result(source, &options);
    assert!(
        result.diagnostics.iter().all(|d| d.fix_available),
        "{:?}",
        result.diagnostics
    );
    assert!(result.output.contains("    AND (\n        a.id <= (-- upper bound\n            SELECT id\n            FROM bounds\n        )\n    ) -- comparison"), "{}", result.output);
    assert!(
        result.output.contains("    AND (a.enabled = TRUE)"),
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

#[test]
fn nested_exists_predicate_is_one_level_below_its_where_clause() {
    let source = "WITH prepared AS (SELECT * FROM rows a WHERE ((NOT (EXISTS (SELECT 1 FROM mutes m WHERE ((m.id = a.id) AND (m.active = TRUE))))) AND (NOT (EXISTS (SELECT 1 FROM progress p WHERE\n((p.id = a.id)\nAND (p.item = a.item)\nAND (p.part = a.part))\n))))) SELECT * FROM prepared;";
    for source in [
        source.to_owned(),
        format!(
            "CREATE FUNCTION sample() RETURNS SETOF rows LANGUAGE SQL BEGIN ATOMIC {source} END;"
        ),
    ] {
        let options = FormatOptions::default();
        let result = format_sql_result(&source, &options);
        assert!(
            result.diagnostics.iter().all(|d| d.fix_available),
            "{:?}",
            result.diagnostics
        );
        let lines = result.output.lines().collect::<Vec<_>>();
        let from = lines
            .iter()
            .position(|line| line.trim() == "FROM progress p")
            .unwrap();
        let where_indent = lines[from + 1].len() - lines[from + 1].trim_start().len();
        let predicate_indent = lines[from + 2].len() - lines[from + 2].trim_start().len();
        assert_eq!(predicate_indent, where_indent + 4, "{}", result.output);
        validate_equivalent(&source, &result.output).unwrap();
        assert_eq!(
            format_sql_result(&result.output, &options).output,
            result.output
        );
        assert!(check_sql(&result.output, &options).compliant);
    }
}
