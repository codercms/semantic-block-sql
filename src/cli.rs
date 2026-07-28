mod output;
mod planning;

use std::fs::OpenOptions;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use semblock::config::{Config, ConfigError};
use semblock::diff;
use semblock::discover::{DiscoverError, discover, filter_candidates};
use semblock::git::{GitError, GitSelection, read_staged_files, select_files};
use semblock::rewrite::{RewriteError, atomic_replace};
use semblock::source::{Language, SourceError, format_source, infer_language};

use output::{
    CheckOutput, display_path, emit_check_path, emit_check_summary, emit_check_summary_refs,
    emit_diagnostics,
};
use planning::{PlanInput, build_plans};

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

    /// Number of discovery and formatting workers
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
    Check(CheckArgs),
    /// Print unified diffs without writing
    Diff(Paths),
    /// Inspect the resolved configuration
    Config(ConfigArgs),
    /// Create a semblock.toml configuration file
    Init(InitArgs),
}

#[derive(Debug, Clone, Args)]
struct Paths {
    /// Files or directories; defaults to the current directory
    #[arg(value_name = "PATH", conflicts_with_all = ["staged", "changed_since"])]
    paths: Vec<PathBuf>,

    /// Process files staged in the current Git repository
    #[arg(long, conflicts_with = "changed_since")]
    staged: bool,

    /// Process files changed since the merge base with REF, including untracked files
    #[arg(long, value_name = "REF")]
    changed_since: Option<String>,
}

impl Paths {
    fn git_selection(&self) -> Option<GitSelection> {
        if self.staged {
            Some(GitSelection::Staged)
        } else {
            self.changed_since
                .as_ref()
                .map(|reference| GitSelection::ChangedSince(reference.clone()))
        }
    }
}

#[derive(Debug, Args)]
struct CheckArgs {
    #[command(flatten)]
    paths: Paths,

    /// Print paths that would be reformatted
    #[arg(long)]
    list_different: bool,

    /// Print a deterministic check summary
    #[arg(long)]
    summary: bool,
}

#[derive(Debug, Args)]
struct ConfigArgs {
    #[command(subcommand)]
    command: ConfigCommand,
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    /// Print the resolved configuration path or <defaults>
    Path,
    /// Print the effective configuration as TOML
    Show,
}

#[derive(Debug, Args)]
struct InitArgs {
    /// Configuration file to create
    #[arg(value_name = "PATH", default_value = "semblock.toml")]
    path: PathBuf,

    /// Replace an existing configuration file
    #[arg(long)]
    force: bool,
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

        match &self.command {
            Command::Config(args) => return self.run_config(args),
            Command::Init(args) => return self.run_init(args),
            _ => {}
        }

        let (mode, paths, check_output) = match &self.command {
            Command::Fmt(paths) => (Mode::Fmt, paths.clone(), CheckOutput::default()),
            Command::Check(args) => (
                Mode::Check,
                args.paths.clone(),
                CheckOutput {
                    list_different: args.list_different,
                    summary: args.summary,
                },
            ),
            Command::Diff(paths) => (Mode::Diff, paths.clone(), CheckOutput::default()),
            Command::Config(_) | Command::Init(_) => unreachable!("handled above"),
        };
        let config = Config::load(self.config.as_deref()).map_err(RunError::config)?;

