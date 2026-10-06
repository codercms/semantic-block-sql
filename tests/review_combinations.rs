use semblock::{FormatOptions, TypeAliasFamily, check_sql, format_sql_result};

#[test]
fn over_width_unsupported_leaves_keep_diagnostics_and_sibling_width_checks() {
    use semblock::{Severity, UnsupportedPolicy};
    for width in [80, 160] {
        let predicate =
            "first_identifier = second_identifier AND third_identifier = fourth_identifier";
        let predicate = if width == 160 {
            format!("{predicate} AND {predicate}")
        } else {
            predicate.into()
        };
        for newline in ["\n", "\r\n"] {
            let leaf = format!(
                "SELECT json_value(payload, '$.id')\nINTO x FROM sample_rows\nWHERE {predicate};"
            );
            let source =
                format!("-- café\nSELECT 1;\nDO $$ DECLARE x text; BEGIN\n{leaf}\nSELECT first_identifier INTO x FROM sample_rows WHERE {predicate};\nEND; $$;")
                    .replace("\n", newline);
            let leaf = leaf.replace('\n', newline);
            for policy in [UnsupportedPolicy::Skip, UnsupportedPolicy::Error] {
                let options = FormatOptions {
                    soft_line_width: width,
                    hard_line_width: width,
                    unsupported_policy: policy,
                    ..FormatOptions::default()
                };
                let result = format_sql_result(&source, &options);
                assert!(result.output.contains(&leaf), "{}", result.output);
                let diagnostic = result
                    .diagnostics
                    .iter()
                    .find(|d| d.rule_id == "syntax.unsupported")
                    .expect("unsupported leaf diagnostic");
                assert!(
                    !result.diagnostics.iter().any(|d| matches!(
                        d.rule_id.as_str(),
                        "format.statement_skipped" | "layout.hard_line_width"
                    )),
                    "{:?}",
                    result.diagnostics
                );
                assert!(diagnostic.source_range.start >= source.find("SELECT json_value").unwrap());
                assert!(
                    diagnostic.source_range.end <= source.find("SELECT first_identifier").unwrap()
                );
                assert_eq!(
                    format_sql_result(&result.output, &options).output,
                    result.output
                );
                if policy == UnsupportedPolicy::Error {
                    assert_eq!(result.output, source);
                    assert_eq!(diagnostic.severity, Severity::Error);
                } else {
                    assert!(
                        result
                            .output
                            .replace(&leaf, "")
                            .lines()
                            .all(|line| line.chars().count() <= width),
                        "{}",
                        result.output
                    );
                }
            }
        }
    }
    let options = FormatOptions {
        soft_line_width: 80,
        hard_line_width: 80,
        ..FormatOptions::default()
    };
    let long_identifier = "identifier_".repeat(9);
    let source = format!(
        "DO $$ DECLARE x text; BEGIN\nSELECT json_value(payload, '$.id') INTO x FROM sample_rows;\nSELECT {long_identifier} INTO x FROM sample_rows;\nEND; $$;"
    );
    let result = format_sql_result(&source, &options);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.rule_id == "syntax.unsupported")
    );
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.rule_id == "layout.hard_line_width"),
        "{:?}",
        result.diagnostics
    );
}

#[test]
fn multiline_unsupported_into_leaves_remain_protected() {
    use semblock::UnsupportedPolicy;
    for leaf in [
        "SELECT json_value(payload, '$.id')\nINTO x FROM sample_rows;",
        "SELECT json_value(payload, '$.id') -- value \n  INTO x FROM sample_rows;",
        "SELECT payload INTO x\nFROM sample_rows -- source\nWHERE json_value(payload, '$.id') IS NULL;",
        "\tSELECT json_value(payload, '$.id')\n\n\tINTO x FROM sample_rows; -- attached ",
        "SELECT json_value(payload, '$.id')\nFROM sample_rows;",
    ] {
        for newline in ["\n", "\r\n"] {
            let source = format!("-- café\nDO $$ DECLARE x text; BEGIN\n{leaf}\nEND; $$;")
                .replace('\n', newline);
            for policy in [UnsupportedPolicy::Skip, UnsupportedPolicy::Error] {
                let options = FormatOptions {
                    unsupported_policy: policy,
                    ..FormatOptions::default()
                };
                let result = format_sql_result(&source, &options);
                assert!(
                    result.output.contains(&leaf.replace('\n', newline)),
                    "{}",
                    result.output
                );
                assert!(
                    !result
                        .diagnostics
                        .iter()
                        .any(|d| d.rule_id == "format.statement_skipped"),
                    "{:?}",
                    result.diagnostics
                );
                let diagnostic = result
                    .diagnostics
                    .iter()
                    .find(|d| d.rule_id == "syntax.unsupported")
                    .expect("unsupported SQL leaf");
                assert!(diagnostic.source_range.start >= source.find("SELECT").unwrap());
                assert!(diagnostic.source_range.end <= source.find("END;").unwrap());
                let second = format_sql_result(&result.output, &options);
                assert_eq!(second.output, result.output);
                assert!(
                    second
                        .diagnostics
                        .iter()
                        .any(|d| d.rule_id == "syntax.unsupported")
                );
                assert!(
                    !second
                        .diagnostics
                        .iter()
                        .any(|d| d.rule_id == "format.statement_skipped")
                );
                if policy == UnsupportedPolicy::Error {
                    assert_eq!(result.output, source);
                }
            }
        }
    }
}

