use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
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
    #[error("Git candidate is not a repository-relative path: {0}")]
    InvalidCandidate(PathBuf),
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
        configure_builder(&mut builder, discovery, jobs);

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

pub fn filter_candidates(
    root: &Path,
    candidates: &[PathBuf],
    language: Language,
    discovery: &DiscoveryConfig,
    go: &GoConfig,
    jobs: usize,
) -> Result<Vec<PathBuf>, DiscoverError> {
    let mut exact = BTreeMap::new();
    let mut allowed = BTreeSet::from([root.to_path_buf()]);
    for candidate in candidates {
        if candidate.is_absolute()
            || candidate.components().any(|component| {
                matches!(
                    component,
                    Component::ParentDir | Component::RootDir | Component::Prefix(_)
                )
            })
        {
            return Err(DiscoverError::InvalidCandidate(candidate.clone()));
        }
        let path = root.join(candidate);
        exact.insert(path.clone(), candidate.clone());
        let mut ancestor = path.as_path();
        while let Some(parent) = ancestor.parent() {
            allowed.insert(ancestor.to_path_buf());
            if ancestor == root {
                break;
            }
            ancestor = parent;
        }
    }

    let mut builder = WalkBuilder::new(root);
    configure_builder(&mut builder, discovery, jobs);
    builder.filter_entry(move |entry| allowed.contains(entry.path()));

    let walker = builder.build_parallel();
    let (sender, receiver) = mpsc::channel();
    walker.run(|| {
        let sender = sender.clone();
        let exact = &exact;
        Box::new(move |entry| {
            match entry {
                Ok(entry) if entry.file_type().is_some_and(|kind| kind.is_file()) => {
                    if let Some(candidate) = exact.get(entry.path())
                        && accepts(entry.path(), language, go)
                    {
                        let _ = sender.send(Ok(candidate.clone()));
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

    let mut accepted = BTreeSet::new();
    let mut walk_errors = Vec::new();
    for result in receiver {
        match result {
            Ok(path) => {
                accepted.insert(path);
            }
            Err(error) => walk_errors.push(error),
        }
    }
    if !walk_errors.is_empty() {
        return Err(DiscoverError::Walk(walk_errors.join("; ")));
    }
    Ok(accepted.into_iter().collect())
}

fn configure_builder(builder: &mut WalkBuilder, discovery: &DiscoveryConfig, jobs: usize) {
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
}

pub fn accepts(path: &Path, language: Language, go: &GoConfig) -> bool {
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
