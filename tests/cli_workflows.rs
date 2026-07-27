use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use tempfile::TempDir;

fn run(root: &Path, args: &[&str], stdin: Option<&str>) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_semblock"));
    command.current_dir(root).args(args);
    match stdin {
        None => command.output().expect("run semblock"),
        Some(input) => {
            let mut child = command
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .expect("spawn semblock");
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input.as_bytes())
                .unwrap();
            child.wait_with_output().unwrap()
        }
    }
}

fn write(root: &Path, name: &str, source: &str) {
    fs::write(root.join(name), source).unwrap();
}
fn git(root: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .current_dir(root)
            .args(args)
            .status()
            .unwrap()
            .success()
    );
}

#[test]
fn typed_stdin_uses_a_synthetic_filename() {
    let root = TempDir::new().unwrap();
    let output = run(
        root.path(),
        &["fmt", "--stdin", "--language", "sql"],
        Some("select 1;\n"),
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "SELECT 1;\n");
}

#[test]
fn check_lists_changed_files_and_summarizes() {
    let root = TempDir::new().unwrap();
    write(root.path(), "b.sql", "select b;\n");
    write(root.path(), "a.sql", "select a;\n");
    let output = run(
        root.path(),
        &["check", ".", "--list-different", "--summary", "--jobs", "2"],
        None,
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "a.sql\nb.sql\n");
    assert!(String::from_utf8_lossy(&output.stderr).contains("Checked 2 input(s): 2 would change"));
}

#[test]
fn config_commands_and_init_are_available() {
    let root = TempDir::new().unwrap();
    assert!(run(root.path(), &["init"], None).status.success());
    let output = run(root.path(), &["config", "show"], None);
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stdout).contains("dialect = \"postgresql\""));
}

#[test]
fn staged_selection_ignores_unstaged_and_unsupported_files() {
    let root = TempDir::new().unwrap();
    git(root.path(), &["init", "-b", "main"]);
    git(root.path(), &["config", "user.name", "Tests"]);
    git(root.path(), &["config", "user.email", "tests@example.invalid"]);
    write(root.path(), "query.sql", "SELECT 1;\n");
    write(root.path(), "note.txt", "baseline\n");
    git(root.path(), &["add", "."]);
    git(root.path(), &["commit", "-m", "baseline"]);
    write(root.path(), "query.sql", "select 1;\n");
    write(root.path(), "note.txt", "changed\n");
    git(root.path(), &["add", "."]);
    let output = run(
        root.path(),
        &["check", "--staged", "--list-different"],
        None,
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "query.sql\n");
}
