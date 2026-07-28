use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitSelection {
    Staged,
    ChangedSince(String),
}

#[derive(Debug)]
pub struct SelectedPaths {
    pub root: PathBuf,
    pub paths: Vec<PathBuf>,
}

#[derive(Debug)]
pub struct StagedFile {
    pub path: PathBuf,
    pub source: String,
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
    #[error("staged blob is not UTF-8: {0}")]
    NonUtf8Blob(PathBuf),
}

pub fn select_files(selection: &GitSelection) -> Result<SelectedPaths, GitError> {
    let root = repository_root()?;
    let mut paths = BTreeSet::new();

    match selection {
        GitSelection::Staged => {
            paths.extend(run_path_command(
                &root,
                os_args(&[
                    "diff",
                    "--cached",
                    "--name-only",
                    "--diff-filter=ACMR",
                    "-z",
                    "--",
                ]),
            )?);
        }
        GitSelection::ChangedSince(reference) => {
            let merge_base = run_text_command(
                &root,
                vec![
                    OsString::from("merge-base"),
                    OsString::from(reference),
                    OsString::from("HEAD"),
                ],
            )?;
            let merge_base = merge_base.trim();
            paths.extend(run_path_command(
                &root,
                vec![
                    OsString::from("diff"),
                    OsString::from("--name-only"),
                    OsString::from("--diff-filter=ACMR"),
                    OsString::from("-z"),
                    OsString::from(merge_base),
                    OsString::from("--"),
                ],
            )?);
            paths.extend(run_path_command(
                &root,
                os_args(&["ls-files", "--others", "--exclude-standard", "-z", "--"]),
            )?);
        }
    }

    Ok(SelectedPaths {
        root,
        paths: paths.into_iter().collect(),
    })
}

pub fn read_staged_files(root: &Path, paths: &[PathBuf]) -> Result<Vec<StagedFile>, GitError> {
    paths
        .iter()
        .map(|path| {
            let mut blob_spec = OsString::from(":");
            blob_spec.push(path.as_os_str());
            let output = run_git(
                root,
                &[
                    OsString::from("cat-file"),
                    OsString::from("blob"),
                    blob_spec,
                ],
            )?;
            let source = String::from_utf8(output.stdout)
                .map_err(|_| GitError::NonUtf8Blob(path.clone()))?;
            Ok(StagedFile {
                path: path.clone(),
                source,
            })
        })
        .collect()
}

fn repository_root() -> Result<PathBuf, GitError> {
    let current = std::env::current_dir().map_err(|error| GitError::Command {
        command: "determine current directory".into(),
        message: error.to_string(),
    })?;
    let output = run_git(&current, &os_args(&["rev-parse", "--show-toplevel"]))?;
    let bytes = trim_line_ending(&output.stdout);
    path_from_bytes(bytes)
}

fn run_text_command(root: &Path, args: Vec<OsString>) -> Result<String, GitError> {
    let output = run_git(root, &args)?;
    String::from_utf8(output.stdout).map_err(|error| GitError::Command {
        command: format_command(&args),
        message: error.to_string(),
    })
}

fn run_path_command(root: &Path, args: Vec<OsString>) -> Result<Vec<PathBuf>, GitError> {
    let output = run_git(root, &args)?;
    output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(path_from_bytes)
        .collect()
}

fn run_git(root: &Path, args: &[OsString]) -> Result<Output, GitError> {
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

fn os_args(args: &[&str]) -> Vec<OsString> {
    args.iter().map(OsString::from).collect()
}

fn format_command(args: &[OsString]) -> String {
    let mut command = OsString::from("git");
    for arg in args {
        command.push(OsStr::new(" "));
        command.push(arg);
    }
    command.to_string_lossy().into_owned()
}

fn trim_line_ending(bytes: &[u8]) -> &[u8] {
    bytes
        .strip_suffix(b"\r\n")
        .or_else(|| bytes.strip_suffix(b"\n"))
        .unwrap_or(bytes)
}

#[cfg(unix)]
fn path_from_bytes(bytes: &[u8]) -> Result<PathBuf, GitError> {
    use std::os::unix::ffi::OsStringExt;

    Ok(PathBuf::from(OsString::from_vec(bytes.to_vec())))
}

#[cfg(windows)]
fn path_from_bytes(bytes: &[u8]) -> Result<PathBuf, GitError> {
    let path = String::from_utf8(bytes.to_vec()).map_err(|_| GitError::NonUtf8Path)?;
    Ok(PathBuf::from(path))
}
