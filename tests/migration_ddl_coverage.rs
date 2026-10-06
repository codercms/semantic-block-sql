mod support;

fn assert_supported(source: &str) {
    let result = semblock::format_sql(source, &semblock::FormatOptions::default()).unwrap();
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.fix_available),
        "{:?}",
        result.diagnostics
    );
    semblock::validate_equivalent(source, &result.output).unwrap();
    assert!(
        result
            .output
            .lines()
            .all(|line| line.chars().count() <= 160)
    );
    support::assert_sql_layout_only(&result.output, &result.output);
}

#[test]
fn trigger_transition_tables_bind_old_new_and_keyword_aliases() {
    for referencing in [
        "OLD TABLE AS before",
        "NEW TABLE AS after",
        "OLD TABLE AS before NEW TABLE AS after",
    ] {
        let source = format!(
            "CREATE TRIGGER sample_changes AFTER UPDATE ON sample_rows\nREFERENCING {referencing}\nFOR EACH STATEMENT EXECUTE FUNCTION sample_handler();"
        );
        assert_supported(&source);
    }
}

#[test]
fn sequence_option_locations_preserve_order_comments_and_owned_names() {
    assert_supported(
        "CREATE SEQUENCE sample_counter\n    INCREMENT BY -2\n\n    -- retained option group\n    NO MINVALUE\n    NO MAXVALUE\n    START WITH 10\n    CACHE 5\n    CYCLE\n    OWNED BY sample_schema.sample_rows.start;",
    );
}

#[test]
fn foreign_key_actions_keep_authored_clause_boundaries() {
    assert_supported(
        "ALTER TABLE sample_rows\n    ADD CONSTRAINT sample_reference\n        FOREIGN KEY (first_id, second_id)\n        REFERENCES sample_parent (first_id, second_id)\n        MATCH FULL\n        ON UPDATE SET DEFAULT\n\n        -- retained delete action\n        ON DELETE SET NULL\n        DEFERRABLE INITIALLY DEFERRED NOT VALID;",
    );
}

#[test]
fn trigger_headers_cover_timing_events_conditions_and_arguments() {
    let name = "sample_".repeat(8);
    for timing in [
        "BEFORE INSERT OR UPDATE",
        "AFTER DELETE",
        "INSTEAD OF UPDATE",
    ] {
        let source = format!(
            "CREATE TRIGGER {name}changes {timing} ON sample_rows FOR EACH ROW WHEN (sample_test(1)) EXECUTE PROCEDURE sample_handler('keep this argument');"
        );
        assert_supported(&source);
    }
}

#[test]
fn foreign_key_default_and_explicit_actions_have_fixture_backed_ownership() {
    for action in [
        "",
        "ON DELETE NO ACTION",
        "ON UPDATE RESTRICT",
        "ON UPDATE CASCADE ON DELETE SET NULL",
        "ON UPDATE SET DEFAULT NOT DEFERRABLE",
    ] {
        assert_supported(&format!(
            "ALTER TABLE sample_rows ADD FOREIGN KEY (id) REFERENCES sample_parent {action};"
        ));
    }
}

#[test]
fn trigger_transition_rows_remain_an_explicit_unsupported_boundary() {
    let source = "CREATE TRIGGER sample_changes AFTER UPDATE ON sample_rows REFERENCING NEW ROW AS changed FOR EACH ROW EXECUTE FUNCTION sample_handler();";
    let result = semblock::format_sql_result(source, &semblock::FormatOptions::default());
    assert_eq!(result.output, source);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.rule_id == "syntax.unsupported")
    );
}

#[test]
fn identity_options_cover_both_generation_modes_and_column_declarations() {
    for mode in ["ALWAYS", "BY DEFAULT"] {
        for options in [
            "",
            " (START WITH 2 INCREMENT BY -1 NO MINVALUE NO MAXVALUE CACHE 5 CYCLE)",
            " (\nSTART WITH 2\n\n-- retained sequence option group\nCACHE 10\n)",
        ] {
            assert_supported(&format!(
                "CREATE TABLE sample_rows (id bigint GENERATED {mode} AS IDENTITY{options}, label text);"
            ));
            assert_supported(&format!(
                "ALTER TABLE sample_rows ALTER COLUMN id ADD GENERATED {mode} AS IDENTITY{options};"
            ));
        }
    }
}
