use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use thiserror::Error;

use crate::config::GoConfig;
use crate::source::Language;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitSelection {
    Staged,
    ChangedSince(String),
}

#[derive(Debug, Error)]
pub enum GitError {
    #[error("git executable was not found")]
    Unavailable,
    #[error("{command} failed: {message}")]
    Command { command: String, message: String },
    #[cfg(windows)]
    #[error("Git returned a non-UTF-8 path, which is not supported on Windows")]
    NonUtf8Path,
}

pub fn select_files(
    selection: &GitSelection,
    language: Language,
    go: &GoConfig,
) -> Result<Vec<PathBuf>, GitError> {
    let root = repository_root()?;
    let mut paths = BTreeSet::new();

    match selection {
        GitSelection::Staged => {
            paths.extend(run_path_command(
                &root,
                &[
                    "diff",
                    "--cached",
                    "--name-only",
                    "--diff-filter=ACMR",
                    "-z",
                    "--",
                ],
            )?);
        }
        GitSelection::ChangedSince(reference) => {
            let merge_base = run_text_command(&root, &["merge-base", reference, "HEAD"])?;
            let merge_base = merge_base.trim();
            paths.extend(run_path_command(
                &root,
                &[
                    "diff",
                    "--name-only",
                    "--diff-filter=ACMR",
                    "-z",
                    merge_base,
                    "--",
                ],
            )?);
            paths.extend(run_path_command(
                &root,
                &["ls-files", "--others", "--exclude-standard", "-z", "--"],
            )?);
        }
    }

    Ok(paths
        .into_iter()
        .filter(|path| accepts(path, language, go))
        .map(|path| root.join(path))
        .collect())
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

fn repository_root() -> Result<PathBuf, GitError> {
    let current = std::env::current_dir().map_err(|error| GitError::Command {
        command: "determine current directory".into(),
        message: error.to_string(),
    })?;
    let output = run_git(&current, &["rev-parse", "--show-toplevel"])?;
    let bytes = trim_line_ending(&output.stdout);
    path_from_bytes(bytes)
}

fn run_text_command(root: &Path, args: &[&str]) -> Result<String, GitError> {
    let output = run_git(root, args)?;
    String::from_utf8(output.stdout).map_err(|error| GitError::Command {
        command: format_command(args),
        message: error.to_string(),
    })
}

fn run_path_command(root: &Path, args: &[&str]) -> Result<Vec<PathBuf>, GitError> {
    let output = run_git(root, args)?;
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(path_from_bytes)
        .collect()
}

fn run_git(root: &Path, args: &[&str]) -> Result<Output, GitError> {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                GitError::Unavailable
            } else {
                GitError::Command {
                    command: format_command(args),
                    message: error.to_string(),
                }
            }
        })?;

    if output.status.success() {
        return Ok(output);
    }

    let message = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    Err(GitError::Command {
        command: format_command(args),
        message: if message.is_empty() {
            format!("exit status {}", output.status)
        } else {
            message
        },
    })
}

fn format_command(args: &[&str]) -> String {
    format!("git {}", args.join(" "))
}

fn trim_line_ending(bytes: &[u8]) -> &[u8] {
    bytes
        .strip_suffix(b"\r\n")
        .or_else(|| bytes.strip_suffix(b"\n"))
        .unwrap_or(bytes)
}

#[cfg(unix)]
fn path_from_bytes(bytes: &[u8]) -> Result<PathBuf, GitError> {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    Ok(PathBuf::from(OsString::from_vec(bytes.to_vec())))
}

#[cfg(windows)]
fn path_from_bytes(bytes: &[u8]) -> Result<PathBuf, GitError> {
    let path = String::from_utf8(bytes.to_vec()).map_err(|_| GitError::NonUtf8Path)?;
    Ok(PathBuf::from(path))
}
