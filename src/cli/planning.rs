use std::fs;
use std::path::PathBuf;

use rayon::ThreadPoolBuilder;
use rayon::prelude::*;
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

    let worker_count = jobs.min(files.len()).max(1);
    let pool = ThreadPoolBuilder::new()
        .num_threads(worker_count)
        .thread_name(|index| format!("semblock-plan-{index}"))
        .build()
        .map_err(|error| {
            RunError::filesystem(format!("failed to create formatting thread pool: {error}"))
        })?;

    let results = pool.install(|| {
        files
            .into_par_iter()
            .map(|path| plan_file(path, requested_language, config))
            .collect::<Vec<_>>()
    });

    // Preserve file-order error selection and output ordering regardless of
    // worker completion order.
    results.into_iter().collect()
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
