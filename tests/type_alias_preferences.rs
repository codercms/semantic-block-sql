use std::path::Path;

use semblock::config::Config;
use semblock::source::{Language, format_source};
use semblock::{FormatOptions, TypeAliasFamily, check_sql, format_sql};

#[test]
fn preserves_type_aliases_by_default() {
    let source = "SELECT NULL::integer, NULL::character varying, NULL::timestamp WITH time zone;";
    let formatted = format_sql(source, &FormatOptions::default()).expect("formatting succeeds");

    assert_eq!(formatted.output, source);
    assert!(formatted.diagnostics.is_empty(), "{formatted:#?}");
}

#[test]
fn normalizes_only_configured_type_alias_families() {
    let source = "SELECT NULL::integer, NULL::character varying, NULL::timestamp WITH time zone;";
    let mut options = FormatOptions::default();
    options
        .type_aliases
        .insert(TypeAliasFamily::Integer, "int".into());
    options
        .type_aliases
        .insert(TypeAliasFamily::CharacterVarying, "varchar".into());
    options
        .type_aliases
        .insert(TypeAliasFamily::TimestampWithTimeZone, "timestamptz".into());

    let formatted = format_sql(source, &options).expect("formatting succeeds");
    assert_eq!(
        formatted.output,
        "SELECT NULL::int, NULL::varchar, NULL::timestamptz;"
    );

    let checked = check_sql(source, &options);
    assert!(!checked.compliant);
    assert_eq!(
        checked
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.rule_id == "type.alias")
            .count(),
        3,
        "{checked:#?}"
    );

    let clean = check_sql(&formatted.output, &options);
    assert!(clean.compliant, "{clean:#?}");
}

#[test]
fn supports_reverse_preferences() {
    let source = "SELECT NULL::int, NULL::varchar(24);";
    let mut options = FormatOptions::default();
    options
        .type_aliases
        .insert(TypeAliasFamily::Integer, "integer".into());
    options.type_aliases.insert(
        TypeAliasFamily::CharacterVarying,
        "character varying".into(),
    );

    let formatted = format_sql(source, &options).expect("formatting succeeds");
    assert_eq!(
        formatted.output,
        "SELECT NULL::integer, NULL::character varying(24);"
    );
}

#[test]
fn supports_every_real_type_alias_family_and_preserves_modifiers() {
    let source = "SELECT NULL::int2, NULL::int4, NULL::int8, NULL::bool, NULL::char(4), NULL::character varying(12), NULL::varbit(8), NULL::decimal(10, 2), NULL::float4, NULL::float8, NULL::timetz, NULL::timestamp without time zone, NULL::timestamptz;";
    let mut options = FormatOptions::default();
    for (family, spelling) in [
        (TypeAliasFamily::Smallint, "smallint"),
        (TypeAliasFamily::Integer, "int"),
        (TypeAliasFamily::Bigint, "bigint"),
        (TypeAliasFamily::Boolean, "boolean"),
        (TypeAliasFamily::Character, "character"),
        (TypeAliasFamily::CharacterVarying, "varchar"),
        (TypeAliasFamily::BitVarying, "bit varying"),
        (TypeAliasFamily::Numeric, "numeric"),
        (TypeAliasFamily::Real, "real"),
        (TypeAliasFamily::DoublePrecision, "double precision"),
        (TypeAliasFamily::TimeWithTimeZone, "time with time zone"),
        (TypeAliasFamily::TimestampWithoutTimeZone, "timestamp"),
        (
            TypeAliasFamily::TimestampWithTimeZone,
            "timestamp with time zone",
        ),
    ] {
        options.type_aliases.insert(family, spelling.into());
    }

    let formatted = format_sql(source, &options).expect("formatting succeeds");
    assert_eq!(
        formatted.output,
        "SELECT\n    NULL::smallint,\n    NULL::int,\n    NULL::bigint,\n    NULL::boolean,\n    NULL::character(4),\n    NULL::varchar(12),\n    NULL::bit varying(8),\n    NULL::numeric(10, 2),\n    NULL::real,\n    NULL::double precision,\n    NULL::time WITH time zone,\n    NULL::timestamp,\n    NULL::timestamp WITH time zone;"
    );
    assert!(check_sql(&formatted.output, &options).compliant);
}

