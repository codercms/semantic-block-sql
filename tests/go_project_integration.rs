use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

fn fixture_root(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("create fixture destination");
    for entry in fs::read_dir(source).expect("read fixture directory") {
        let entry = entry.expect("read fixture entry");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if entry.file_type().expect("read fixture file type").is_dir() {
            copy_tree(&source_path, &destination_path);
        } else {
            fs::copy(&source_path, &destination_path).expect("copy fixture file");
        }
    }
}

fn fixture_copy(name: &str) -> TempDir {
    let project = TempDir::new().expect("create temporary Go project");
    copy_tree(&fixture_root(name), project.path());
    project
}

fn project_copy() -> TempDir {
    fixture_copy("go-project")
}

fn semblock(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_semblock"))
        .current_dir(root)
        .args(args)
        .output()
        .expect("run semblock")
}

fn tool(root: &Path, program: &str, args: &[&str]) -> Output {
    Command::new(program)
        .current_dir(root)
        .args(args)
        .output()
        .unwrap_or_else(|error| panic!("run {program}: {error}"))
}

fn normalized_lines(output: &[u8]) -> Vec<String> {
    String::from_utf8_lossy(output)
        .lines()
        .map(|line| line.trim_start_matches(".\\").replace('\\', "/"))
        .collect()
}

fn collect_tree(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn visit(root: &Path, current: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(current).expect("read project directory") {
            let entry = entry.expect("read project entry");
            let path = entry.path();
            if entry.file_type().expect("read project file type").is_dir() {
                visit(root, &path, files);
            } else {
                let relative = path.strip_prefix(root).expect("project-relative path");
                files.insert(
                    relative.to_path_buf(),
                    fs::read(path).expect("read project file"),
                );
            }
        }
    }

    let mut files = BTreeMap::new();
    visit(root, root, &mut files);
    files
}

fn go_files(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    collect_tree(root)
        .into_iter()
        .filter(|(path, _)| path.extension().is_some_and(|extension| extension == "go"))
        .collect()
}

fn assert_matches_adjacent_goldens(root: &Path) {
    for (relative, actual) in go_files(root) {
        let expected_relative = PathBuf::from(format!("{}.expected", relative.display()));
        let expected = fs::read(root.join(&expected_relative)).unwrap_or_else(|error| {
            panic!(
                "read golden {} for {}: {error}",
                expected_relative.display(),
                relative.display()
            )
        });
        assert_eq!(
            actual,
            expected,
            "formatted Go file does not match {}",
            expected_relative.display()
        );
    }
}

#[test]
fn formats_a_realistic_go_project_deterministically_and_compiles_it() {
    let serial = project_copy();
    let parallel = project_copy();

    let check = semblock(
        serial.path(),
        &["check", ".", "--list-different", "--jobs", "4"],
    );
    assert_eq!(check.status.code(), Some(1), "{check:?}");
    assert_eq!(
        normalized_lines(&check.stdout),
        [
            "internal/migrations/schema.go",
            "internal/orders/repository.go",
            "internal/users/repository.go",
        ]
    );

    let serial_format = semblock(serial.path(), &["fmt", ".", "--jobs", "1"]);
    assert!(serial_format.status.success(), "{serial_format:?}");
    let parallel_format = semblock(parallel.path(), &["fmt", ".", "--jobs", "4"]);
    assert!(parallel_format.status.success(), "{parallel_format:?}");

    assert_eq!(
        collect_tree(serial.path()),
        collect_tree(parallel.path()),
        "worker count must not affect formatted project bytes"
    );
    assert_matches_adjacent_goldens(serial.path());

    let clean_check = semblock(serial.path(), &["check", ".", "--jobs", "4"]);
    assert!(clean_check.status.success(), "{clean_check:?}");

    let before_second_format = go_files(serial.path());
    let second_format = semblock(serial.path(), &["fmt", ".", "--jobs", "4"]);
    assert!(second_format.status.success(), "{second_format:?}");
    assert_eq!(
        go_files(serial.path()),
        before_second_format,
        "second format must be byte-idempotent"
    );

    let gofmt = tool(serial.path(), "gofmt", &["-l", "."]);
    assert!(gofmt.status.success(), "{gofmt:?}");
    assert!(
        gofmt.stdout.is_empty(),
        "gofmt reported changed files:\n{}",
        String::from_utf8_lossy(&gofmt.stdout)
    );

    let go_test = tool(serial.path(), "go", &["test", "./..."]);
    assert!(go_test.status.success(), "{go_test:?}");
}

