use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use semblock::config::Config;
use tempfile::TempDir;

fn run(root: &Path, args: &[&str], input: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_semblock"));
    command.current_dir(root).args(args);
    if let Some(input) = input {
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    } else {
        command.output().unwrap()
    }
}

fn git(root: &Path, args: &[&str]) -> Output {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    output
}

#[test]
fn discovery_check_diff_fmt_and_parallelism_include_rs() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("queries.rs"),
        "const SQL: &str = \"select 1\";\n",
    )
    .unwrap();
    fs::write(root.path().join("plain.sql"), "select 2;\n").unwrap();
    fs::write(root.path().join(".semblockignore"), "ignored.rs\n").unwrap();
    fs::write(
        root.path().join("ignored.rs"),
        "const SQL: &str = \"select 3\";\n",
    )
    .unwrap();
    assert_eq!(
        run(root.path(), &["check", "."], None).status.code(),
        Some(1)
    );
    let diff = run(root.path(), &["diff", "--language", "rust", "."], None);
    assert_eq!(diff.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&diff.stdout).contains("+const SQL: &str = \"SELECT 1\";"));
    assert_eq!(
        fs::read_to_string(root.path().join("queries.rs")).unwrap(),
        "const SQL: &str = \"select 1\";\n"
    );
    let formatted = run(root.path(), &["fmt", "--jobs", "4", "."], None);
    assert!(formatted.status.success(), "{formatted:?}");
    assert_eq!(
        fs::read_to_string(root.path().join("queries.rs")).unwrap(),
        "const SQL: &str = \"SELECT 1\";\n"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("plain.sql")).unwrap(),
        "SELECT 2;\n"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("ignored.rs")).unwrap(),
        "const SQL: &str = \"select 3\";\n"
    );
    assert!(run(root.path(), &["check", "."], None).status.success());
    assert!(
        run(root.path(), &["fmt", "--jobs", "1", "."], None)
            .status
            .success()
    );
    assert!(run(root.path(), &["check", "."], None).status.success());
}

#[test]
fn stdin_filename_and_explicit_language_support_rust() {
    let root = TempDir::new().unwrap();
    let source = "const SQL: &str = \"select 1\";\n";
    let inferred = run(
        root.path(),
        &["fmt", "--stdin", "--filename", "queries.RS"],
        Some(source),
    );
    let explicit = run(
        root.path(),
        &["fmt", "--stdin", "--language", "rust"],
        Some(source),
    );
    assert!(inferred.status.success(), "{inferred:?}");
    assert!(explicit.status.success(), "{explicit:?}");
    assert_eq!(inferred.stdout, b"const SQL: &str = \"SELECT 1\";\n");
    assert_eq!(inferred.stdout, explicit.stdout);
}

#[test]
fn config_is_strict_round_trips_and_controls_rust_discovery() {
    let root = TempDir::new().unwrap();
    let path = root.path().join("semblock.toml");
    let config = Config::default();
    fs::write(&path, config.to_toml()).unwrap();
    assert_eq!(Config::load(Some(&path)).unwrap(), config);
    fs::write(&path, "[rust]\nenabled = false\n").unwrap();
    let source = "const SQL: &str = \"select 1\";\n";
    fs::write(root.path().join("queries.rs"), source).unwrap();
    fs::write(root.path().join("plain.sql"), "select 2;\n").unwrap();
    assert!(run(root.path(), &["fmt", "."], None).status.success());
    assert_eq!(
        fs::read_to_string(root.path().join("queries.rs")).unwrap(),
        source
    );
    assert_eq!(
        run(root.path(), &["fmt", "queries.rs"], None).status.code(),
        Some(3)
    );
    for invalid in [
        "[rust]\nunknown = true\n",
        "[rust]\nenabled = \"yes\"\n",
        "[rust]\nmultiline_string_style = \"other\"\n",
    ] {
        fs::write(&path, invalid).unwrap();
        assert_eq!(
            run(root.path(), &["check", "."], None).status.code(),
            Some(2)
        );
    }
}

