use semblock::config::{GoConfig, RustConfig, RustMultilineStringStyle};
use semblock::source::{Language, format_source, format_source_with_rust};
use semblock::{FormatOptions, UnsupportedPolicy, validate_equivalent};
use std::io::Write;
use std::process::{Command, Stdio};

fn format(source: &str) -> semblock::source::FormattedSource {
    format_source(
        source,
        Language::Rust,
        &FormatOptions::default(),
        &GoConfig::default(),
    )
    .unwrap()
}

fn stdin_fmt(source: &str) -> std::process::Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_semblock"))
        .args(["fmt", "--stdin", "--language", "rust"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

#[test]
fn rust_const_and_inline_strings_use_the_shared_formatter() {
    let source = "const QUERY: &str = r#\"select id from items where id=$1\"#;\nfn main() { query(\"select 1\"); }\n";
    let result = stdin_fmt(source);
    assert!(result.status.success(), "{result:?}");
    let output = String::from_utf8(result.stdout).unwrap();
    assert!(output.contains("SELECT id"), "{output}");
    assert!(output.contains("query(\"SELECT 1\")"), "{output}");
    assert_eq!(stdin_fmt(&output).stdout, output.as_bytes());
}

#[test]
fn publication_const_preserves_query_semantics_and_raw_envelope() {
    let source = include_str!("fixtures/rust/publication.input.rs");
    let result = format(source);
    assert_eq!(
        result.output,
        include_str!("fixtures/rust/publication.expected.rs")
    );
    assert!(
        result
            .output
            .starts_with("const VALIDATE_PUBLICATION_SQL: &str = r#\"\n")
    );
    assert!(result.output.ends_with("\n\"#;\n"));
    let input = source
        .split_once("r#\"")
        .unwrap()
        .1
        .rsplit_once("\"#")
        .unwrap()
        .0;
    let output = result
        .output
        .split_once("r#\"")
        .unwrap()
        .1
        .rsplit_once("\"#")
        .unwrap()
        .0;
    validate_equivalent(input, output).unwrap();
    assert!(!format(&result.output).changed);
}

#[test]
fn expression_contexts_and_rust_2024_syntax_are_supported() {
    let source = r####"
static SQL: &str = r"select 1";
const RAW: &str = r###"select 2"###;
struct Queries { sql: &'static str }
fn query(_: &str) {}
async fn nested() {
    let mut sql = "select 3";
    sql = "select 4";
    query("select 5");
    let fields = Queries { sql: "select 6" };
    let list = ["select 7", "select 8"];
    let pair = ("select 9", "select 10");
    let closure = || "select 11";
    query({ "select 12" });
    if let Some(value) = Some(1) && value > 0 { query("select 13"); }
}
fn result() -> &'static str { return "select 14"; }
fn tail() -> &'static str { "select 15" }
"####;
    let output = format(source).output;
    for number in 1..=15 {
        assert!(output.contains(&format!("SELECT {number}")), "{output}");
    }
    assert!(output.contains("r###\"SELECT 2\"###"));
}

#[test]
fn only_reviewed_sqlx_macro_sql_positions_are_formatted() {
    let source = r##"
fn run() {
    sqlx::query!("select 1", "select 101");
    sqlx::query_as!(Row, r#"select 2"#, "select 102");
    sqlx::query_scalar!("select 3");
    sqlx::query_unchecked!("select 4");
    sqlx::query_as_unchecked!(Row, "select 5");
    sqlx::query_scalar_unchecked!("select 6");
    sqlx::query_as!(Row<A, B>, "select 7");
    sqlx::query!(format!("select 8"));
    sqlx::query!("select 9" + suffix);
    sqlx::query_as!("select 10", "select 11");
    custom!("select 12");
    format!("select 13");
    concat!("select 14", " from items");
    sqlx::query_file!("select 15");
}
"##;
    let output = format(source).output;
    for number in 1..=7 {
        assert!(output.contains(&format!("SELECT {number}")), "{output}");
    }
    for number in 8..=15 {
        assert!(output.contains(&format!("select {number}")), "{output}");
    }
    assert!(output.contains("\"select 101\""));
    assert!(output.contains("\"select 102\""));
}

#[test]
fn non_expression_strings_dynamic_values_and_fragments_are_untouched() {
    let source = r##"
#[doc = "select 1"]
extern "select 2" { fn foreign(); }
fn run() {
    // select 3
    let bytes = b"select 4";
    let raw_bytes = br#"select 5"#;
    let c = c"select 6";
    let raw_c = cr#"select 7"#;
    let dynamic = "select 8".to_owned() + suffix;
    let fragment = "where id=$1";
    let prose = "select your preferred option";
    match input { "select 9" => (), _ => () }
}
macro_rules! sql { () => { "select 10" }; }
"##;
    assert_eq!(format(source).output, source);
}

#[test]
fn escapes_unicode_delimiters_and_preserve_style_round_trip() {
    let source = r##"const SQL: &str = "select\n \'\u{0436}\x41\' as label,\n 1\nfrom items";"##;
    let result = format(source);
    assert!(
        result
            .output
            .contains("r\"SELECT\n    'жA' AS label,\n    1\nFROM items\""),
        "{}",
        result.output
    );
    let config = RustConfig {
        multiline_string_style: RustMultilineStringStyle::Preserve,
        ..RustConfig::default()
    };
    let preserved = format_source_with_rust(
        source,
        Language::Rust,
        &FormatOptions::default(),
        &GoConfig::default(),
        &config,
    )
    .unwrap();
    assert!(
        preserved
            .output
            .contains("= \"SELECT\\n    'жA' AS label,\\n    1\\nFROM items\""),
        "{}",
        preserved.output
    );
    let delimiter = r###"const SQL: &str = r#"select '"# quoted', '\\' from items"#;"###;
    // The embedded close delimiter above is invalid Rust and must fail closed.
    assert!(
        format_source(
            delimiter,
            Language::Rust,
            &FormatOptions::default(),
            &GoConfig::default()
        )
        .is_err()
    );
}

#[test]
fn crlf_and_closing_host_indent_are_preserved() {
    let source = "fn run() {\r\n    let sql = r##\"\r\n        select id\r\n        from items\r\n    \"##;\r\n}\r\n";
    let output = format(source).output;
    assert!(
        output.contains("r##\"\r\nSELECT id FROM items\r\n    \"##"),
        "{output:?}"
    );
    assert!(!output.replace("\r\n", "").contains('\n'));
    let cooked = "fn run() {\r\n    query(\"select\\n a,\\n b\\nfrom items\");\r\n}\r\n";
    let output = format(cooked).output;
    assert!(!output.replace("\r\n", "").contains('\n'), "{output:?}");
}

#[test]
fn directives_force_ignore_and_reject_misplacement() {
    let source = "// semblock:ignore\n// Legacy spelling.\nconst A: &str = \"select 1\";\n// language=SQL\n// Complete SQL value.\nconst B: &str = \"select 2\";\n";
    let output = format(source).output;
    assert!(output.contains("\"select 1\""));
    assert!(output.contains("\"SELECT 2\""));
    let forced = RustConfig {
        auto_detect: false,
        ..RustConfig::default()
    };
    let result = format_source_with_rust(
        source,
        Language::Rust,
        &FormatOptions::default(),
        &GoConfig::default(),
        &forced,
    )
    .unwrap();
    assert_eq!(result.output, output);
    let ignored = "// semblock:file-ignore\nconst A: &str = \"select 1\";\n";
    assert_eq!(format(ignored).output, ignored);
    for source in [
        "// semblock:sql\nconst A: &str = \"select from\";",
        "// semblock:sql\n// semblock:ignore\nconst A: &str = \"select 1\";",
        "// semblock:off\nconst A: &str = \"select 1\";",
        "// semblock:sql\nconst A: i32 = 1;",
        "const A: i32 = 1;\n// semblock:file-ignore\n",
    ] {
        assert!(
            format_source(
                source,
                Language::Rust,
                &FormatOptions::default(),
                &GoConfig::default()
            )
            .is_err(),
            "{source}"
        );
    }
}

#[test]
fn unsupported_sql_is_byte_identical_and_strict_policy_restores_whole_source() {
    let source = r##"const A: &str = "select 1";
const B: &str = r#"select * from xmltable('/rows' passing '<rows/>' columns id int path '@id')"#;
"##;
    let result = format(source);
    assert!(result.output.contains("\"SELECT 1\""));
    assert!(result.output.ends_with(source.split_once('\n').unwrap().1));
    assert!(
        result
            .diagnostics
            .iter()
            .any(|d| d.rule_id == "syntax.unsupported")
    );
    let options = FormatOptions {
        unsupported_policy: UnsupportedPolicy::Error,
        ..FormatOptions::default()
    };
    assert_eq!(
        format_source(source, Language::Rust, &options, &GoConfig::default())
            .unwrap()
            .output,
        source
    );
}

#[test]
fn diagnostics_follow_the_owning_literals_in_each_pass() {
    let source = "const A: &str = \"select\\n a,\\n b\\nfrom items\";\nconst B: &str = \"select * from xmltable('/r' passing '<r/>' columns id int path '@id')\";\n";
    let result = format(source);
    for (text, diagnostics) in [
        (source, &result.diagnostics),
        (result.output.as_str(), &result.output_diagnostics),
    ] {
        let diagnostic = diagnostics
            .iter()
            .find(|d| d.rule_id == "syntax.unsupported")
            .unwrap();
        assert!(
            text[diagnostic.source_range.start..diagnostic.source_range.end]
                .starts_with("\"select * from xmltable")
        );
    }
    assert_ne!(
        result
            .diagnostics
            .iter()
            .find(|d| d.rule_id == "syntax.unsupported")
            .unwrap()
            .source_range,
        result
            .output_diagnostics
            .iter()
            .find(|d| d.rule_id == "syntax.unsupported")
            .unwrap()
            .source_range,
    );
}

#[test]
fn formatted_literals_compile_and_decode_as_the_intended_runtime_sql() {
    let source = r###"const SQL: &str = "select\n '\u{0436}\x41\',\n 'quote \"# and slash \\'\nfrom items";
fn main() { print!("{SQL}"); }
"###;
    let output = format(source).output;
    let literal = output
        .split_once("= ")
        .unwrap()
        .1
        .split_once(";\n")
        .unwrap()
        .0;
    let expected = syn::parse_str::<syn::LitStr>(literal).unwrap().value();
    assert!(literal.starts_with("r##\""), "{literal}");
    let root = tempfile::TempDir::new().unwrap();
    let path = root.path().join("query.rs");
    let executable = root
        .path()
        .join(format!("query{}", std::env::consts::EXE_SUFFIX));
    std::fs::write(&path, &output).unwrap();
    let compile = Command::new("rustc")
        .args(["--edition=2024", "-o"])
        .arg(&executable)
        .arg(&path)
        .output()
        .unwrap();
    assert!(compile.status.success(), "{compile:?}\n{output}");
    let runtime = Command::new(executable).output().unwrap();
    assert!(runtime.status.success());
    assert_eq!(runtime.stdout, expected.as_bytes());
}

#[test]
fn rust_line_continuations_and_control_values_round_trip() {
    let source = "const SQL: &str = \"select \\\n    'x\\t\\u{1b}' as label\";";
    let output = format(source).output;
    let literal = output
        .split_once("= ")
        .unwrap()
        .1
        .strip_suffix(';')
        .unwrap();
    assert_eq!(
        syn::parse_str::<syn::LitStr>(literal).unwrap().value(),
        "SELECT 'x\t\u{1b}' AS label"
    );
    for malformed in [
        r#"const SQL: &str = "select '\q'";"#,
        r#"const SQL: &str = "select '\u{D800}'";"#,
    ] {
        assert!(
            format_source(
                malformed,
                Language::Rust,
                &FormatOptions::default(),
                &GoConfig::default()
            )
            .is_err()
        );
    }
}

#[test]
fn sql_comments_groups_and_multiline_values_remain_attached_and_equivalent() {
    let source = r###"const SQL: &str = r#"
select
    a,
    -- User-visible fields.
    b,

    c,
    'first
        second' as label
from items
"#;
"###;
    let result = format(source);
    assert!(
        result
            .output
            .contains("    -- User-visible fields.\n    b,\n\n    c,"),
        "{}",
        result.output
    );
    assert!(result.output.contains("'first\n        second' AS label"));
    let input = source
        .split_once("r#\"")
        .unwrap()
        .1
        .rsplit_once("\"#")
        .unwrap()
        .0;
    let output = result
        .output
        .split_once("r#\"")
        .unwrap()
        .1
        .rsplit_once("\"#")
        .unwrap()
        .0;
    validate_equivalent(input, output).unwrap();
    assert!(!format(&result.output).changed);
}

#[test]
fn sqlx_type_arguments_and_directives_allow_adjacent_comments() {
    let source = r##"fn run() {
    // semblock:sql
    sqlx::query_as!(Row<A, B>,
        // language=SQL
        "select 1",
        parameter,
    );
}"##;
    let output = format(source).output;
    assert_eq!(output, source.replace("\"select 1\"", "\"SELECT 1\""));
}

#[test]
fn string_method_receivers_are_opaque_but_chained_query_arguments_format() {
    let source = r##"fn run() {
    let dynamic = "select 1".replace("1", runtime);
    let query = sqlx::query("select 2").bind(parameter);
}"##;
    let output = format(source).output;
    assert_eq!(output, source.replace("\"select 2\"", "\"SELECT 2\""));
}

#[test]
fn sql_root_indent_is_normalized_and_authored_boundary_newlines_are_kept() {
    let source = "const SQL: &str = r#\"\n    SELECT 1\n\"#;\n";
    assert_eq!(
        format(source).output,
        "const SQL: &str = r#\"\nSELECT 1\n\"#;\n"
    );
    let trailing = "const SQL: &str = \"select 1\\n\";\n";
    let output = format(trailing).output;
    let literal = output
        .split_once("= ")
        .unwrap()
        .1
        .strip_suffix(";\n")
        .unwrap();
    assert_eq!(
        syn::parse_str::<syn::LitStr>(literal).unwrap().value(),
        "SELECT 1\n"
    );
}