#[test]
fn preserves_crlf_in_multiline_go_raw_sql_envelopes() {
    let project = TempDir::new().expect("create temporary Go project");
    let source = b"package queries\r\n\r\nfunc load() {\r\n\tconst query = `\r\n        select id,title from public.items;\r\n\t`\r\n\t_ = query\r\n}\r\n";
    fs::write(project.path().join("queries.go"), source).expect("write CRLF Go source");

    let output = semblock(project.path(), &["fmt", "queries.go"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fs::read(project.path().join("queries.go")).expect("read formatted Go source"),
        b"package queries\r\n\r\nfunc load() {\r\n\tconst query = `\r\nSELECT id, title FROM public.items;\r\n\t`\r\n\t_ = query\r\n}\r\n"
    );
}

#[test]
fn explicit_dynamic_fragments_warn_by_default_and_fail_atomically_in_strict_mode() {
    let project = TempDir::new().expect("create temporary Go project");
    let valid = "package fixture\n\nconst query = `select id,name from public.users;`\n";
    let formatted_valid = "package fixture\n\nconst query = `SELECT id, name FROM public.users;`\n";
    let fragment = "package fixture\n\nconst columns = \"id,name\"\n\n// semblock:sql\nconst query = `SELECT ` + columns + ` FROM public.users`\n";
    fs::write(project.path().join("valid.go"), valid).expect("write valid Go source");
    fs::write(project.path().join("fragment.go"), fragment).expect("write concatenated Go source");

    let output = semblock(project.path(), &["fmt", "."]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("warning[syntax.unsupported]"),
        "{output:?}"
    );
    assert_eq!(
        fs::read_to_string(project.path().join("valid.go")).expect("read valid Go"),
        formatted_valid
    );
    assert_eq!(
        fs::read_to_string(project.path().join("fragment.go")).expect("read fragment Go"),
        fragment
    );

    fs::write(project.path().join("valid.go"), valid).expect("restore valid Go source");
    let before = collect_tree(project.path());
    let strict = semblock(project.path(), &["--strict-unsupported", "fmt", "."]);
    assert_eq!(strict.status.code(), Some(3), "{strict:?}");
    assert!(
        String::from_utf8_lossy(&strict.stderr).contains("error[syntax.unsupported]"),
        "{strict:?}"
    );
    assert_eq!(collect_tree(project.path()), before);
}

#[test]
fn directives_attach_to_direct_return_and_expression_statement_owners() {
    let project = TempDir::new().expect("create temporary Go project");
    fs::write(
        project.path().join("queries.go"),
        r#"package fixture

func identity(query string) string { return query }
func consume(query string) {}

func returned() string {
	// semblock:sql
	return identity(`
        /* injected */ select id,name from public.users;
	`)
}

func executed() {
	// language=SQL
	consume(`
        /* injected */ update public.users set active=false where id=$1;
	`)
}
"#,
    )
    .expect("write direct-owner Go source");

    let output = semblock(project.path(), &["fmt", "queries.go"]);

    assert!(output.status.success(), "{output:?}");
    let formatted = fs::read_to_string(project.path().join("queries.go"))
        .expect("read formatted direct-owner Go source");
    assert!(
        formatted.contains("`\n/* injected */ SELECT id, name FROM public.users;\n\t`"),
        "formatted Go:\n{formatted}"
    );
    assert!(
        formatted.contains(
            "`\n/* injected */ UPDATE public.users SET active = FALSE WHERE id = $1;\n\t`"
        ),
        "formatted Go:\n{formatted}"
    );
}

#[test]
fn permanent_failure_projects_preserve_every_file() {
    for (fixture, expected_error) in [
        ("go-project-invalid-sql", "PostgreSQL parse failed"),
        ("go-project-invalid-go", "Go parse failed"),
        ("go-project-directive-error", "misplaced Go directive"),
    ] {
        let project = fixture_copy(fixture);
        let before = collect_tree(project.path());

        let output = semblock(project.path(), &["fmt", ".", "--jobs", "4"]);

        assert_eq!(
            output.status.code(),
            Some(3),
            "fixture {fixture}: {output:?}"
        );
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(expected_error),
            "fixture {fixture}: {output:?}"
        );
        assert_eq!(
            collect_tree(project.path()),
            before,
            "fixture {fixture}: preflight failure must prevent every project write"
        );
    }
}

