use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use tempfile::TempDir;

fn semblock() -> Command {
    Command::new(env!("CARGO_BIN_EXE_semblock"))
}

fn run(root: &Path, args: &[&str], stdin: Option<&str>) -> Output {
    let mut command = semblock();
    command.current_dir(root).args(args);
    if let Some(stdin) = stdin {
        command.stdin(Stdio::piped());
        let mut child = command
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn semblock");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(stdin.as_bytes())
            .expect("write stdin");
        child.wait_with_output().expect("wait for semblock")
    } else {
        command.output().expect("run semblock")
    }
}

fn write(root: &Path, path: &str, contents: &str) {
    let path = root.join(path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create fixture parent");
    }
    fs::write(path, contents).expect("write fixture");
}

#[test]
fn check_reports_changes_without_writing() {
    let project = TempDir::new().expect("temp project");
    let source = "select id,title from public.items where deleted_at is null;\n";
    write(project.path(), "query.sql", source);

    let output = run(project.path(), &["check", "query.sql"], None);

    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("query.sql:"), "{stderr}");
    assert!(stderr.contains("error[casing.keyword]"), "{stderr}");
    assert!(stderr.contains("error[spacing.comma]"), "{stderr}");
    assert_eq!(
        fs::read_to_string(project.path().join("query.sql")).expect("read query"),
        source
    );
}

#[test]
fn fmt_discovers_sql_and_go_while_respecting_both_ignore_files() {
    let project = TempDir::new().expect("temp project");
    write(project.path(), ".gitignore", "git-ignored.sql\n");
    write(project.path(), ".semblockignore", "semblock-ignored.sql\n");
    write(
        project.path(),
        "query.sql",
        "select id,title from public.items where deleted_at is null;\n",
    );
    write(
        project.path(),
        "git-ignored.sql",
        "select ignored from legacy;\n",
    );
    write(
        project.path(),
        "semblock-ignored.sql",
        "select ignored_too from legacy;\n",
    );
    write(
        project.path(),
        "queries.go",
        "package queries\n\nconst query = `\n    select id,title from public.items where deleted_at is null;\n`\n",
    );

    let output = run(project.path(), &["fmt", ".", "--jobs", "2"], None);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        fs::read_to_string(project.path().join("query.sql")).expect("read query"),
        "SELECT id, title FROM public.items WHERE deleted_at IS NULL;\n"
    );
    assert_eq!(
        fs::read_to_string(project.path().join("git-ignored.sql")).expect("read ignored"),
        "select ignored from legacy;\n"
    );
    assert_eq!(
        fs::read_to_string(project.path().join("semblock-ignored.sql"))
            .expect("read semblock ignored"),
        "select ignored_too from legacy;\n"
    );
    let go = fs::read_to_string(project.path().join("queries.go")).expect("read Go");
    assert!(go.contains("`\nSELECT id, title FROM public.items WHERE deleted_at IS NULL;\n`"));
}

#[test]
fn explicit_file_is_processed_even_when_an_ignore_rule_matches_it() {
    let project = TempDir::new().expect("temp project");
    write(project.path(), ".gitignore", "query.sql\n");
    write(
        project.path(),
        "query.sql",
        "select id from public.items;\n",
    );

    let output = run(project.path(), &["fmt", "query.sql"], None);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        fs::read_to_string(project.path().join("query.sql")).expect("read query"),
        "SELECT id FROM public.items;\n"
    );
}

#[test]
fn diff_prints_a_unified_diff_and_never_writes() {
    let project = TempDir::new().expect("temp project");
    let source = "select id from public.items;\n";
    write(project.path(), "query.sql", source);

    let output = run(project.path(), &["diff", "query.sql"], None);

    assert_eq!(output.status.code(), Some(1));
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--- a/query.sql"));
    assert!(stdout.contains("+++ b/query.sql"));
    assert!(stdout.contains("-select id from public.items;"));
    assert!(stdout.contains("+SELECT id"));
    assert_eq!(
        fs::read_to_string(project.path().join("query.sql")).expect("read query"),
        source
    );
}

#[test]
fn stdin_uses_the_filename_to_detect_sql() {
    let project = TempDir::new().expect("temp project");

    let output = run(
        project.path(),
        &["fmt", "--stdin", "--filename", "query.sql"],
        Some("select id from public.items;\n"),
    );

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "SELECT id FROM public.items;\n"
    );
}

