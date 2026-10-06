use semblock::FormatOptions;
use semblock::config::GoConfig;
use semblock::source::{Language, format_source};

#[test]
fn indivisible_warnings_inside_routine_bodies_point_to_inner_tokens() {
    let literal = format!("'{}'", "ж".repeat(200));
    for body in [
        format!(
            "CREATE FUNCTION sample_warning() RETURNS text LANGUAGE SQL AS $body$\r\nselect {literal};\r\n$body$;"
        ),
        format!("DO $body$\r\nBEGIN\r\n    perform {literal};\r\nEND;\r\n$body$;"),
    ] {
        let source = format!("-- retained prefix\r\nSELECT 1;\r\n\r\n{body}\r\n");
        let result = semblock::format_sql(&source, &FormatOptions::default()).unwrap();
        let warnings = result
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.rule_id == "layout.hard_line_width")
            .collect::<Vec<_>>();
        assert!(!warnings.is_empty());
        for warning in warnings {
            let start = source.find(&literal).unwrap();
            assert_eq!(
                warning.source_range,
                semblock::SourceRange::new(start, start + literal.len())
            );
        }
        let repeated = semblock::format_sql(&result.output, &FormatOptions::default()).unwrap();
        let start = result.output.find(&literal).unwrap();
        assert!(
            repeated
                .diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.rule_id == "layout.hard_line_width")
                .all(|diagnostic| diagnostic.source_range
                    == semblock::SourceRange::new(start, start + literal.len()))
        );
    }
}

#[test]
fn indivisible_multiline_comment_warnings_point_to_the_wide_source_line() {
    let line = "x".repeat(200);
    for newline in ["\n", "\r\n"] {
        let source = format!(
            "SELECT 1;{newline}/* retained comment{newline}{line}{newline}*/{newline}SELECT 2;"
        );
        let result = semblock::format_sql(&source, &FormatOptions::default()).unwrap();
        let warning = result
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.rule_id == "layout.hard_line_width")
            .unwrap();
        let start = source.find(&line).unwrap();
        assert_eq!(
            warning.source_range,
            semblock::SourceRange::new(start, start + line.len())
        );
    }
}

#[test]
fn ordinary_dollar_literals_keep_their_literal_warning_range() {
    let literal = format!("$body$SELECT '{}' $body$", "x".repeat(200));
    let source = format!("SELECT {literal};");
    let result = semblock::format_sql(&source, &FormatOptions::default()).unwrap();
    let warning = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.rule_id == "layout.hard_line_width")
        .unwrap();
    let start = source.find(&literal).unwrap();
    assert_eq!(
        warning.source_range,
        semblock::SourceRange::new(start, start + literal.len())
    );
}

#[test]
fn routine_warning_mapping_survives_type_alias_token_changes() {
    let literal = format!("'{}'", "x".repeat(200));
    let source = format!(
        "SELECT 1;\nCREATE FUNCTION sample_warning() RETURNS TABLE(a varchar, b text) LANGUAGE plpgsql AS $$\nBEGIN\nRETURN QUERY SELECT NULL::character varying, {literal};\nEND;\n$$;"
    );
    let mut options = FormatOptions::default();
    options.type_aliases.insert(
        semblock::TypeAliasFamily::CharacterVarying,
        "varchar".into(),
    );
    let result = semblock::format_sql(&source, &options).unwrap();
    let warning = result
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.rule_id == "layout.hard_line_width")
        .unwrap();
    let start = source.find(&literal).unwrap();
    assert_eq!(
        warning.source_range,
        semblock::SourceRange::new(start, start + literal.len())
    );
}

#[test]
fn sql_directive_diagnostics_are_shifted_to_document_offsets() {
    let source = "-- semblock:off\nselect vendor_specific_magic(;\n-- semblock:on\nselect id,title from public.items;\n";
    let active_start = source.rfind("select id").expect("active SQL");
    let formatted = format_source(
        source,
        Language::Sql,
        &FormatOptions::default(),
        &GoConfig::default(),
    )
    .expect("format source");

    assert!(formatted.changed);
    assert!(!formatted.diagnostics.is_empty());
    assert!(formatted.diagnostics.iter().all(|diagnostic| {
        diagnostic.source_range.start >= active_start && diagnostic.source_range.end <= source.len()
    }));
}

