use semblock::FormatOptions;
use semblock::config::GoConfig;
use semblock::source::{Language, format_source};

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