#[test]
fn supports_serial_aliases_only_in_parser_owned_column_types() {
    let source = "CREATE TABLE public.sample (a serial2, b serial4, c serial8);";
    let mut options = FormatOptions::default();
    options
        .type_aliases
        .insert(TypeAliasFamily::Smallserial, "smallserial".into());
    options
        .type_aliases
        .insert(TypeAliasFamily::Serial, "serial".into());
    options
        .type_aliases
        .insert(TypeAliasFamily::Bigserial, "bigserial".into());

    let formatted = format_sql(source, &options).expect("formatting succeeds");
    assert_eq!(
        formatted.output,
        "CREATE TABLE public.sample (\n    a smallserial,\n    b serial,\n    c bigserial\n);"
    );
}

#[test]
fn rejects_non_alias_targets_and_preserves_ambiguous_type_names() {
    let mut invalid = FormatOptions::default();
    invalid
        .type_aliases
        .insert(TypeAliasFamily::CharacterVarying, "text".into());
    assert!(format_sql("SELECT NULL::varchar;", &invalid).is_err());

    let mut options = FormatOptions::default();
    options
        .type_aliases
        .insert(TypeAliasFamily::Integer, "int".into());
    options
        .type_aliases
        .insert(TypeAliasFamily::DoublePrecision, "double precision".into());
    let source = "SELECT NULL::public.int4, NULL::\"int4\", NULL::float(20), int4 FROM public.values_source;";
    let formatted = format_sql(source, &options).expect("formatting succeeds");
    assert_eq!(formatted.output, source);
}

#[test]
fn type_alias_diagnostics_point_to_complete_authored_spellings() {
    let source = "SELECT NULL::character varying, NULL::timestamp WITH time zone;";
    let mut options = FormatOptions::default();
    options
        .type_aliases
        .insert(TypeAliasFamily::CharacterVarying, "varchar".into());
    options
        .type_aliases
        .insert(TypeAliasFamily::TimestampWithTimeZone, "timestamptz".into());

    let checked = check_sql(source, &options);
    let spellings = checked
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.rule_id == "type.alias")
        .map(|diagnostic| &source[diagnostic.source_range.start..diagnostic.source_range.end])
        .collect::<Vec<_>>();
    assert_eq!(spellings, ["character varying", "timestamp WITH time zone"]);
}

#[test]
fn diagnostic_ranges_follow_crlf_bytes_and_arrays_are_preserved() {
    let source = "SELECT NULL::integer[], NULL::character varying(8);\r\n";
    let mut options = FormatOptions::default();
    options
        .type_aliases
        .insert(TypeAliasFamily::Integer, "int".into());
    options
        .type_aliases
        .insert(TypeAliasFamily::CharacterVarying, "varchar".into());

    let formatted = format_sql(source, &options).expect("formatting succeeds");
    assert_eq!(formatted.output, "SELECT NULL::int[], NULL::varchar(8);\n");
    for diagnostic in check_sql(source, &options)
        .diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.rule_id == "type.alias")
    {
        assert!(
            matches!(
                &source[diagnostic.source_range.start..diagnostic.source_range.end],
                "integer" | "character varying"
            ),
            "{diagnostic:#?}"
        );
    }
}

#[test]
fn longer_preferences_participate_in_width_layout() {
    let source = "SELECT NULL::int, NULL::int;";
    let mut options = FormatOptions {
        soft_line_width: 30,
        hard_line_width: 35,
        ..FormatOptions::default()
    };
    options
        .type_aliases
        .insert(TypeAliasFamily::Integer, "integer".into());

    let formatted = format_sql(source, &options).expect("formatting succeeds");
    assert!(formatted.output.contains('\n'), "{}", formatted.output);
    assert!(formatted.output.lines().all(|line| line.len() <= 35));
}

