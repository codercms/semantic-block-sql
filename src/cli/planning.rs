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

pub(super) enum PlanInput {
    Worktree(PathBuf),
    Provided { path: PathBuf, source: String },
}

impl PlanInput {
    pub(super) fn path(&self) -> &PathBuf {
        match self {
            Self::Worktree(path) | Self::Provided { path, .. } => path,
        }
    }
}

pub(super) fn build_plans(
    inputs: Vec<PlanInput>,
    requested_language: Language,
    config: &Config,
    jobs: usize,
) -> Result<Vec<Plan>, RunError> {
    if inputs.is_empty() {
        return Ok(Vec::new());
    }

    let worker_count = jobs.min(inputs.len()).max(1);
    let pool = ThreadPoolBuilder::new()
        .num_threads(worker_count)
        .thread_name(|index| format!("semblock-plan-{index}"))
        .build()
        .map_err(|error| {
            RunError::filesystem(format!("failed to create formatting thread pool: {error}"))
        })?;

    let results = pool.install(|| {
        inputs
            .into_par_iter()
            .map(|input| plan_file(input, requested_language, config))
            .collect::<Vec<_>>()
    });

    // Preserve file-order error selection and output ordering regardless of
    // worker completion order.
    results.into_iter().collect()
}

fn plan_file(
    input: PlanInput,
    requested_language: Language,
    config: &Config,
) -> Result<Plan, RunError> {
    let (path, source) = match input {
        PlanInput::Worktree(path) => {
            let source = fs::read_to_string(&path)
                .map_err(|error| RunError::filesystem(format!("{}: {error}", path.display())))?;
            (path, source)
        }
        PlanInput::Provided { path, source } => (path, source),
    };
    let language = infer_language(&path, requested_language)
        .map_err(|error| RunError::source_with_path(&path, error))?;
    let formatted = format_source(&source, language, &config.format, &config.go)
        .map_err(|error| RunError::source_with_path(&path, error))?;

    Ok(Plan {
        path,
        source,
        formatted,
    })
}