#[test]
fn crlf_diagnostic_ranges_map_back_to_original_bytes() {
    let source = "SELECT 1;\r\nselect count(*);\r\n";
    let second_select = source.rfind("select").expect("second SELECT");
    let formatted = format_source(
        source,
        Language::Sql,
        &FormatOptions::default(),
        &GoConfig::default(),
    )
    .expect("format source");

    let diagnostic = formatted
        .diagnostics
        .iter()
        .find(|diagnostic| {
            diagnostic.rule_id == "casing.keyword" && diagnostic.source_range.start == second_select
        })
        .expect("second SELECT diagnostic");
    assert_eq!(
        &source[diagnostic.source_range.start..diagnostic.source_range.end],
        "select"
    );
}

#[test]
fn go_diagnostics_are_attributed_to_the_owning_raw_literal() {
    let source = "package queries\n\nconst query = `select count(*) from public.items;`\n";
    let content_start = source.find("select count").expect("SQL content");
    let content_end = source[content_start..]
        .find('`')
        .map(|offset| content_start + offset)
        .expect("closing backtick");
    let formatted = format_source(
        source,
        Language::Go,
        &FormatOptions::default(),
        &GoConfig::default(),
    )
    .expect("format source");

    assert!(formatted.changed);
    assert!(!formatted.diagnostics.is_empty());
    assert!(formatted.diagnostics.iter().all(|diagnostic| {
        diagnostic.source_range.start == content_start
            && diagnostic.source_range.end == content_end
            && diagnostic.message.starts_with("embedded SQL: ")
    }));
}

#[test]
fn go_output_diagnostics_follow_literals_shifted_by_earlier_formatting() {
    let unsupported = "CREATE TABLE public.new_items (LIKE public.items INCLUDING ALL);";
    let source = format!(
        "package queries\n\nconst formatted = `select id from public.items where active=true and (title is not null or original_title is not null);`\nconst opaque = `{unsupported}`\n"
    );
    let formatted = format_source(
        &source,
        Language::Go,
        &FormatOptions::default(),
        &GoConfig::default(),
    )
    .expect("format Go source");

    let input = formatted
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.rule_id == "syntax.unsupported")
        .expect("input-relative Go diagnostic");
    let output = formatted
        .output_diagnostics
        .iter()
        .find(|diagnostic| diagnostic.rule_id == "syntax.unsupported")
        .expect("output-relative Go diagnostic");
    assert_eq!(
        &source[input.source_range.start..input.source_range.end],
        unsupported
    );
    assert_eq!(
        &formatted.output[output.source_range.start..output.source_range.end],
        unsupported
    );
    assert_ne!(input.source_range, output.source_range);
}

#[test]
fn input_and_output_diagnostics_slice_their_matching_crlf_sources() {
    let literal = "'длинная строка, которая намеренно превышает настроенную жесткую границу и остается неделимой'";
    let unsupported = "CREATE TABLE public.new_items (LIKE public.items INCLUDING ALL);";
    let source = format!(
        "select 'я' as label,id from public.items where active=true and (title is not null or original_title is not null);\r\nSELECT {literal};\r\n{unsupported}\r\n"
    );
    let options = FormatOptions {
        soft_line_width: 64,
        hard_line_width: 80,
        ..FormatOptions::default()
    };

    let formatted = format_source(&source, Language::Sql, &options, &GoConfig::default())
        .expect("format source");

    let input_width = formatted
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.rule_id == "layout.hard_line_width")
        .expect("input-relative width warning");
    let output_width = formatted
        .output_diagnostics
        .iter()
        .find(|diagnostic| diagnostic.rule_id == "layout.hard_line_width")
        .expect("output-relative width warning");
    assert_eq!(
        &source[input_width.source_range.start..input_width.source_range.end],
        literal
    );
    assert_eq!(
        &formatted.output[output_width.source_range.start..output_width.source_range.end],
        literal
    );
    assert_ne!(input_width.source_range, output_width.source_range);

    let input_unsupported = formatted
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.rule_id == "syntax.unsupported")
        .expect("input-relative unsupported warning");
    let output_unsupported = formatted
        .output_diagnostics
        .iter()
        .find(|diagnostic| diagnostic.rule_id == "syntax.unsupported")
        .expect("output-relative unsupported warning");
    assert_eq!(
        &source[input_unsupported.source_range.start..input_unsupported.source_range.end],
        unsupported
    );
    assert_eq!(
        &formatted.output
            [output_unsupported.source_range.start..output_unsupported.source_range.end],
        unsupported
    );

    let repeated = format_source(
        &formatted.output,
        Language::Sql,
        &options,
        &GoConfig::default(),
    )
    .expect("format output again");
    assert_eq!(repeated.output, formatted.output);
    assert_eq!(repeated.diagnostics, formatted.output_diagnostics);
    assert_eq!(repeated.output_diagnostics, formatted.output_diagnostics);
}
