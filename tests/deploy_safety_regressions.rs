mod support;

#[test]
fn aggregate_alias_does_not_claim_the_same_spelled_call_name() {
    support::assert_sql(
        "SELECT count(*) AS count FROM demo.samples;",
        "SELECT COUNT(*) AS count FROM demo.samples;",
    );
}

#[test]
fn copy_payload_is_never_scanned_as_sql() {
    for payload in [
        "1\t7th avenue\n2\tO'Brien\n",
        "1\t'quoted; -- value\n2\t/* value\n",
        "1\t\\N\n2\tРусский текст\n",
    ] {
        let source = format!(
            "begin;\ncopy demo.labels(id,label) from stdin;\n{payload}\\.\nselect id from demo.labels;\ncommit;\n"
        );
        let expected = format!(
            "BEGIN;\nCOPY demo.labels(id, label) FROM STDIN;\n{payload}\\.\nSELECT id FROM demo.labels;\nCOMMIT;\n"
        );
        support::assert_sql_layout_only(&source, &expected);
    }
}

#[test]
fn partial_index_expansion_is_decided_on_the_first_pass() {
    support::assert_sql(
        "CREATE INDEX specimen_ready_idx ON demo.specimens (sample_id, position) WHERE ready;",
        "CREATE INDEX specimen_ready_idx ON demo.specimens (\n    sample_id,\n    position\n)\nWHERE ready;",
    );
}

#[test]
fn distinctness_comparison_in_boolean_target_has_a_safe_breakpoint() {
    let options = semblock::FormatOptions {
        soft_line_width: 45,
        hard_line_width: 80,
        ..Default::default()
    };
    support::assert_sql_with(
        "SELECT COALESCE(left_label, backup_left_label) IS DISTINCT FROM COALESCE(right_label, backup_right_label) OR enabled AS changed FROM demo.specimens;",
        "SELECT\n    COALESCE(left_label, backup_left_label)\n        IS DISTINCT FROM COALESCE(right_label, backup_right_label)\n    OR enabled AS changed\nFROM demo.specimens;",
        &options,
    );
}

#[test]
fn alias_ownership_preserves_names_in_queries_dml_and_atomic_routines() {
    for function in ["count", "sum", "max", "coalesce"] {
        let args = if function == "count" {
            "*"
        } else if function == "coalesce" {
            "amount, 0"
        } else {
            "amount"
        };
        for alias in [
            format!("AS {function}"),
            function.to_owned(),
            "AS \"Mixed Alias\"".into(),
        ] {
            support::assert_sql(
                &format!("SELECT {function}({args}) {alias} FROM demo.samples;"),
                &format!(
                    "SELECT {}({args}) {alias} FROM demo.samples;",
                    function.to_uppercase()
                ),
            );
        }
    }
    for source in [
        "WITH tallies AS (SELECT count(*) AS count FROM demo.samples) SELECT count FROM tallies;",
        "UPDATE demo.samples SET amount = 1 RETURNING amount AS amount;",
        "INSERT INTO demo.samples (amount) VALUES (1) RETURNING amount;",
        "CREATE FUNCTION demo.purge() RETURNS bigint LANGUAGE SQL BEGIN ATOMIC WITH removed AS (DELETE FROM demo.samples RETURNING amount) SELECT count(*) AS count FROM removed; END;",
    ] {
        let result = semblock::format_sql(source, &Default::default()).unwrap();
        assert!(
            result.diagnostics.iter().all(|d| d.fix_available),
            "{:?}",
            result.diagnostics
        );
        if source.starts_with("CREATE FUNCTION") {
            support::assert_sql_layout_only(source, &result.output);
        } else {
            support::assert_sql(source, &result.output);
        }
    }
}