#[test]
fn go_cst_supports_auto_detection_and_declaration_directives() {
    let project = TempDir::new().expect("temp project");
    write(
        project.path(),
        "queries.go",
        r#"package queries

const automatic = `
    select id,title from public.items;
`

// semblock:ignore
const ignored = `
    select id,title from legacy.items;
`

// semblock:sql
var explicit = `
    /* injected */ select id,title from public.items;
`

// language=SQL
var jetbrains = `
    select id,title from public.items;
`

func assign() {
    query := `
        select id,title from public.items;
    `
    query = `
        select id,title from public.items;
    `
    _ = query
}

const notSQL = `hello`
const fragment = `WHERE deleted_at IS NULL`
"#,
    );

    let output = run(
        project.path(),
        &["fmt", "--language", "go", "queries.go"],
        None,
    );

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let go = fs::read_to_string(project.path().join("queries.go")).expect("read Go");
    assert!(go.contains("`\nSELECT id, title FROM public.items;\n`"));
    assert!(go.contains("    select id,title from legacy.items;"));
    assert!(
        go.contains("`\n/* injected */ SELECT id, title"),
        "formatted Go:\n{go}"
    );
    assert!(go.contains("const notSQL = `hello`"));
    assert!(go.contains("const fragment = `WHERE deleted_at IS NULL`"));
}

#[test]
fn malformed_auto_detected_go_candidate_is_skipped_while_valid_sql_formats() {
    let project = TempDir::new().expect("temp project");
    let source = r#"package queries

const valid = `select id,title from public.items;`
const invalid = `select from;`
"#;
    write(project.path(), "queries.go", source);

    let output = run(project.path(), &["fmt", "queries.go"], None);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let formatted = fs::read_to_string(project.path().join("queries.go")).expect("read Go");
    assert!(formatted.contains("const valid = `SELECT id, title FROM public.items;`"));
    assert!(formatted.contains("const invalid = `select from;`"));
}

#[test]
fn file_and_block_directives_are_honored_and_invalid_nesting_is_diagnostic() {
    let project = TempDir::new().expect("temp project");
    write(
        project.path(),
        "blocks.sql",
        "select id from public.items;\n-- semblock:off\nselect vendor_specific_magic(;\n-- semblock:on\nselect title from public.items;\n",
    );
    write(
        project.path(),
        "ignored.sql",
        "-- semblock:file-ignore\nselect from;\n",
    );
    let invalid = "-- semblock:off\n-- semblock:off\nselect 1;\n-- semblock:on\n";
    write(project.path(), "invalid.sql", invalid);

    let output = run(project.path(), &["fmt", "blocks.sql", "ignored.sql"], None);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let blocks = fs::read_to_string(project.path().join("blocks.sql")).expect("read blocks");
    assert!(blocks.starts_with("SELECT id FROM public.items;\n"));
    assert!(blocks.contains("select vendor_specific_magic(;\n"));
    assert!(blocks.ends_with("SELECT title FROM public.items;\n"));
    assert_eq!(
        fs::read_to_string(project.path().join("ignored.sql")).expect("read ignored"),
        "-- semblock:file-ignore\nselect from;\n"
    );

    let output = run(project.path(), &["fmt", "invalid.sql"], None);
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("nested semblock:off"));
    assert_eq!(
        fs::read_to_string(project.path().join("invalid.sql")).expect("read invalid"),
        invalid
    );
}

#[test]
fn config_is_strict_and_controls_layout() {
    let project = TempDir::new().expect("temp project");
    write(
        project.path(),
        "semblock.toml",
        r#"dialect = "postgresql"

[layout]
soft_line_width = 40
hard_line_width = 60
"#,
    );
    write(
        project.path(),
        "query.sql",
        "select item.id from public.items item where item.deleted_at is null and (item.title_rus is not null or item.title_orig is not null);\n",
    );

    let output = run(project.path(), &["fmt", "query.sql"], None);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    let sql = fs::read_to_string(project.path().join("query.sql")).expect("read query");
    assert!(sql.contains("\n    item.deleted_at IS NULL"));
    assert!(sql.contains("\n        item.title_rus IS NOT NULL"));

    for (key, value) in [
        ("indent_width", "2"),
        ("preserve_list_groups", "false"),
        ("preserve_blank_lines", "false"),
    ] {
        let filename = format!("obsolete-{key}.toml");
        write(
            project.path(),
            &filename,
            &format!("[layout]\n{key} = {value}\n"),
        );
        let output = run(
            project.path(),
            &["check", "--config", &filename, "query.sql"],
            None,
        );
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(&format!("unknown field `{key}`")),
            "{output:?}"
        );
    }

    write(project.path(), "bad.toml", "unknown = true\n");
    let output = run(
        project.path(),
        &["check", "--config", "bad.toml", "query.sql"],
        None,
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
}

