mod support;

#[test]
fn drops_routines_with_reviewed_signatures() {
    for (source, expected) in [
        (
            "drop function if exists demo.normalize(integer,text) cascade;",
            "DROP FUNCTION IF EXISTS demo.normalize(integer, text) CASCADE;",
        ),
        (
            "drop procedure demo.load_rows();",
            "DROP PROCEDURE demo.load_rows();",
        ),
        (
            "drop function demo.no_signature;",
            "DROP FUNCTION demo.no_signature;",
        ),
        (
            "drop procedure if exists demo.load_rows(integer),demo.save_rows(text) restrict;",
            "DROP PROCEDURE IF EXISTS demo.load_rows(integer), demo.save_rows(text) RESTRICT;",
        ),
        (
            "drop function demo.\"MixedName\"(timestamp with time zone, integer[]);",
            "DROP FUNCTION demo.\"MixedName\"(timestamp with time zone, integer[]);",
        ),
        (
            "drop function demo.normalize(/* keep type */ integer);",
            "DROP FUNCTION demo.normalize(/* keep type */ integer);",
        ),
    ] {
        support::assert_sql(source, expected);
    }
}

#[test]
fn formats_plain_rollback_spellings() {
    for (source, expected) in [
        ("rollback;", "ROLLBACK;"),
        ("rollback work;", "ROLLBACK WORK;"),
        (
            "rollback transaction and no chain;",
            "ROLLBACK TRANSACTION AND NO CHAIN;",
        ),
        ("rollback /* retained */;", "ROLLBACK /* retained */;"),
    ] {
        support::assert_sql(source, expected);
    }
}

#[test]
fn unreviewed_routine_and_transaction_neighbors_stay_unsupported() {
    for source in [
        "DROP ROUTINE demo.normalize(integer);",
        "DROP AGGREGATE demo.total(integer);",
        "DROP FUNCTION demo.normalize(value integer);",
        "DROP PROCEDURE demo.load_rows(OUT integer);",
        "ROLLBACK AND CHAIN;",
        "ROLLBACK TO SAVEPOINT sample_point;",
    ] {
        support::assert_unsupported(source);
    }
}

#[test]
fn migration_utilities_share_strict_document_and_host_formatting() {
    let options = semblock::FormatOptions {
        unsupported_policy: semblock::UnsupportedPolicy::Error,
        ..Default::default()
    };
    support::assert_sql_with(
        "begin;\ndrop function demo.normalize(integer);\nrollback;\n",
        "BEGIN;\nDROP FUNCTION demo.normalize(integer);\nROLLBACK;\n",
        &options,
    );
    let source = "drop function demo.normalize(integer);\nROLLBACK AND CHAIN;";
    for (language, host) in [
        (
            semblock::source::Language::Go,
            "package demo\nconst query = `drop function demo.normalize(integer); rollback;`\n",
        ),
        (
            semblock::source::Language::Rust,
            "const QUERY: &str = \"drop function demo.normalize(integer); rollback;\";\n",
        ),
    ] {
        let result = semblock::source::format_source(
            host,
            language,
            &Default::default(),
            &Default::default(),
        )
        .unwrap();
        assert!(
            result
                .output
                .contains("DROP FUNCTION demo.normalize(integer); ROLLBACK;"),
            "{language:?}: {} {:?}",
            result.output,
            result.diagnostics
        );
        assert!(result.output_diagnostics.iter().all(|d| d.fix_available));
    }
    let result = semblock::format_sql_result(source, &options);
    assert_eq!(result.output, source);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.rule_id == "syntax.unsupported" && d.severity == semblock::Severity::Error)
    );
}