#[test]
fn copy_segments_keep_alias_policies_and_warning_coordinates() {
    let payload = format!("9\t{}\n", "literal ' /* ; 7th ".repeat(20));
    let source = format!(
        "select null::real;\ncopy demo.samples(id,label) from stdin;\n{payload}\\.\nselect '{}';\ncopy demo.samples(id,label) from stdin;\n8\tother value\n\\.\nselect null::real;\n",
        "x".repeat(170)
    );
    let mut options = semblock::FormatOptions::default();
    options
        .type_aliases
        .insert(semblock::TypeAliasFamily::Real, "float4".into());
    let result = semblock::format_sql(&source, &options).unwrap();
    assert!(result.output.contains(&payload));
    assert_eq!(result.output.matches("NULL::float4").count(), 2);
    assert_eq!(result.warnings.len(), 1, "{:?}", result.warnings);
    let warning = result
        .diagnostics
        .iter()
        .find(|d| d.rule_id == "layout.hard_line_width")
        .unwrap();
    assert!(
        source[warning.source_range.start..warning.source_range.end].contains(&"x".repeat(170))
    );
    assert_eq!(
        semblock::format_sql(&result.output, &options)
            .unwrap()
            .output,
        result.output
    );
    assert!(semblock::check_sql(&result.output, &options).compliant);
}

#[test]
fn copy_preserves_crlf_payload_and_fails_atomically_for_bad_suffixes() {
    let source = "copy demo.samples(id,label) from stdin;\r\n1\t7th street\r\n\\.\r\nselect id from demo.samples;\r\n";
    let result = semblock::source::format_source(
        source,
        semblock::source::Language::Sql,
        &Default::default(),
        &Default::default(),
    )
    .unwrap();
    assert!(result.output.contains("1\t7th street\r\n\\.\r\n"));
    assert!(result.output_diagnostics.is_empty());
    for suffix in ["SELECT FROM;", "CREATE PUBLICATION demo_publication;"] {
        let source = format!("copy demo.samples(id) from stdin;\n7th\n\\.\n{suffix}");
        let options = semblock::FormatOptions {
            unsupported_policy: semblock::UnsupportedPolicy::Error,
            ..Default::default()
        };
        let result = semblock::format_sql_result(&source, &options);
        assert_eq!(result.output, source);
        assert!(
            result
                .diagnostics
                .iter()
                .any(|d| d.severity == semblock::Severity::Error && !d.fix_available)
        );
    }
}

#[test]
fn partial_indexes_cover_authored_groups_comments_and_options() {
    support::assert_sql(
        "create index sample_ready_idx on demo.samples (id) where ready;",
        "CREATE INDEX sample_ready_idx ON demo.samples (id) WHERE ready;",
    );
    for source in [
        "CREATE INDEX sample_pair_idx ON demo.samples (id, amount) WHERE ready AND amount > 0;",
        "CREATE INDEX sample_pair_idx ON demo.samples (\n id, amount\n) WHERE ready;",
        "CREATE INDEX sample_pair_idx ON demo.samples (id, -- retained key\n amount) WHERE ready;",
        "CREATE INDEX sample_pair_idx ON demo.samples (id, amount) INCLUDE (label) WITH (fillfactor = 80) WHERE ready;",
        "CREATE UNIQUE INDEX CONCURRENTLY IF NOT EXISTS \"Mixed Index\" ON demo.samples (id, amount DESC NULLS LAST) WHERE ready;",
    ] {
        let result = semblock::format_sql(source, &Default::default()).unwrap();
        assert!(
            result.diagnostics.iter().all(|d| d.fix_available),
            "{:?}",
            result.diagnostics
        );
        support::assert_sql(source, &result.output);
    }
}

#[test]
fn distinctness_comparisons_wrap_across_expression_owners() {
    let options = semblock::FormatOptions {
        soft_line_width: 45,
        hard_line_width: 80,
        ..Default::default()
    };
    for predicate in ["IS DISTINCT FROM", "IS NOT DISTINCT FROM"] {
        let expression = format!(
            "COALESCE(left_label, backup_left_label) {predicate} COALESCE(right_label, backup_right_label) OR enabled"
        );
        for source in [
            format!("SELECT {expression} AS changed FROM demo.samples;"),
            format!("SELECT id FROM demo.samples WHERE {expression};"),
            format!("UPDATE demo.samples SET ready = {expression};"),
            format!("SELECT id FROM demo.samples GROUP BY id HAVING {expression};"),
        ] {
            let result = semblock::format_sql(&source, &options).unwrap();
            assert!(
                result.diagnostics.iter().all(|d| d.fix_available),
                "{source}: {:?}",
                result.diagnostics
            );
            assert!(
                result
                    .output
                    .lines()
                    .all(|line| line.chars().count() <= options.hard_line_width)
            );
            support::assert_sql_with(&source, &result.output, &options);
        }
    }
}