fn supported(source: &str, options: &FormatOptions) -> String {
    let result = format_sql_result(source, options);
    assert!(
        result
            .diagnostics
            .iter()
            .all(|d| d.fix_available || d.rule_id == "layout.hard_line_width"),
        "{:?}",
        result.diagnostics
    );
    let second = format_sql_result(&result.output, options);
    assert_eq!(second.output, result.output);
    assert!(
        check_sql(&result.output, options).compliant,
        "{:?}",
        second.diagnostics
    );
    result.output
}

#[test]
fn normalized_headers_rebind_all_sql_body_shapes() {
    for body in [
        "RETURN a;",
        "BEGIN ATOMIC SELECT a; END;",
        "AS $$ SELECT a; $$;",
    ] {
        let source = format!(
            "CREATE FUNCTION sample_value(\n-- note \na int\n) RETURNS int LANGUAGE SQL {body}"
        );
        let output = supported(&source, &FormatOptions::default());
        assert!(output.contains("-- note\n"));
    }
}

#[test]
fn sql_body_inline_comments_remain_with_their_statement() {
    for body in [
        "BEGIN ATOMIC\nSELECT 1; -- first statement\nSELECT 2;\nEND",
        "AS $$\nSELECT 1; -- first statement\nSELECT 2;\n$$",
    ] {
        let output = supported(
            &format!("CREATE FUNCTION sample_value() RETURNS int LANGUAGE SQL {body};"),
            &FormatOptions::default(),
        );
        assert!(
            output.contains("SELECT 1; -- first statement\n"),
            "{output}"
        );
    }
}

#[test]
fn atomic_body_preserves_multiline_literal_bytes() {
    let source = "CREATE FUNCTION sample_value() RETURNS text LANGUAGE SQL BEGIN ATOMIC SELECT 'first\nsecond'; END;";
    let output = supported(source, &FormatOptions::default());
    assert!(output.contains("'first\nsecond'"), "{output}");
}

#[test]
fn values_cte_and_derived_owners_can_coexist() {
    supported(
        "WITH c AS (VALUES (1)) SELECT * FROM c CROSS JOIN (VALUES (2)) AS v(n);",
        &FormatOptions::default(),
    );
}

#[test]
fn procedural_into_survives_type_alias_token_changes() {
    let mut options = FormatOptions::default();
    options
        .type_aliases
        .insert(TypeAliasFamily::CharacterVarying, "varchar".into());
    let output = supported(
        "DO $$ DECLARE x varchar; BEGIN SELECT NULL::character varying INTO x; END; $$;",
        &options,
    );
    assert!(output.contains("::varchar"), "{output}");
}

#[test]
fn procedural_into_preserves_line_comment_termination() {
    for clause in [
        "INTO x -- retained\n;",
        "INTO -- retained\nx;",
        "INTO STRICT -- retained\nx;",
        "INTO -- retained\nSTRICT x;",
    ] {
        let output = supported(
            &format!("DO $$ DECLARE x int; BEGIN SELECT 1 {clause} END; $$;"),
            &FormatOptions::default(),
        );
        assert!(output.contains("-- retained\n"), "{output}");
    }
}

#[test]
fn trigger_transition_aliases_allow_optional_as() {
    let source = "CREATE TRIGGER sample_trigger AFTER UPDATE ON sample_rows REFERENCING OLD TABLE old_rows NEW TABLE new_rows FOR EACH STATEMENT EXECUTE FUNCTION sample_fn();";
    let output = supported(source, &FormatOptions::default());
    assert!(!output.contains("TABLE AS"), "{output}");
}

#[test]
fn unsupported_into_child_retains_identity_and_leaf_coordinates() {
    let source = "DO $$ DECLARE x text;\nBEGIN SELECT json_value(payload, '$.id') INTO x FROM sample_rows; END; $$;";
    let result = format_sql_result(source, &FormatOptions::default());
    assert!(
        result
            .output
            .contains("SELECT json_value(payload, '$.id') INTO x FROM sample_rows;")
    );
    let diagnostic = result
        .diagnostics
        .iter()
        .find(|d| d.rule_id == "syntax.unsupported")
        .expect("child unsupported diagnostic");
    assert!(
        diagnostic.source_range.start >= source.find("SELECT").unwrap(),
        "{diagnostic:?}"
    );
    assert!(
        diagnostic.source_range.end <= source.find(" END;").unwrap(),
        "{diagnostic:?}"
    );
}