#[test]
fn config_controls_core_lexical_policies() {
    let project = TempDir::new().expect("temp project");
    write(
        project.path(),
        "semblock.toml",
        r#"dialect = "postgresql"

[format]
semicolon_policy = "omit"
not_equal_policy = "prefer_bang"
syntax_diagnostics = "parser_available"
"#,
    );
    write(
        project.path(),
        "query.sql",
        "select count(*) from public.items where status <> 'deleted';\n",
    );

    let output = run(project.path(), &["fmt", "query.sql"], None);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        fs::read_to_string(project.path().join("query.sql")).expect("read query"),
        "SELECT COUNT(*) FROM public.items WHERE status != 'deleted'\n"
    );

    write(
        project.path(),
        "bad-policy.toml",
        r#"[format]
not_equal_policy = "always_rewrite"
"#,
    );
    let output = run(
        project.path(),
        &["check", "--config", "bad-policy.toml", "query.sql"],
        None,
    );
    assert_eq!(output.status.code(), Some(2), "{output:?}");
}

#[cfg(unix)]
#[test]
fn atomic_fmt_preserves_file_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let project = TempDir::new().expect("temp project");
    write(
        project.path(),
        "query.sql",
        "select id from public.items;\n",
    );
    let path = project.path().join("query.sql");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).expect("set mode");

    let output = run(project.path(), &["fmt", "query.sql"], None);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        fs::metadata(path).expect("metadata").permissions().mode() & 0o777,
        0o640
    );
}

#[test]
fn go_file_ignore_and_misplaced_or_unsupported_directives_are_safe() {
    let project = TempDir::new().expect("temp project");
    let ignored = r#"// semblock:file-ignore
package queries

const invalid = `select from;`
"#;
    write(project.path(), "ignored.go", ignored);

    let output = run(project.path(), &["fmt", "ignored.go"], None);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        fs::read_to_string(project.path().join("ignored.go")).expect("read ignored Go"),
        ignored
    );

    let misplaced = r#"package queries

// semblock:sql
func query() {}
"#;
    write(project.path(), "misplaced.go", misplaced);
    let output = run(project.path(), &["fmt", "misplaced.go"], None);
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("misplaced Go directive"));
    assert_eq!(
        fs::read_to_string(project.path().join("misplaced.go")).expect("read misplaced Go"),
        misplaced
    );

    let interpreted = r#"package queries

// language=SQL
const query = "select id from public.items;"
"#;
    write(project.path(), "interpreted.go", interpreted);
    let output = run(project.path(), &["fmt", "interpreted.go"], None);
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        fs::read_to_string(project.path().join("interpreted.go")).expect("read interpreted Go"),
        r#"package queries

// language=SQL
const query = "SELECT id FROM public.items;"
"#
    );
}

#[test]
fn nested_semblockignore_and_crlf_are_preserved() {
    let project = TempDir::new().expect("temp project");
    write(project.path(), "nested/.semblockignore", "ignored.sql\n");
    write(
        project.path(),
        "nested/ignored.sql",
        "select ignored from public.items;\n",
    );
    fs::write(
        project.path().join("nested/query.sql"),
        b"select id,title from public.items;\r\n",
    )
    .expect("write CRLF query");

    let output = run(project.path(), &["fmt", "nested"], None);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        fs::read_to_string(project.path().join("nested/ignored.sql")).expect("read ignored"),
        "select ignored from public.items;\n"
    );
    assert_eq!(
        fs::read(project.path().join("nested/query.sql")).expect("read CRLF query"),
        b"SELECT id, title FROM public.items;\r\n"
    );
}

#[test]
fn semblockignore_has_precedence_over_gitignore_and_hidden_paths_stay_hidden() {
    let project = TempDir::new().expect("temp project");
    write(project.path(), ".gitignore", "query.sql\n");
    write(project.path(), ".semblockignore", "!query.sql\n");
    write(
        project.path(),
        "query.sql",
        "select id from public.items;\n",
    );
    write(
        project.path(),
        ".hidden/query.sql",
        "select hidden from public.items;\n",
    );

    let output = run(project.path(), &["fmt", "."], None);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert_eq!(
        fs::read_to_string(project.path().join("query.sql")).expect("read query"),
        "SELECT id FROM public.items;\n"
    );
    assert_eq!(
        fs::read_to_string(project.path().join(".hidden/query.sql")).expect("read hidden"),
        "select hidden from public.items;\n"
    );
}

