#[path = "support/project.rs"]
mod project;

use std::fs;
use std::process::Command;

use project::{assert_goldens, fixture_copy, semblock, tree};
use tempfile::TempDir;

#[test]
fn formats_the_go_equivalent_cargo_project_deterministically_and_compiles_it() {
    let serial = fixture_copy("rust-project");
    let parallel = fixture_copy("rust-project");
    let before = tree(serial.path());
    let check = semblock(serial.path(), &["check", ".", "--list-different"]);
    assert_eq!(check.status.code(), Some(1), "{check:?}");
    let changed = String::from_utf8_lossy(&check.stdout)
        .lines()
        .map(|line| line.trim_start_matches(".\\").replace('\\', "/"))
        .collect::<Vec<_>>();
    assert_eq!(
        changed,
        [
            "src/audit.rs",
            "src/catalog.rs",
            "src/migrations.rs",
            "src/orders.rs",
            "src/users.rs",
            "tests/catalog.rs"
        ]
    );
    let diff = semblock(serial.path(), &["diff", "."]);
    assert_eq!(diff.status.code(), Some(1), "{diff:?}");
    assert_eq!(tree(serial.path()), before, "check/diff must not write");
    for (root, jobs) in [(serial.path(), "1"), (parallel.path(), "4")] {
        let output = semblock(root, &["fmt", ".", "--jobs", jobs]);
        assert!(output.status.success(), "{output:?}");
        assert_goldens(root);
    }
    assert_eq!(tree(serial.path()), tree(parallel.path()));
    let formatted = tree(serial.path());
    assert_eq!(
        formatted[std::path::Path::new("src/ignored/legacy.rs")],
        before[std::path::Path::new("src/ignored/legacy.rs")]
    );
    assert!(semblock(serial.path(), &["check", "."]).status.success());
    assert!(semblock(serial.path(), &["fmt", "."]).status.success());
    assert_eq!(
        tree(serial.path()),
        formatted,
        "second pass must be byte-identical"
    );

    let target = TempDir::new().unwrap();
    for action in ["check", "test"] {
        let output = Command::new("cargo")
            .current_dir(serial.path())
            .args([
                action,
                "--locked",
                "--offline",
                "--all-targets",
                "--target-dir",
            ])
            .arg(target.path())
            .output()
            .unwrap();
        assert!(output.status.success(), "{action}: {output:?}");
    }
    assert_eq!(
        tree(serial.path()),
        formatted,
        "Cargo must not mutate fixture sources or lockfile"
    );
}

#[test]
#[ignore = "opt-in: fetch the pinned SQLx fixture dependencies before running"]
fn actual_sqlx_macros_compile_before_and_after_formatting_offline() {
    let root = fixture_copy("rust-sqlx-project");
    let target = TempDir::new().unwrap();
    for formatted in [false, true] {
        if formatted {
            let output = semblock(root.path(), &["fmt", "."]);
            assert!(output.status.success(), "{output:?}");
            assert_goldens(root.path());
            assert!(semblock(root.path(), &["check", "."]).status.success());
        }
        let output = Command::new("cargo")
            .current_dir(root.path())
            .env("SQLX_OFFLINE", "true")
            .env_remove("DATABASE_URL")
            .args([
                "check",
                "--locked",
                "--offline",
                "--all-targets",
                "--target-dir",
            ])
            .arg(target.path())
            .output()
            .unwrap();
        assert!(output.status.success(), "formatted={formatted}: {output:?}");
    }
    let before = fs::read(root.path().join("src/lib.rs")).unwrap();
    assert!(semblock(root.path(), &["fmt", "."]).status.success());
    assert_eq!(fs::read(root.path().join("src/lib.rs")).unwrap(), before);
}
