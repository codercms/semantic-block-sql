use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use serde::Deserialize;

#[derive(Deserialize)]
struct Project {
    name: String,
    repository: String,
    revision: String,
    license: String,
    license_path: String,
    test_command: Vec<String>,
}

#[test]
fn external_go_corpus_is_pinned_licensed_and_opt_in() {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/corpus/go-projects.json");
    let projects: Vec<Project> =
        serde_json::from_str(&fs::read_to_string(path).expect("read external Go corpus manifest"))
            .expect("parse external Go corpus manifest");
    assert!(projects.len() >= 3);
    let mut names = BTreeSet::new();
    for project in projects {
        assert!(names.insert(project.name));
        assert!(project.repository.starts_with("https://github.com/"));
        assert!(!matches!(
            project.revision.as_str(),
            "main" | "master" | "HEAD"
        ));
        assert!(!project.license.is_empty());
        assert!(!project.license_path.is_empty());
        assert!(!project.test_command.is_empty());
    }
    assert!(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("scripts/test-go-corpus.sh")
            .is_file()
    );
    assert!(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("scripts/test-go-corpus.ps1")
            .is_file()
    );
}