#[test]
fn invalid_cli_language_and_sql_fail_with_stable_exit_classes() {
    let project = TempDir::new().expect("temp project");
    write(
        project.path(),
        "query.txt",
        "select id from public.items;\n",
    );
    write(project.path(), "invalid.sql", "select from;\n");

    let output = run(project.path(), &["check", "query.txt"], None);
    assert_eq!(output.status.code(), Some(2), "{output:?}");

    let output = run(project.path(), &["check", "invalid.sql"], None);
    assert_eq!(output.status.code(), Some(3), "{output:?}");

    let output = run(project.path(), &["check", "--jobs", "0", "."], None);
    assert_eq!(output.status.code(), Some(2), "{output:?}");

    let output = run(project.path(), &["check", "missing.sql"], None);
    assert_eq!(output.status.code(), Some(4), "{output:?}");
}

#[test]
fn a_parse_error_prevents_every_planned_project_write() {
    let project = TempDir::new().expect("temp project");
    let valid = "select id from public.items;\n";
    let invalid = "select from;\n";
    write(project.path(), "a-valid.sql", valid);
    write(project.path(), "z-invalid.sql", invalid);

    let output = run(project.path(), &["fmt", "."], None);

    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert_eq!(
        fs::read_to_string(project.path().join("a-valid.sql")).expect("read valid"),
        valid
    );
    assert_eq!(
        fs::read_to_string(project.path().join("z-invalid.sql")).expect("read invalid"),
        invalid
    );
}

#[test]
fn malformed_go_and_unmatched_sql_directives_never_rewrite() {
    let project = TempDir::new().expect("temp project");
    let malformed_go = "package queries\n\nfunc broken( {\n";
    write(project.path(), "broken.go", malformed_go);
    let output = run(project.path(), &["fmt", "broken.go"], None);
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert_eq!(
        fs::read_to_string(project.path().join("broken.go")).expect("read broken Go"),
        malformed_go
    );

    for (name, source, expected) in [
        (
            "unmatched-off.sql",
            "-- semblock:off\nselect 1;\n",
            "unmatched semblock:off",
        ),
        (
            "unmatched-on.sql",
            "-- semblock:on\nselect 1;\n",
            "unmatched semblock:on",
        ),
    ] {
        write(project.path(), name, source);
        let output = run(project.path(), &["fmt", name], None);
        assert_eq!(output.status.code(), Some(3), "{output:?}");
        assert!(String::from_utf8_lossy(&output.stderr).contains(expected));
        assert_eq!(
            fs::read_to_string(project.path().join(name)).expect("read directive fixture"),
            source
        );
    }
}

#[test]
fn unsupported_statements_are_skipped_by_default_and_strict_mode_is_atomic() {
    let project = TempDir::new().expect("temp project");
    let valid = "select id from public.items;\n";
    let formatted_valid = "SELECT id FROM public.items;\n";
    let unsupported = "create table public.new_items (like public.items including all);\n";
    write(project.path(), "a-valid.sql", valid);
    write(project.path(), "z-unsupported.sql", unsupported);

    let output = run(project.path(), &["fmt", "."], None);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("warning[syntax.unsupported]"));
    assert_eq!(
        fs::read_to_string(project.path().join("a-valid.sql")).expect("read valid"),
        formatted_valid
    );
    assert_eq!(
        fs::read_to_string(project.path().join("z-unsupported.sql")).expect("read unsupported"),
        unsupported
    );

    write(project.path(), "a-valid.sql", valid);
    let strict = run(project.path(), &["--strict-unsupported", "fmt", "."], None);
    assert_eq!(strict.status.code(), Some(3), "{strict:?}");
    assert!(String::from_utf8_lossy(&strict.stderr).contains("error[syntax.unsupported]"));
    assert_eq!(
        fs::read_to_string(project.path().join("a-valid.sql")).expect("read strict valid"),
        valid
    );
    assert_eq!(
        fs::read_to_string(project.path().join("z-unsupported.sql"))
            .expect("read strict unsupported"),
        unsupported
    );
}
