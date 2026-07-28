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
    let path = root.join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, source).unwrap();
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

fn git_output(root: &Path, args: &[&str]) -> Output {
    Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .expect("run git")
}

fn init_git(root: &Path) {
    git(root, &["init", "-b", "main"]);
    git(root, &["config", "user.name", "Tests"]);
    git(root, &["config", "user.email", "tests@example.invalid"]);
}

fn commit_all(root: &Path, message: &str) {
    git(root, &["add", "."]);
    git(root, &["commit", "-m", message]);
}

fn index_blob(root: &Path, path: &str) -> Vec<u8> {
    let output = git_output(root, &["cat-file", "blob", &format!(":{path}")]);
    assert!(output.status.success(), "{output:?}");
    output.stdout
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
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace(".\\", ""),
        "a.sql\nb.sql\n"
    );
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
    init_git(root.path());
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

#[test]
fn staged_check_and_diff_read_the_index_blob() {
    let root = TempDir::new().unwrap();
    init_git(root.path());
    write(root.path(), "query.sql", "SELECT 1;\n");
    commit_all(root.path(), "baseline");

    write(root.path(), "query.sql", "select 1;\n");
    git(root.path(), &["add", "query.sql"]);
    write(root.path(), "query.sql", "SELECT 1;\n");

    let check = run(
        root.path(),
        &["check", "--staged", "--list-different"],
        None,
    );
    assert_eq!(check.status.code(), Some(1), "{check:?}");
    assert_eq!(String::from_utf8_lossy(&check.stdout), "query.sql\n");

    let diff = run(root.path(), &["diff", "--staged"], None);
    assert_eq!(diff.status.code(), Some(1), "{diff:?}");
    let stdout = String::from_utf8_lossy(&diff.stdout);
    assert!(stdout.contains("-select 1;"), "{stdout}");
    assert!(stdout.contains("+SELECT 1;"), "{stdout}");
}

#[test]
fn staged_fmt_rejects_partial_staging_without_modifying_either_copy() {
    let root = TempDir::new().unwrap();
    init_git(root.path());
    write(root.path(), "query.sql", "SELECT 1;\n");
    commit_all(root.path(), "baseline");

    write(root.path(), "query.sql", "select 1;\n");
    git(root.path(), &["add", "query.sql"]);
    write(root.path(), "query.sql", "SELECT 1;\n");
    let before_index = index_blob(root.path(), "query.sql");
    let before_worktree = fs::read(root.path().join("query.sql")).unwrap();

    let output = run(root.path(), &["fmt", "--staged"], None);
    assert_eq!(output.status.code(), Some(4), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("query.sql"),
        "{output:?}"
    );
    assert_eq!(index_blob(root.path(), "query.sql"), before_index);
    assert_eq!(
        fs::read(root.path().join("query.sql")).unwrap(),
        before_worktree
    );
}

#[test]
fn staged_fmt_formats_worktree_but_leaves_index_unchanged() {
    let root = TempDir::new().unwrap();
    init_git(root.path());
    write(root.path(), "query.sql", "SELECT 1;\n");
    commit_all(root.path(), "baseline");

    write(root.path(), "query.sql", "select 1;\n");
    git(root.path(), &["add", "query.sql"]);
    let before_index = index_blob(root.path(), "query.sql");

    let output = run(root.path(), &["fmt", "--staged"], None);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fs::read_to_string(root.path().join("query.sql")).unwrap(),
        "SELECT 1;\n"
    );
    assert_eq!(index_blob(root.path(), "query.sql"), before_index);
}

#[test]
fn staged_fmt_rejects_a_missing_worktree_file() {
    let root = TempDir::new().unwrap();
    init_git(root.path());
    write(root.path(), "query.sql", "select 1;\n");
    git(root.path(), &["add", "query.sql"]);
    let before_index = index_blob(root.path(), "query.sql");
    fs::remove_file(root.path().join("query.sql")).unwrap();

    let output = run(root.path(), &["fmt", "--staged"], None);
    assert_eq!(output.status.code(), Some(4), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("query.sql"),
        "{output:?}"
    );
    assert_eq!(index_blob(root.path(), "query.sql"), before_index);
    assert!(!root.path().join("query.sql").exists());
}

#[test]
fn changed_since_includes_all_live_change_sources_and_respects_nested_ignore() {
    let root = TempDir::new().unwrap();
    init_git(root.path());
    for path in [
        "unstaged.sql",
        "staged.sql",
        "deleted.sql",
        "nested/ignored.sql",
    ] {
        write(root.path(), path, "SELECT 1;\n");
    }
    write(root.path(), "nested/.semblockignore", "ignored.sql\n");
    commit_all(root.path(), "baseline");
    let base = git_output(root.path(), &["rev-parse", "HEAD"]);
    assert!(base.status.success(), "{base:?}");
    let base = String::from_utf8(base.stdout).unwrap();
    let base = base.trim();

    write(root.path(), "committed.sql", "select 1;\n");
    commit_all(root.path(), "committed change");
    write(root.path(), "unstaged.sql", "select 1;\n");
    write(root.path(), "staged.sql", "select 1;\n");
    git(root.path(), &["add", "staged.sql"]);
    write(root.path(), "untracked.sql", "select 1;\n");
    fs::remove_file(root.path().join("deleted.sql")).unwrap();
    write(root.path(), "nested/ignored.sql", "select 1;\n");

    let output = run(
        root.path(),
        &[
            "check",
            "--changed-since",
            base,
            "--list-different",
            "--jobs",
            "4",
        ],
        None,
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "committed.sql\nstaged.sql\nunstaged.sql\nuntracked.sql\n"
    );
}

#[test]
fn changed_since_rejects_an_invalid_reference() {
    let root = TempDir::new().unwrap();
    init_git(root.path());
    let output = run(
        root.path(),
        &["check", "--changed-since", "definitely-missing-ref"],
        None,
    );
    assert_eq!(output.status.code(), Some(4), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("definitely-missing-ref"),
        "{output:?}"
    );
}

#[test]
fn parallel_planning_reports_the_first_path_deterministically() {
    let root = TempDir::new().unwrap();
    write(root.path(), "b.sql", "select 'unterminated;\n");
    write(root.path(), "a.sql", "select 'unterminated;\n");

    for _ in 0..8 {
        let output = run(root.path(), &["check", ".", "--jobs", "4"], None);
        assert_eq!(output.status.code(), Some(3), "{output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("a.sql:"), "{stderr}");
    }
}

#[test]
fn staged_selection_respects_nested_semblockignore() {
    let root = TempDir::new().unwrap();
    init_git(root.path());
    write(root.path(), "nested/.semblockignore", "ignored.sql\n");
    write(root.path(), "nested/ignored.sql", "SELECT 1;\n");
    commit_all(root.path(), "baseline");
    write(root.path(), "nested/ignored.sql", "select 1;\n");
    git(root.path(), &["add", "nested/ignored.sql"]);

    let output = run(
        root.path(),
        &["check", "--staged", "--list-different"],
        None,
    );
    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
}

#[test]
fn staged_index_blobs_support_nested_repository_paths() {
    let root = TempDir::new().unwrap();
    init_git(root.path());
    write(root.path(), "nested/query.sql", "select 1;\n");
    git(root.path(), &["add", "nested/query.sql"]);

    let output = run(
        root.path(),
        &["check", "--staged", "--list-different"],
        None,
    );
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).replace('\\', "/"),
        "nested/query.sql\n"
    );
}
