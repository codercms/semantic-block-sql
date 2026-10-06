use semblock::{FormatOptions, check_sql, format_sql_result, validate_equivalent};

fn reviewed(source: &str) -> String {
    assert!(pg_query::parse(source).is_ok(), "invalid fixture: {source}");
    let options = FormatOptions::default();
    let result = format_sql_result(source, &options);
    assert!(
        result.diagnostics.iter().all(|d| d.fix_available),
        "{:?}",
        result.diagnostics
    );
    if !source.starts_with("DO ") {
        validate_equivalent(source, &result.output).unwrap();
    }
    assert_eq!(
        format_sql_result(&result.output, &options).output,
        result.output
    );
    assert!(check_sql(&result.output, &options).compliant);
    result.output
}

#[test]
fn temporal_type_suffixes_are_lowercase_across_declarations_and_casts() {
    for datatype in [
        "timestamp with time zone",
        "timestamp(3) with time zone",
        "time with time zone",
        "time(2) with time zone",
        "timestamp without time zone",
        "time without time zone",
    ] {
        for source in [
            format!("CREATE TABLE sample_rows (created_at {datatype} DEFAULT now() NOT NULL);"),
            format!("ALTER TABLE sample_rows ADD COLUMN created_at {datatype};"),
            format!("SELECT NULL::{datatype}, CAST(NULL AS {datatype});"),
            format!(
                "CREATE FUNCTION sample_fn(value {datatype}) RETURNS {datatype} LANGUAGE SQL RETURN value;"
            ),
            format!("DO $$ DECLARE value {datatype}; BEGIN value := NULL::{datatype}; END; $$;"),
        ] {
            let output = reviewed(&source);
            assert!(output.contains(datatype), "{output}");
            assert!(!output.contains("WITH time zone"), "{output}");
            if source.contains("DEFAULT now()") {
                assert!(output.contains("DEFAULT NOW()"), "{output}");
            }
        }
    }
}

#[test]
fn temporal_type_comments_keep_the_suffix_casing_and_attachment() {
    for datatype in [
        "timestamp(3) /* modifier */ WITH TIME ZONE",
        "time -- modifier\nWITH TIME ZONE",
        "timestamp /* modifier */ WITHOUT TIME ZONE",
    ] {
        let output = reviewed(&format!(
            "CREATE TABLE sample_rows (created_at {datatype});"
        ));
        assert!(
            !output.contains("WITH") && !output.contains("TIME") && !output.contains("ZONE"),
            "{output}"
        );
        if datatype.contains("-- modifier") {
            assert!(output.contains("-- modifier\n"), "{output}");
        }
        if datatype.contains("/* modifier */") {
            assert!(output.contains("/* modifier */"), "{output}");
        }
    }
}

#[test]
fn unrelated_with_and_at_time_zone_remain_uppercase() {
    let output = reviewed(
        "with sample as (select now() at time zone 'UTC' as local_value) select * from sample;",
    );
    assert!(output.starts_with("WITH sample"), "{output}");
    assert!(output.contains("NOW() AT TIME ZONE 'UTC'"), "{output}");
}