#[test]
fn markers_and_string_kind_configuration_are_honored() {
    let root = TempDir::new().unwrap();
    let path = root.path().join("semblock.toml");
    fs::write(&path, "[rust]\nauto_detect = false\n").unwrap();
    let source =
        "const A: &str = \"select 1\";\n// semblock:sql\nconst B: &str = r#\"select 2\"#;\n";
    let output = run(
        root.path(),
        &["fmt", "--stdin", "--language", "rust"],
        Some(source),
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        output.stdout,
        b"const A: &str = \"select 1\";\n// semblock:sql\nconst B: &str = r#\"SELECT 2\"#;\n"
    );
    for field in ["raw_strings", "interpreted_strings"] {
        fs::write(&path, format!("[rust]\n{field} = false\n")).unwrap();
        let source = if field == "raw_strings" {
            "// semblock:sql\nconst SQL: &str = r\"select 1\";"
        } else {
            "// semblock:sql\nconst SQL: &str = \"select 1\";"
        };
        assert_eq!(
            run(
                root.path(),
                &["fmt", "--stdin", "--language", "rust"],
                Some(source)
            )
            .status
            .code(),
            Some(3)
        );
    }
}

#[test]
fn one_rust_failure_prevents_all_project_writes() {
    for invalid in [
        "fn broken( {",
        "// semblock:sql\nconst SQL: &str = \"select from\";",
        "// semblock:sql\nconst VALUE: i32 = 1;",
    ] {
        let root = TempDir::new().unwrap();
        let good = "const SQL: &str = \"select 1\";\n";
        fs::write(root.path().join("a.rs"), good).unwrap();
        fs::write(root.path().join("b.rs"), invalid).unwrap();
        fs::write(root.path().join("c.sql"), "select 2;\n").unwrap();
        let output = run(root.path(), &["fmt", "."], None);
        assert_eq!(output.status.code(), Some(3), "{output:?}");
        assert_eq!(fs::read_to_string(root.path().join("a.rs")).unwrap(), good);
        assert_eq!(
            fs::read_to_string(root.path().join("b.rs")).unwrap(),
            invalid
        );
        assert_eq!(
            fs::read_to_string(root.path().join("c.sql")).unwrap(),
            "select 2;\n"
        );
    }
}

#[test]
fn strict_unsupported_rust_sql_prevents_project_writes() {
    let root = TempDir::new().unwrap();
    fs::write(
        root.path().join("semblock.toml"),
        "[format]\nunsupported_policy = \"error\"\n",
    )
    .unwrap();
    fs::write(root.path().join("a.sql"), "select 1;\n").unwrap();
    let opaque = r#"const SQL: &str = "select * from xmltable('/r' passing '<r/>' columns id int path '@id')";"#;
    fs::write(root.path().join("b.rs"), opaque).unwrap();
    let output = run(root.path(), &["fmt", "."], None);
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    assert_eq!(
        fs::read_to_string(root.path().join("a.sql")).unwrap(),
        "select 1;\n"
    );
    assert_eq!(
        fs::read_to_string(root.path().join("b.rs")).unwrap(),
        opaque
    );
}

#[test]
fn staged_and_changed_since_selection_include_rust_without_mutating_the_index() {
    let root = TempDir::new().unwrap();
    git(root.path(), &["init", "-b", "main"]);
    git(root.path(), &["config", "user.name", "Tests"]);
    git(
        root.path(),
        &["config", "user.email", "tests@example.invalid"],
    );
    fs::write(
        root.path().join("queries.rs"),
        "const SQL: &str = \"SELECT 1\";\n",
    )
    .unwrap();
    git(root.path(), &["add", "queries.rs"]);
    git(root.path(), &["commit", "-m", "baseline"]);
    let source = "const SQL: &str = \"select 2\";\n";
    fs::write(root.path().join("queries.rs"), source).unwrap();
    git(root.path(), &["add", "queries.rs"]);
    let index = git(root.path(), &["show", ":queries.rs"]).stdout;
    assert_eq!(
        run(root.path(), &["check", "--staged"], None).status.code(),
        Some(1)
    );
    assert_eq!(
        run(root.path(), &["diff", "--changed-since", "HEAD"], None)
            .status
            .code(),
        Some(1)
    );
    assert_eq!(
        fs::read_to_string(root.path().join("queries.rs")).unwrap(),
        source
    );
    let output = run(root.path(), &["fmt", "--staged"], None);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fs::read_to_string(root.path().join("queries.rs")).unwrap(),
        "const SQL: &str = \"SELECT 2\";\n"
    );
    assert_eq!(git(root.path(), &["show", ":queries.rs"]).stdout, index);
}