        if self.stdin {
            if !paths.paths.is_empty() || paths.git_selection().is_some() {
                return Err(RunError::usage(
                    "--stdin cannot be combined with filesystem or Git selection",
                ));
            }
            return self.run_stdin(mode, &config, check_output);
        }
        if self.filename.is_some() {
            return Err(RunError::usage("--filename requires --stdin"));
        }
        self.run_paths(mode, &paths, &config, check_output)
    }

    fn run_config(&self, args: &ConfigArgs) -> Result<ExitCode, RunError> {
        self.reject_source_options("config")?;
        let loaded = Config::load_resolved(self.config.as_deref()).map_err(RunError::config)?;
        match args.command {
            ConfigCommand::Path => match loaded.path {
                Some(path) => println!("{}", path.display()),
                None => println!("<defaults>"),
            },
            ConfigCommand::Show => {
                match loaded.path {
                    Some(path) => println!("# source: {}", path.display()),
                    None => println!("# source: built-in defaults"),
                }
                print!("{}", loaded.config.to_toml());
            }
        }
        Ok(ExitCode::SUCCESS)
    }

    fn run_init(&self, args: &InitArgs) -> Result<ExitCode, RunError> {
        self.reject_source_options("init")?;
        if self.config.is_some() {
            return Err(RunError::usage(
                "--config cannot be combined with init; pass the target path positionally",
            ));
        }
        let mut options = OpenOptions::new();
        options.write(true);
        if args.force {
            options.create(true).truncate(true);
        } else {
            options.create_new(true);
        }
        let mut file = options.open(&args.path).map_err(|error| {
            let message = if error.kind() == std::io::ErrorKind::AlreadyExists {
                format!(
                    "{} already exists; pass --force to replace it",
                    args.path.display()
                )
            } else {
                format!("failed to create {}: {error}", args.path.display())
            };
            RunError::filesystem(message)
        })?;
        file.write_all(Config::default().to_toml().as_bytes())
            .map_err(|error| {
                RunError::filesystem(format!("failed to write {}: {error}", args.path.display()))
            })?;
        if !self.quiet {
            eprintln!("Created: {}", args.path.display());
        }
        Ok(ExitCode::SUCCESS)
    }

    fn reject_source_options(&self, command: &str) -> Result<(), RunError> {
        if self.stdin || self.filename.is_some() || self.language != Language::Auto {
            return Err(RunError::usage(format!(
                "source input options cannot be combined with the {command} command"
            )));
        }
        Ok(())
    }

    fn run_stdin(
        self,
        mode: Mode,
        config: &Config,
        check_output: CheckOutput,
    ) -> Result<ExitCode, RunError> {
        let filename = match self.filename {
            Some(path) => path,
            None if self.language != Language::Auto => PathBuf::from("<stdin>"),
            None => {
                return Err(RunError::usage(
                    "--stdin requires --filename when --language is auto",
                ));
            }
        };
        let language = infer_language(&filename, self.language).map_err(RunError::source)?;
        let mut source = String::new();
        io::stdin()
            .read_to_string(&mut source)
            .map_err(|error| RunError::filesystem(format!("failed to read stdin: {error}")))?;
        let formatted = format_source(&source, language, &config.format, &config.go)
            .map_err(RunError::source)?;

        match mode {
            Mode::Fmt => {
                emit_diagnostics(&filename, &formatted, self.quiet, false);
                print!("{}", formatted.output);
                Ok(ExitCode::SUCCESS)
            }
            Mode::Check => {
                emit_diagnostics(&filename, &formatted, self.quiet, true);
                emit_check_path(&filename, &formatted, check_output, self.quiet);
                if check_output.summary {
                    emit_check_summary(std::slice::from_ref(&formatted));
                }
                Ok(if formatted.changed {
                    ExitCode::from(1)
                } else {
                    ExitCode::SUCCESS
                })
            }
            Mode::Diff => {
                emit_diagnostics(&filename, &formatted, self.quiet, false);
                if formatted.changed {
                    print!(
                        "{}",
                        diff::unified(&filename.display().to_string(), &source, &formatted.output)
                    );
                    Ok(ExitCode::from(1))
                } else {
                    Ok(ExitCode::SUCCESS)
                }
            }
        }
    }

    fn run_paths(
        self,
        mode: Mode,
        paths: &Paths,
        config: &Config,
        check_output: CheckOutput,
    ) -> Result<ExitCode, RunError> {
        let inputs: Vec<PlanInput> = if let Some(selection) = paths.git_selection() {
            let selected = select_files(&selection).map_err(RunError::git)?;
            let filtered = filter_candidates(
                &selected.root,
                &selected.paths,
                self.language,
                &config.discovery,
                &config.go,
                self.jobs,
            )
            .map_err(RunError::discovery)?;

            match selection {
                GitSelection::Staged => {
                    let staged =
                        read_staged_files(&selected.root, &filtered).map_err(RunError::git)?;
                    if mode == Mode::Fmt {
                        for file in &staged {
                            let worktree_path = selected.root.join(&file.path);
                            let worktree = std::fs::read(&worktree_path).map_err(|error| {
                                RunError::filesystem(format!(
                                    "cannot format staged path {} because its worktree file is unavailable: {error}",
                                    file.path.display()
                                ))
                            })?;
                            if worktree != file.source.as_bytes() {
                                return Err(RunError::filesystem(format!(
                                    "cannot format staged path {} because the Git index and worktree differ",
                                    file.path.display()
                                )));
                            }
                        }
                        staged
                            .into_iter()
                            .map(|file| PlanInput::Worktree(selected.root.join(file.path)))
                            .collect()
                    } else {
                        staged
                            .into_iter()
                            .map(|file| PlanInput::Provided {
                                path: selected.root.join(file.path),
                                source: file.source,
                            })
                            .collect()
                    }
                }
                GitSelection::ChangedSince(_) => filtered
                    .into_iter()
                    .map(|path| PlanInput::Worktree(selected.root.join(path)))
                    .collect(),
            }
        } else {
            let roots = if paths.paths.is_empty() {
                vec![PathBuf::from(".")]
            } else {
                paths.paths.clone()
            };
            discover(
                &roots,
                self.language,
                &config.discovery,
                &config.go,
                self.jobs,
            )
            .map_err(RunError::discovery)?
            .into_iter()
            .map(PlanInput::Worktree)
            .collect()
        };

        if self.verbose {
            for input in &inputs {
                eprintln!("Inspecting: {}", input.path().display());
            }
        }
        let plans = build_plans(inputs, self.language, config, self.jobs)?;
        let changed = plans.iter().filter(|plan| plan.formatted.changed).count();

        match mode {
            Mode::Fmt => {
                for plan in &plans {
                    emit_diagnostics(&plan.path, &plan.formatted, self.quiet, false);
                }
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
                for plan in &plans {
                    emit_diagnostics(&plan.path, &plan.formatted, self.quiet, true);
                    emit_check_path(&plan.path, &plan.formatted, check_output, self.quiet);
                }
                if check_output.summary {
                    emit_check_summary_refs(plans.iter().map(|plan| &plan.formatted));
                }
                Ok(if changed == 0 {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(1)
                })
            }
            Mode::Diff => {
                for plan in &plans {
                    emit_diagnostics(&plan.path, &plan.formatted, self.quiet, false);
                }
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

    pub(super) fn source_with_path(path: &Path, error: SourceError) -> Self {
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

    fn git(error: GitError) -> Self {
        Self::filesystem(error.to_string())
    }

    fn rewrite(error: RewriteError) -> Self {
        Self::filesystem(error.to_string())
    }

    pub(super) fn filesystem(message: impl Into<String>) -> Self {
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