#[test]
fn sql_body_comments_cover_final_block_and_blank_boundaries() {
    for body in [
        "BEGIN ATOMIC\nSELECT 1; /* attached */\n\n-- next group\nSELECT 2; -- final\nEND",
        "AS $body$\nSELECT 1; /* attached */\n\n-- next group\nSELECT 2; -- final\n$body$",
    ] {
        let output = supported(
            &format!("CREATE FUNCTION sample_value() RETURNS int LANGUAGE SQL {body};"),
            &FormatOptions::default(),
        );
        assert!(
            output.contains("SELECT 1; /* attached */\n\n    -- next group"),
            "{output}"
        );
        assert!(output.contains("SELECT 2; -- final\n"), "{output}");
    }
}

#[test]
fn values_provenance_keeps_unreviewed_subquery_elements_unsupported() {
    let source = "WITH c AS (VALUES (1)) SELECT * FROM (VALUES ((SELECT n FROM (VALUES (2)) AS inner_rows(n)))) AS outer_rows(n), c;";
    let result = format_sql_result(source, &FormatOptions::default());
    assert_eq!(result.output, source);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.rule_id == "syntax.unsupported")
    );
}

#[test]
fn into_aliases_cover_expansion_modifiers_and_query_suffixes() {
    for preferred in ["varchar", "character varying"] {
        let mut options = FormatOptions::default();
        options
            .type_aliases
            .insert(TypeAliasFamily::CharacterVarying, preferred.into());
        for query in [
            "SELECT NULL::varchar(20) INTO x FROM sample_rows WHERE label::character varying IS NULL;",
            "SELECT NULL::character varying(20) INTO x;",
            "UPDATE sample_rows SET label = NULL::character varying RETURNING label::varchar INTO x;",
        ] {
            supported(
                &format!("DO $$ DECLARE x text; BEGIN {query} END; $$;"),
                &options,
            );
        }
    }
}

#[test]
fn unsupported_into_child_after_targets_maps_through_removed_span() {
    use semblock::{Severity, UnsupportedPolicy};
    let source = "-- café\r\nDO $$ DECLARE x text; BEGIN SELECT payload INTO STRICT x FROM sample_rows WHERE json_value(payload, '$.id') IS NULL; END; $$;";
    for policy in [UnsupportedPolicy::Skip, UnsupportedPolicy::Error] {
        let options = FormatOptions {
            unsupported_policy: policy,
            ..FormatOptions::default()
        };
        let result = format_sql_result(source, &options);
        let diagnostic = result
            .diagnostics
            .iter()
            .find(|d| d.rule_id == "syntax.unsupported")
            .expect("unsupported leaf");
        assert!(diagnostic.source_range.start >= source.find("SELECT").unwrap());
        assert!(diagnostic.source_range.end <= source.find(" END;").unwrap());
        assert!(
            !result
                .diagnostics
                .iter()
                .any(|d| d.rule_id == "format.statement_skipped")
        );
        if policy == UnsupportedPolicy::Error {
            assert_eq!(result.output, source);
            assert_eq!(diagnostic.severity, Severity::Error);
        }
    }
}

#[test]
fn into_target_lists_keep_authored_groups_and_wrap_at_safe_commas() {
    let options = FormatOptions {
        soft_line_width: 40,
        hard_line_width: 70,
        ..FormatOptions::default()
    };
    let source = "DO $$ DECLARE first_target_variable int; second_target_variable int; third_target_variable int; BEGIN SELECT 1, 2, 3 INTO first_target_variable,\nsecond_target_variable, third_target_variable; END; $$;";
    let output = supported(source, &options);
    assert!(output.contains("first_target_variable,\n"), "{output}");
}

#[test]
fn into_introducer_comments_keep_blank_target_boundaries() {
    let output = supported(
        "DO $$ DECLARE x int; BEGIN SELECT 1 INTO -- note\n\nx; END; $$;",
        &FormatOptions::default(),
    );
    assert!(output.contains("-- note\n\n"), "{output}");
}

#[test]
fn into_targets_keep_blank_boundaries_before_sql_suffixes() {
    for target in ["x\n\n", "x -- target\n\n"] {
        let output = supported(
            &format!("DO $$ DECLARE x int; BEGIN SELECT 1 INTO {target}FROM sample_rows; END; $$;"),
            &FormatOptions::default(),
        );
        assert!(output.contains("\n\n    FROM sample_rows"), "{output}");
    }
}
