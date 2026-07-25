use std::env;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Parser, Subcommand};
use semblock::config::{Config, ConfigError};
use semblock::diff;
use semblock::discover::{DiscoverError, discover};
use semblock::rewrite::{RewriteError, atomic_replace};
use semblock::source::{FormattedSource, Language, SourceError, format_source, infer_language};

// The CLI flow is adapted from pgfmt 2.2.0 (BSD-3-Clause). See
// THIRD_PARTY_NOTICES.md. Project discovery and host extraction are semblock
// modules and do not come from upstream pgfmt.

#[derive(Debug, Parser)]
#[command(
    name = "semblock",
    about = "Format PostgreSQL using Semantic Block SQL"
)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,

    /// Explicit semblock.toml path
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    /// Read one source from stdin
    #[arg(long, global = true)]
    stdin: bool,

    /// Logical filename for stdin language detection and diagnostics
    #[arg(long, global = true)]
    filename: Option<PathBuf>,

    /// Force input language instead of inferring it from extensions
    #[arg(long, value_enum, default_value_t = Language::Auto, global = true)]
    language: Language,

    /// Number of project-discovery workers
    #[arg(long, default_value_t = default_jobs(), global = true)]
    jobs: usize,

    /// Print every inspected file
    #[arg(long, global = true, conflicts_with = "quiet")]
    verbose: bool,

    /// Suppress progress output
    #[arg(long, global = true)]
    quiet: bool,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Format files in place, or print formatted stdin
    Fmt(Paths),
    /// Check formatting without writing
    Check(Paths),
    /// Print unified diffs without writing
    Diff(Paths),
}

#[derive(Debug, clap::Args)]
struct Paths {
    /// Files or directories; defaults to the current directory
    paths: Vec<PathBuf>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Fmt,
    Check,
    Diff,
}

impl Cli {
    pub fn run(self) -> Result<ExitCode, RunError> {
        if self.jobs == 0 {
            return Err(RunError::usage("--jobs must be greater than zero"));
        }
        let (mode, paths) = match &self.command {
            Command::Fmt(paths) => (Mode::Fmt, paths.paths.clone()),
            Command::Check(paths) => (Mode::Check, paths.paths.clone()),
            Command::Diff(paths) => (Mode::Diff, paths.paths.clone()),
        };
        let config = Config::load(self.config.as_deref()).map_err(RunError::config)?;

        if self.stdin {
            if !paths.is_empty() {
                return Err(RunError::usage(
                    "--stdin cannot be combined with filesystem paths",
                ));
            }
            return self.run_stdin(mode, &config);
        }
        if self.filename.is_some() {
            return Err(RunError::usage("--filename requires --stdin"));
        }
        self.run_paths(mode, &paths, &config)
    }

    fn run_stdin(self, mode: Mode, config: &Config) -> Result<ExitCode, RunError> {
        let filename = self
            .filename
            .as_deref()
            .ok_or_else(|| RunError::usage("--stdin requires --filename"))?;
        let language = infer_language(filename, self.language).map_err(RunError::source)?;
        let mut source = String::new();
        io::stdin()
            .read_to_string(&mut source)
            .map_err(|error| RunError::filesystem(format!("failed to read stdin: {error}")))?;
        let formatted = format_source(&source, language, &config.format, &config.go)
            .map_err(RunError::source)?;
        emit_warnings(filename, &formatted, self.quiet);

        match mode {
            Mode::Fmt => {
                print!("{}", formatted.output);
                Ok(ExitCode::SUCCESS)
            }
            Mode::Check if formatted.changed => {
                if !self.quiet {
                    eprintln!("Would reformat: {}", filename.display());
                }
                Ok(ExitCode::from(1))
            }
            Mode::Check => Ok(ExitCode::SUCCESS),
            Mode::Diff if formatted.changed => {
                print!(
                    "{}",
                    diff::unified(&filename.display().to_string(), &source, &formatted.output)
                );
                Ok(ExitCode::from(1))
            }
            Mode::Diff => Ok(ExitCode::SUCCESS),
        }
    }

