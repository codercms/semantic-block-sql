use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

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
    let next = AtomicUsize::new(0);
    let (sender, receiver) = mpsc::channel();

    std::thread::scope(|scope| {
        for _ in 0..worker_count {
            let sender = sender.clone();
            let files = &files;
            let next = &next;
            scope.spawn(move || {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = files.get(index) else {
                        break;
                    };
                    let result = plan_file(path.clone(), requested_language, config);
                    if sender.send((index, result)).is_err() {
                        break;
                    }
                }
            });
        }
    });
    drop(sender);

    let mut results: Vec<Option<Result<Plan, RunError>>> =
        std::iter::repeat_with(|| None).take(files.len()).collect();
    for (index, result) in receiver {
        results[index] = Some(result);
    }

    results
        .into_iter()
        .map(|result| result.expect("every planning job returned a result"))
        .collect()
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