#[test]
fn preserves_aliases_inside_unsupported_statements() {
    let source = "SELECT NULL::integer;\nCREATE VIEW sample AS WITH values_source AS (SELECT NULL::integer) SELECT * FROM values_source;";
    let unsupported = "CREATE VIEW sample AS WITH values_source AS (SELECT NULL::integer) SELECT * FROM values_source;";
    let mut options = FormatOptions::default();
    options
        .type_aliases
        .insert(TypeAliasFamily::Integer, "int".into());

    let formatted = format_sql(source, &options).expect("formatting succeeds");
    assert!(formatted.output.starts_with("SELECT NULL::int;\n"));
    assert!(formatted.output.ends_with(unsupported));
}

#[test]
fn normalizes_around_copy_stdin_without_touching_payload() {
    let source = "SELECT NULL::integer;\nCOPY public.items(value) FROM STDIN;\ninteger\n\\.\nSELECT NULL::integer;";
    let mut options = FormatOptions::default();
    options
        .type_aliases
        .insert(TypeAliasFamily::Integer, "int".into());

    let formatted = format_sql(source, &options).expect("formatting succeeds");
    assert_eq!(
        formatted.output,
        "SELECT NULL::int;\nCOPY public.items(value) FROM STDIN;\ninteger\n\\.\nSELECT NULL::int;"
    );
}

#[test]
fn checked_in_example_enables_every_family_and_round_trips() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("examples")
        .join("semblock-all-type-aliases.toml");
    let config = Config::load(Some(&path)).expect("example configuration is valid");

    assert_eq!(config.format.type_aliases.len(), 16);
    let shown = config.to_toml();
    assert!(shown.contains("[format.type_aliases]"));
    assert!(shown.contains("integer = \"int\""));
    assert!(shown.contains("timestamp_with_time_zone = \"timestamptz\""));
}

#[test]
fn configuration_rejects_text_as_a_varchar_alias() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("semblock.toml");
    std::fs::write(
        &path,
        "[format.type_aliases]\ncharacter_varying = \"text\"\n",
    )
    .expect("write config");

    let error = Config::load(Some(&path)).expect_err("text is not a varchar alias");
    assert!(error.to_string().contains("character_varying"), "{error}");
}

#[test]
fn applies_alias_preferences_through_go_host_formatting() {
    let source = "package sample\n\nconst query = `select null::integer;`\n";
    let mut options = FormatOptions::default();
    options
        .type_aliases
        .insert(TypeAliasFamily::Integer, "int".into());

    let formatted = format_source(source, Language::Go, &options, &Config::default().go)
        .expect("Go formatting succeeds");
    assert!(formatted.output.contains("`SELECT NULL::int;`"));
    assert!(formatted.output_diagnostics.is_empty());
}

#[test]
fn applies_alias_preferences_to_routine_headers_and_plpgsql() {
    let source = r#"CREATE FUNCTION public.alias_sample(value integer)
RETURNS integer
LANGUAGE plpgsql
AS $$
DECLARE
local_value integer;
label varchar(5);
BEGIN
RETURN value::integer;
END;
$$;"#;
    let mut options = FormatOptions::default();
    options
        .type_aliases
        .insert(TypeAliasFamily::Integer, "int".into());
    options.type_aliases.insert(
        TypeAliasFamily::CharacterVarying,
        "character varying".into(),
    );

    let formatted = format_sql(source, &options).expect("routine formatting succeeds");
    assert!(formatted.output.contains("(value int)"), "{formatted:#?}");
    assert!(formatted.output.contains("RETURNS int"));
    assert!(formatted.output.contains("local_value int;"));
    assert!(formatted.output.contains("label character varying(5);"));
    assert!(formatted.output.contains("value::int"));
    assert!(
        check_sql(source, &options)
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.rule_id == "type.alias")
    );
    assert!(check_sql(&formatted.output, &options).compliant);
}