#[test]
fn unsupported_plpgsql_is_skipped_by_default_and_strict_mode_is_atomic() {
    let project = TempDir::new().expect("create temporary Go project");
    let valid = "package fixture\n\nconst query = `select id,name from public.users;`\n";
    let formatted_valid = "package fixture\n\nconst query = `SELECT id, name FROM public.users;`\n";
    let routine = "package fixture\n\nconst routine = `\nCREATE PROCEDURE p() LANGUAGE plpgsql AS $$ BEGIN perform 1; COMMIT; END; $$;\n`\n";
    let formatted_routine = "package fixture\n\nconst routine = `\nCREATE PROCEDURE p() LANGUAGE plpgsql AS $$\nBEGIN\n    PERFORM 1;\n    COMMIT;\nEND;\n$$;\n`\n";
    fs::write(project.path().join("valid.go"), valid).expect("write valid Go source");
    fs::write(project.path().join("routine.go"), routine)
        .expect("write unsupported routine source");

    let output = semblock(project.path(), &["fmt", ".", "--jobs", "4"]);

    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("warning[syntax.unsupported]"),
        "{output:?}"
    );
    assert_eq!(
        fs::read_to_string(project.path().join("valid.go")).expect("read formatted valid"),
        formatted_valid
    );
    assert_eq!(
        fs::read_to_string(project.path().join("routine.go")).expect("read formatted routine"),
        formatted_routine
    );

    fs::write(project.path().join("valid.go"), valid).expect("restore valid Go source");
    fs::write(project.path().join("routine.go"), routine).expect("restore routine source");
    let before = collect_tree(project.path());
    let strict = semblock(
        project.path(),
        &["--strict-unsupported", "fmt", ".", "--jobs", "4"],
    );
    assert_eq!(strict.status.code(), Some(3), "{strict:?}");
    assert!(
        String::from_utf8_lossy(&strict.stderr).contains("error[syntax.unsupported]"),
        "{strict:?}"
    );
    assert_eq!(collect_tree(project.path()), before);
}

#[test]
fn preserves_crlf_and_custom_dollar_tags_in_plpgsql_files() {
    let project = TempDir::new().expect("create temporary SQL project");
    let source = b"DO $custom$\r\ndeclare\r\n    item_id bigint:=1;\r\nbegin\r\n    perform refresh_item(item_id);\r\nend;\r\n$custom$;\r\n";
    fs::write(project.path().join("routine.sql"), source).expect("write CRLF routine");

    let output = semblock(project.path(), &["fmt", "routine.sql"]);

    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fs::read(project.path().join("routine.sql")).expect("read formatted routine"),
        b"DO $custom$\r\nDECLARE\r\n    item_id bigint := 1;\r\nBEGIN\r\n    PERFORM refresh_item(item_id);\r\nEND;\r\n$custom$;\r\n"
    );
}
