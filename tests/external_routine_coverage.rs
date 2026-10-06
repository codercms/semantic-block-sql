mod support;

use semblock::{FormatOptions, format_sql, validate_equivalent};

#[test]
fn external_routine_headers_preserve_literal_arguments() {
    for source in [
        "CREATE FUNCTION sample_external(integer) RETURNS text LANGUAGE C IMMUTABLE STRICT AS '$libdir/sample_library', 'SampleEntry';",
        "CREATE FUNCTION sample_external() RETURNS integer AS '$libdir/sample_library' LANGUAGE c VOLATILE SECURITY DEFINER PARALLEL UNSAFE;",
        "CREATE FUNCTION sample_internal(integer) RETURNS integer LANGUAGE internal IMMUTABLE STRICT AS 'sample_entry';",
        "CREATE FUNCTION sample_external(sample_value integer DEFAULT 1) RETURNS integer LANGUAGE C AS '$libdir/sample_library', /* retained symbol comment */ 'sample_entry' COST 10 SET search_path = public;",
        "CREATE FUNCTION sample_external(integer) RETURNS integer\n-- retained header comment   \nLANGUAGE C AS '$libdir/sample_library', 'sample_entry';",
    ] {
        let result = format_sql(source, &FormatOptions::default()).unwrap();
        assert!(
            result
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.fix_available),
            "{:?}",
            result.diagnostics
        );
        validate_equivalent(source, &result.output).unwrap();
        support::assert_sql_layout_only(&result.output, &result.output);
    }
}

#[test]
fn long_external_literal_lists_wrap_without_splitting_literals() {
    let library = "sample_library_".repeat(6);
    let symbol = "sample_entry_".repeat(7);
    let source = format!(
        "CREATE FUNCTION sample_external(integer) RETURNS integer LANGUAGE C AS '{library}', '{symbol}';"
    );
    let result = format_sql(&source, &FormatOptions::default()).unwrap();
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.fix_available),
        "{:?}",
        result.diagnostics
    );
    assert!(
        result
            .output
            .lines()
            .all(|line| line.chars().count() <= 160)
    );
    validate_equivalent(&source, &result.output).unwrap();
    support::assert_sql_layout_only(&result.output, &result.output);
}

#[test]
fn unreviewed_languages_remain_byte_identical() {
    let source =
        "CREATE FUNCTION sample_unknown() RETURNS integer LANGUAGE plpython3u AS $$ return 1 $$;";
    let result = format_sql(source, &FormatOptions::default()).unwrap();
    assert_eq!(result.output, source);
    assert!(
        result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.rule_id == "syntax.unsupported")
    );
}
