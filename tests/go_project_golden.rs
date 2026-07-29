use std::collections::BTreeMap;
use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use tempfile::TempDir;

fn semblock(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_semblock"))
        .current_dir(root)
        .args(args)
        .output()
        .expect("run semblock")
}

fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join(name)
}

fn copy_tree(source: &Path, destination: &Path) {
    fs::create_dir_all(destination).expect("create fixture destination");
    for entry in fs::read_dir(source).expect("read fixture directory") {
        let entry = entry.expect("read fixture entry");
        let source_path = entry.path();
        let destination_path = destination.join(entry.file_name());
        if entry.file_type().expect("read fixture entry type").is_dir() {
            copy_tree(&source_path, &destination_path);
        } else {
            fs::copy(&source_path, &destination_path).expect("copy fixture file");
        }
    }
}

fn visit_files(root: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(root).expect("read project directory") {
        let entry = entry.expect("read project entry");
        let path = entry.path();
        if entry.file_type().expect("read project entry type").is_dir() {
            visit_files(&path, files);
        } else {
            files.push(path);
        }
    }
}

fn all_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    visit_files(root, &mut files);
    files.sort();
    files
}

fn go_files(root: &Path) -> Vec<PathBuf> {
    all_files(root)
        .into_iter()
        .filter(|path| path.extension() == Some(OsStr::new("go")))
        .collect()
}

fn golden_pairs(root: &Path) -> Vec<(PathBuf, PathBuf)> {
    let mut pairs = all_files(root)
        .into_iter()
        .filter_map(|expected| {
            let name = expected.file_name()?.to_str()?;
            let actual_name = name.strip_suffix(".expected")?;
            if !actual_name.ends_with(".go") {
                return None;
            }
            Some((expected.with_file_name(actual_name), expected))
        })
        .collect::<Vec<_>>();
    pairs.sort();
    pairs
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    all_files(root)
        .into_iter()
        .map(|path| {
            let relative = path.strip_prefix(root).expect("fixture-relative path");
            (
                relative.to_path_buf(),
                fs::read(path).expect("read snapshot file"),
            )
        })
        .collect()
}

fn run_go(root: &Path, program: &str, args: &[&str]) -> Output {
    Command::new(program)
        .current_dir(root)
        .args(args)
        .env("CGO_ENABLED", "0")
        .env("GOPROXY", "off")
        .env("GOSUMDB", "off")
        .env("GOTOOLCHAIN", "local")
        .env("GOWORK", "off")
        .output()
        .unwrap_or_else(|error| panic!("run {program}: {error}"))
}

fn assert_go_project_is_valid(root: &Path) {
    let files = go_files(root);
    let gofmt = Command::new("gofmt")
        .arg("-l")
        .args(&files)
        .output()
        .expect("run gofmt; Go 1.22+ is required for integration tests");
    assert!(gofmt.status.success(), "{gofmt:?}");
    assert!(
        gofmt.stdout.is_empty(),
        "gofmt reported unformatted fixture files:\n{}",
        String::from_utf8_lossy(&gofmt.stdout)
    );

    let test = run_go(root, "go", &["test", "./..."]);
    assert!(
        test.status.success(),
        "formatted fixture project did not compile:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&test.stdout),
        String::from_utf8_lossy(&test.stderr)
    );
}

#[test]
fn formats_a_realistic_go_project_against_complete_golden_files() {
    let project = TempDir::new().expect("create temporary project");
    copy_tree(&fixture("go-project"), project.path());

    let pairs = golden_pairs(project.path());
    let actual_go = go_files(project.path());
    assert_eq!(
        pairs.iter().map(|(actual, _)| actual).collect::<Vec<_>>(),
        actual_go.iter().collect::<Vec<_>>(),
        "every Go fixture must have an adjacent .go.expected file"
    );

    let check = semblock(
        project.path(),
        &["check", ".", "--list-different", "--jobs", "4"],
    );
    assert_eq!(check.status.code(), Some(1), "{check:?}");
    let changed = String::from_utf8_lossy(&check.stdout).replace('\\', "/");
    assert_eq!(
        changed,
        "internal/orders/queries.go\ninternal/users/repository.go\n"
    );

    let format = semblock(project.path(), &["fmt", ".", "--jobs", "4"]);
    assert!(format.status.success(), "{format:?}");

    for (actual, expected) in &pairs {
        assert_eq!(
            fs::read(actual).expect("read formatted Go file"),
            fs::read(expected).expect("read expected Go file"),
            "golden mismatch for {}",
            actual.strip_prefix(project.path()).unwrap().display()
        );
    }

    let first_pass = snapshot(project.path());
    let second_format = semblock(project.path(), &["fmt", ".", "--jobs", "1"]);
    assert!(second_format.status.success(), "{second_format:?}");
    assert_eq!(
        snapshot(project.path()),
        first_pass,
        "Go project formatting is not idempotent"
    );

    let clean = semblock(project.path(), &["check", ".", "--jobs", "4"]);
    assert!(clean.status.success(), "{clean:?}");

    assert_go_project_is_valid(project.path());
}

#[test]
fn invalid_embedded_sql_aborts_every_go_file_rewrite() {
    let project = TempDir::new().expect("create temporary project");
    copy_tree(&fixture("go-project-invalid"), project.path());
    assert_go_project_is_valid(project.path());

    let valid_check = semblock(project.path(), &["check", "valid.go", "--list-different"]);
    assert_eq!(valid_check.status.code(), Some(1), "{valid_check:?}");

    let before = snapshot(project.path());
    let format = semblock(project.path(), &["fmt", ".", "--jobs", "4"]);
    assert_eq!(format.status.code(), Some(3), "{format:?}");
    assert!(
        String::from_utf8_lossy(&format.stderr).contains("PostgreSQL parse failed"),
        "{format:?}"
    );
    assert_eq!(
        snapshot(project.path()),
        before,
        "failed Go project was partially rewritten"
    );
}