    fn run_paths(
        self,
        mode: Mode,
        paths: &[PathBuf],
        config: &Config,
    ) -> Result<ExitCode, RunError> {
        let roots = if paths.is_empty() {
            vec![PathBuf::from(".")]
        } else {
            paths.to_vec()
        };
        let files = discover(
            &roots,
            self.language,
            &config.discovery,
            &config.go,
            self.jobs,
        )
        .map_err(RunError::discovery)?;

        let mut plans = Vec::with_capacity(files.len());
        for path in files {
            if self.verbose {
                eprintln!("Inspecting: {}", path.display());
            }
            let language = infer_language(&path, self.language).map_err(RunError::source)?;
            let source = fs::read_to_string(&path)
                .map_err(|error| RunError::filesystem(format!("{}: {error}", path.display())))?;
            let formatted = format_source(&source, language, &config.format, &config.go)
                .map_err(|error| RunError::source_with_path(&path, error))?;
            emit_warnings(&path, &formatted, self.quiet);
            plans.push(Plan {
                path,
                source,
                formatted,
            });
        }

        let changed = plans.iter().filter(|plan| plan.formatted.changed).count();
        match mode {
            Mode::Fmt => {
                for plan in plans.iter().filter(|plan| plan.formatted.changed) {
                    atomic_replace(&plan.path, &plan.formatted.output)
                        .map_err(RunError::rewrite)?;
                    if !self.quiet {
                        eprintln!("Formatted: {}", plan.path.display());
                    }
                }
                Ok(ExitCode::SUCCESS)
            }
            Mode::Check => {
                if !self.quiet {
                    for plan in plans.iter().filter(|plan| plan.formatted.changed) {
                        eprintln!("Would reformat: {}", plan.path.display());
                    }
                }
                Ok(if changed == 0 {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(1)
                })
            }
            Mode::Diff => {
                for plan in plans.iter().filter(|plan| plan.formatted.changed) {
                    print!(
                        "{}",
                        diff::unified(
                            &display_path(&plan.path),
                            &plan.source,
                            &plan.formatted.output
                        )
                    );
                }
                Ok(if changed == 0 {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(1)
                })
            }
        }
    }
}

struct Plan {
    path: PathBuf,
    source: String,
    formatted: FormattedSource,
}

fn emit_warnings(path: &Path, formatted: &FormattedSource, quiet: bool) {
    if quiet {
        return;
    }
    for warning in &formatted.warnings {
        eprintln!("{}: warning: {warning:?}", path.display());
    }
}

fn display_path(path: &Path) -> String {
    let current = env::current_dir().ok();
    current
        .as_deref()
        .and_then(|current| path.strip_prefix(current).ok())
        .unwrap_or(path)
        .to_string_lossy()
        .trim_start_matches("./")
        .to_string()
}

fn default_jobs() -> usize {
    std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(1)
}

#[derive(Debug)]
pub struct RunError {
    code: u8,
    message: String,
}

impl RunError {
    fn usage(message: impl Into<String>) -> Self {
        Self {
            code: 2,
            message: message.into(),
        }
    }

    fn config(error: ConfigError) -> Self {
        Self::usage(error.to_string())
    }

    fn source(error: SourceError) -> Self {
        match error {
            SourceError::UnknownLanguage(_) | SourceError::GoDisabled => {
                Self::usage(error.to_string())
            }
            _ => Self {
                code: 3,
                message: error.to_string(),
            },
        }
    }

    fn source_with_path(path: &Path, error: SourceError) -> Self {
        match error {
            SourceError::UnknownLanguage(_) | SourceError::GoDisabled => {
                Self::usage(format!("{}: {error}", path.display()))
            }
            _ => Self {
                code: 3,
                message: format!("{}: {error}", path.display()),
            },
        }
    }

    fn discovery(error: DiscoverError) -> Self {
        Self::filesystem(error.to_string())
    }

    fn rewrite(error: RewriteError) -> Self {
        Self::filesystem(error.to_string())
    }

    fn filesystem(message: impl Into<String>) -> Self {
        Self {
            code: 4,
            message: message.into(),
        }
    }

    pub fn exit_code(&self) -> ExitCode {
        ExitCode::from(self.code)
    }
}

impl std::fmt::Display for RunError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.message)
    }
}
