mod support;

use semblock::{FormatOptions, format_sql, validate_equivalent};
use support::{assert_sql_layout_only, assert_unsupported};

fn assert_reviewed(source: &str) {
    let result = format_sql(source, &FormatOptions::default()).expect("reviewed SQL formats");
    assert!(
        result
            .diagnostics
            .iter()
            .all(|diagnostic| diagnostic.fix_available),
        "{:?}",
        result.diagnostics
    );
    assert!(result.warnings.is_empty());
    validate_equivalent(source, &result.output).expect("semantics preserved");
    assert_sql_layout_only(&result.output, &result.output);
}

#[test]
fn aggregate_star_signature_and_reviewed_options_are_owned() {
    assert_reviewed(
        "CREATE AGGREGATE sample_count(*) (\n    SFUNC = int8inc,\n    STYPE = bigint,\n    INITCOND = '0',\n    PARALLEL = safe\n);",
    );
    assert_reviewed(
        "CREATE OR REPLACE AGGREGATE sample_total(integer) (\n    SFUNC = int4pl,\n    STYPE = integer,\n\n    -- retain option grouping\n    COMBINEFUNC = int4pl,\n    INITCOND = '0',\n    PARALLEL = safe\n);",
    );
}

#[test]
fn setting_forms_preserve_values_and_local_scope() {
    assert_reviewed(
        "SET LOCAL statement_timeout TO '2s';\nSET search_path TO sample_schema, public;\nSET statement_timeout TO DEFAULT;\nSET statement_timeout FROM CURRENT;",
    );
    assert_unsupported("SET TRANSACTION ISOLATION LEVEL SERIALIZABLE;");
}

#[test]
fn index_attachment_is_a_closed_utility_family() {
    assert_reviewed("ALTER INDEX sample_parent ATTACH PARTITION sample_child;");
    assert_unsupported("ALTER INDEX sample_parent SET TABLESPACE sample_space;");
}

#[test]
fn ordered_set_aggregate_remains_unsupported() {
    assert_unsupported(
        "CREATE AGGREGATE sample_percentile(float8 ORDER BY float8) (SFUNC = ordered_set_transition, STYPE = internal, FINALFUNC = percentile_cont_final);",
    );
}

#[test]
fn aggregate_options_have_fixture_backed_ownership() {
    for option in [
        "SSPACE = 16",
        "FINALFUNC = sample_final",
        "FINALFUNC_EXTRA",
        "FINALFUNC_MODIFY = read_only",
        "COMBINEFUNC = sample_combine",
        "SERIALFUNC = sample_serialize",
        "DESERIALFUNC = sample_deserialize",
        "MSFUNC = sample_forward",
        "MINVFUNC = sample_inverse",
        "MSTYPE = integer",
        "MSSPACE = 16",
        "MFINALFUNC = sample_final",
        "MFINALFUNC_EXTRA",
        "MFINALFUNC_MODIFY = read_only",
        "MINITCOND = '0'",
        "SORTOP = <",
        "PARALLEL = safe",
    ] {
        assert_reviewed(&format!(
            "CREATE AGGREGATE sample_total(integer) (\n    SFUNC = int4pl,\n    STYPE = integer,\n    INITCOND = '0',\n    {option}\n);"
        ));
    }
}
