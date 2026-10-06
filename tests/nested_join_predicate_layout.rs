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
        let predicate_indent = lines[join].len() - lines[join].trim_start().len() + 4;
        for level in 0..wrappers {
            assert_eq!(
                lines[join + 1 + level],
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
