use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

use ignore::{WalkBuilder, WalkState};
use thiserror::Error;

use crate::config::{DiscoveryConfig, GoConfig};
use crate::source::Language;

#[derive(Debug, Error)]
pub enum DiscoverError {
    #[error("input path does not exist: {0}")]
    Missing(PathBuf),
    #[error("failed to inspect {path}: {source}")]
    Metadata {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("discovery failed: {0}")]
    Walk(String),
}

pub fn discover(
    roots: &[PathBuf],
    language: Language,
    discovery: &DiscoveryConfig,
    go: &GoConfig,
    jobs: usize,
) -> Result<Vec<PathBuf>, DiscoverError> {
    let mut explicit = BTreeSet::new();
    let mut directories = Vec::new();

    for root in roots {
        let metadata = std::fs::metadata(root).map_err(|source| {
            if source.kind() == std::io::ErrorKind::NotFound {
                DiscoverError::Missing(root.clone())
            } else {
                DiscoverError::Metadata {
                    path: root.clone(),
                    source,
                }
            }
        })?;
        if metadata.is_file() {
            explicit.insert(root.clone());
        } else if metadata.is_dir() {
            directories.push(root.clone());
        }
    }

    let mut walked = Vec::new();
    let mut walk_errors = Vec::new();
    for directory in directories {
        let mut builder = WalkBuilder::new(directory);
        builder
            .standard_filters(true)
            .hidden(true)
            .follow_links(false)
            .threads(jobs.max(1))
            .git_ignore(discovery.respect_gitignore)
            .git_global(discovery.respect_gitignore)
            .git_exclude(discovery.respect_gitignore)
            .require_git(false)
            .add_custom_ignore_filename(&discovery.ignore_file);

        let walker = builder.build_parallel();
        let (sender, receiver) = mpsc::channel();
        walker.run(|| {
            let sender = sender.clone();
            Box::new(move |entry| {
                match entry {
                    Ok(entry) if entry.file_type().is_some_and(|kind| kind.is_file()) => {
                        if accepts(entry.path(), language, go) {
                            let _ = sender.send(Ok(entry.into_path()));
                        }
                    }
                    Ok(_) => {}
                    Err(error) => {
                        let _ = sender.send(Err(error.to_string()));
                    }
                }
                WalkState::Continue
            })
        });
        drop(sender);
        for result in receiver {
            match result {
                Ok(path) => walked.push(path),
                Err(error) => walk_errors.push(error),
            }
        }
    }

    if !walk_errors.is_empty() {
        return Err(DiscoverError::Walk(walk_errors.join("; ")));
    }
    explicit.extend(walked);
    Ok(explicit.into_iter().collect())
}

fn accepts(path: &Path, language: Language, go: &GoConfig) -> bool {
    let extension = path.extension().and_then(|extension| extension.to_str());
    match language {
        Language::Sql => extension.is_some_and(|extension| extension.eq_ignore_ascii_case("sql")),
        Language::Go => {
            go.enabled && extension.is_some_and(|extension| extension.eq_ignore_ascii_case("go"))
        }
        Language::Auto => match extension {
            Some(extension) if extension.eq_ignore_ascii_case("sql") => true,
            Some(extension) if extension.eq_ignore_ascii_case("go") => go.enabled,
            _ => false,
        },
    }
}
