use std::fs;
use std::path::PathBuf;

use rayon::prelude::*;
use rayon::ThreadPoolBuilder;
use semblock::config::Config;
use semblock::source::{FormattedSource, Language, format_source, infer_language};

use super::RunError;

pub(super) struct Plan {
    pub(super) path: PathBuf,
    pub(super) source: String,
    pub(super) formatted: FormattedSource,
}

pub(super) fn build_plans(
    files: Vec<PathBuf>,
    requested_language: Language,
    config: &Config,
    jobs: usize,
) -> Result<Vec<Plan>, RunError> {
    if files.is_empty() {
        return Ok(Vec::new());
    }

    let pool = ThreadPoolBuilder::new()
        .num_threads(jobs.min(files.len()).max(1))
        .build()
        .map_err(|error| {
            RunError::filesystem(format!("failed to initialize formatting workers: {error}"))
        })?;

    pool.install(|| {
        files
            .par_iter()
            .map(|path| plan_file(path.clone(), requested_language, config))
            .collect()
    })
}

fn plan_file(
    path: PathBuf,
    requested_language: Language,
    config: &Config,
) -> Result<Plan, RunError> {
    let language = infer_language(&path, requested_language)
        .map_err(|error| RunError::source_with_path(&path, error))?;
    let source = fs::read_to_string(&path)
        .map_err(|error| RunError::filesystem(format!("{}: {error}", path.display())))?;
    let formatted = format_source(&source, language, &config.format, &config.go)
        .map_err(|error| RunError::source_with_path(&path, error))?;

    Ok(Plan {
        path,
        source,
        formatted,
    })
}
