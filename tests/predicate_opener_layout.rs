use semblock::{FormatOptions, check_sql, format_sql_result, validate_equivalent};

#[test]
fn single_outer_predicate_group_shares_its_clause_line() {
    for source in [
        "SELECT * FROM rows WHERE\n(\na = 1\nAND b = 2\n);",
        "UPDATE rows SET value = 1 WHERE\n(\na = 1\nAND b = 2\n);",
        "DELETE FROM rows WHERE\n(\na = 1\nAND b = 2\n);",
        "INSERT INTO target SELECT * FROM rows WHERE\n(\na = 1\nAND b = 2\n);",
        "SELECT a FROM rows GROUP BY a HAVING\n(\nCOUNT(*) > 1\nAND SUM(b) > 2\n);",
        "SELECT * FROM rows a JOIN others b ON\n(\na.id = b.id\nAND b.active = TRUE\n);",
        "INSERT INTO rows VALUES (1) ON CONFLICT (id) WHERE\n(\na = 1\nAND b = 2\n) DO NOTHING;",
        "INSERT INTO rows VALUES (1) ON CONFLICT (id) DO UPDATE SET value = 1 WHERE\n(\na = 1\nAND b = 2\n);",
        "MERGE INTO rows a USING others b ON\n(\na.id = b.id\nAND b.active = TRUE\n) WHEN MATCHED THEN DELETE;",
        "CREATE INDEX idx ON rows (a) WHERE\n(\na = 1\nAND b = 2\n);",
    ] {
        let options = FormatOptions::default();
        let result = format_sql_result(source, &options);
        assert!(
            result.diagnostics.iter().all(|d| d.fix_available),
            "{:?}",
            result.diagnostics
        );
        let lines = result.output.lines().collect::<Vec<_>>();
        let owner = lines
            .iter()
            .position(|line| {
                ["WHERE (", "HAVING (", "ON ("]
                    .iter()
                    .any(|suffix| line.trim_end().ends_with(suffix))
            })
            .expect(&result.output);
        let indent = lines[owner].len() - lines[owner].trim_start().len();
        assert_eq!(
            lines[owner + 1].len() - lines[owner + 1].trim_start().len(),
            indent + 4,
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
fn disabling_the_preference_retains_the_separate_opener_layout() {
    let source = "SELECT * FROM rows WHERE\n(\na = 1\nAND b = 2\n);";
    let options = FormatOptions {
        inline_predicate_group_opener: false,
        ..FormatOptions::default()
    };
    let result = format_sql_result(source, &options);
    assert!(
        result
            .output
            .contains("WHERE\n    (\n        a = 1\n        AND b = 2\n    )"),
        "{}",
        result.output
    );
    assert!(check_sql(&result.output, &options).compliant);
    assert_eq!(
        format_sql_result(&result.output, &options).output,
        result.output
    );
    validate_equivalent(source, &result.output).unwrap();
}

#[test]
fn nested_subqueries_follow_joined_predicate_indentation() {
    let source = "SELECT * FROM rows a WHERE ((a.active = TRUE) OR EXISTS (SELECT 1 FROM others b WHERE b.id = a.id));";
    let options = FormatOptions::default();
    let result = format_sql_result(source, &options);
    assert!(
        result.diagnostics.iter().all(|d| d.fix_available),
        "{:?}",
        result.diagnostics
    );
    assert!(result.output.contains("WHERE (\n    (a.active = TRUE)\n    OR EXISTS (\n        SELECT 1\n        FROM others b\n        WHERE b.id = a.id\n    )\n)"), "{}", result.output);
    validate_equivalent(source, &result.output).unwrap();
    assert_eq!(
        format_sql_result(&result.output, &options).output,
        result.output
    );
    assert!(check_sql(&result.output, &options).compliant);
}

#[test]
fn opener_preference_respects_boundaries_and_complete_group_ownership() {
    for source in [
        "SELECT * FROM rows WHERE\n\n(\na = 1\nAND b = 2\n);",
        "SELECT * FROM rows WHERE -- retained\n(\na = 1\nAND b = 2\n);",
        "SELECT * FROM rows WHERE\n/* retained */ (\na = 1\nAND b = 2\n);",
        "SELECT * FROM rows WHERE\n(a = 1)\nAND (b = 2);",
    ] {
        let options = FormatOptions::default();
        let result = format_sql_result(source, &options);
        assert!(
            result.diagnostics.iter().all(|d| d.fix_available),
            "{:?}",
            result.diagnostics
        );
        assert!(!result.output.contains("WHERE ("), "{}", result.output);
        assert_eq!(
            source.contains("-- retained"),
            result.output.contains("-- retained")
        );
        assert_eq!(
            source.contains("/* retained */"),
            result.output.contains("/* retained */")
        );
        if source.contains("WHERE\n\n") {
            assert!(result.output.contains("WHERE\n\n"), "{}", result.output);
        }
        validate_equivalent(source, &result.output).unwrap();
        assert_eq!(
            format_sql_result(&result.output, &options).output,
            result.output
        );
        assert!(check_sql(&result.output, &options).compliant);
    }
    let source = "SELECT * FROM rows a JOIN moderately_long_table_name b ON\n(\na.id = b.id\nAND b.active = TRUE\n);";
    let options = FormatOptions {
        soft_line_width: 32,
        ..FormatOptions::default()
    };
    let result = format_sql_result(source, &options);
    assert!(
        result
            .output
            .contains("JOIN moderately_long_table_name b ON\n    ("),
        "{}",
        result.output
    );
    assert!(check_sql(&result.output, &options).compliant);
}

#[test]
fn layout_config_round_trips_and_controls_stdin_formatting() {
    use semblock::config::Config;
    use std::{
        fs,
        io::Write,
        process::{Command, Stdio},
    };
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("semblock.toml");
    assert!(Config::default().format.inline_predicate_group_opener);
    fs::write(&path, "[layout]\nsoft_line_width = 120\n").unwrap();
    assert!(
        Config::load(Some(&path))
            .unwrap()
            .format
            .inline_predicate_group_opener
    );
    for enabled in [true, false] {
        let mut config = Config::default();
        config.format.inline_predicate_group_opener = enabled;
        fs::write(&path, config.to_toml()).unwrap();
        assert_eq!(Config::load(Some(&path)).unwrap(), config);
        let mut child = Command::new(env!("CARGO_BIN_EXE_semblock"))
            .current_dir(root.path())
            .args(["fmt", "--stdin", "--language", "sql"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"SELECT * FROM rows WHERE\n(\na = 1\nAND b = 2\n);")
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{output:?}");
        let output = String::from_utf8(output.stdout).unwrap();
        assert_eq!(output.contains("WHERE ("), enabled, "{output}");
    }
    fs::write(&path, "[layout]\ninline_predicate_group_opener = 'false'\n").unwrap();
    assert!(Config::load(Some(&path)).is_err());
}
