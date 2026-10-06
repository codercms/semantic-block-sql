use semblock::{FormatOptions, TypeAliasFamily, check_sql, format_sql_result};

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
    assert_eq!(result.output, source);
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
