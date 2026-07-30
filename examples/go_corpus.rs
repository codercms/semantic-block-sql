use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use semblock::FormatOptions;
use semblock::config::Config;
use semblock::host::go::{GoFormatStats, format_go_source};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct ProjectSpec {
    name: String,
    repository: String,
    revision: String,
    license: String,
    license_path: String,
    #[serde(default)]
    include_paths: Vec<String>,
    test_command: Vec<String>,
    #[serde(default)]
    environment: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
struct ProjectReport {
    name: String,
    repository: String,
    requested_revision: String,
    resolved_revision: String,
    license: String,
    go_files: usize,
    changed_files: usize,
    discovered_expressions: usize,
    eligible_candidates: usize,
    formatted_expressions: usize,
    unchanged_sql_expressions: usize,
    unsupported_expressions: usize,
    potential_false_positive_skips: usize,
    dynamic_expressions: usize,
    diagnostics: usize,
    gofmt_clean: bool,
    idempotent: bool,
    tests_passed: bool,
}

#[derive(Debug)]
struct Args {
    manifest: PathBuf,
    work_dir: PathBuf,
    report: Option<PathBuf>,
    keep: bool,
}

fn main() {
    if let Err(error) = run() {
        eprintln!("go corpus failed: {error}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let specs: Vec<ProjectSpec> = serde_json::from_str(
        &fs::read_to_string(&args.manifest)
            .map_err(|error| format!("read {}: {error}", args.manifest.display()))?,
    )
    .map_err(|error| format!("parse {}: {error}", args.manifest.display()))?;
    validate_specs(&specs)?;

    fs::create_dir_all(&args.work_dir)
        .map_err(|error| format!("create {}: {error}", args.work_dir.display()))?;
    let mut reports = Vec::new();
    let mut failed = false;
    for spec in specs {
        let report = run_project(&args, &spec)?;
        failed |= !(report.gofmt_clean && report.idempotent && report.tests_passed);
        eprintln!(
            "{}: files={} candidates={} formatted={} unsupported={} parse-skips={} dynamic={} tests={}",
            report.name,
            report.go_files,
            report.eligible_candidates,
            report.formatted_expressions,
            report.unsupported_expressions,
            report.potential_false_positive_skips,
            report.dynamic_expressions,
            if report.tests_passed { "pass" } else { "fail" }
        );
        reports.push(report);
    }

    let json = serde_json::to_string_pretty(&reports)
        .map_err(|error| format!("serialize report: {error}"))?;
    if let Some(path) = args.report {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("create {}: {error}", parent.display()))?;
        }
        fs::write(&path, format!("{json}\n"))
            .map_err(|error| format!("write {}: {error}", path.display()))?;
    } else {
        println!("{json}");
    }
    if failed {
        return Err("one or more corpus projects failed validation".into());
    }
    Ok(())
}

fn parse_args() -> Result<Args, String> {
    let mut manifest = PathBuf::from("tests/corpus/go-projects.json");
    let mut work_dir = PathBuf::from("target/go-corpus");
    let mut report = None;
    let mut keep = false;
    let mut values = env::args_os().skip(1);
    while let Some(argument) = values.next() {
        match argument.to_str() {
            Some("--manifest") => manifest = PathBuf::from(next_value(&mut values, "--manifest")?),
            Some("--work-dir") => work_dir = PathBuf::from(next_value(&mut values, "--work-dir")?),
            Some("--report") => report = Some(PathBuf::from(next_value(&mut values, "--report")?)),
            Some("--keep") => keep = true,
            Some("--help") | Some("-h") => {
                println!(
                    "cargo run --locked --example go_corpus -- [--manifest PATH] [--work-dir PATH] [--report PATH] [--keep]"
                );
                std::process::exit(0);
            }
            _ => return Err(format!("unknown argument {}", argument.to_string_lossy())),
        }
    }
    Ok(Args {
        manifest,
        work_dir,
        report,
        keep,
    })
}

fn next_value(
    values: &mut impl Iterator<Item = std::ffi::OsString>,
    option: &str,
) -> Result<std::ffi::OsString, String> {
    values
        .next()
        .ok_or_else(|| format!("{option} requires a value"))
}

fn validate_specs(specs: &[ProjectSpec]) -> Result<(), String> {
    if specs.is_empty() {
        return Err("corpus manifest is empty".into());
    }
    for spec in specs {
        if spec.name.trim().is_empty()
            || spec.repository.trim().is_empty()
            || spec.revision.trim().is_empty()
            || spec.test_command.is_empty()
        {
            return Err(format!("incomplete corpus entry: {}", spec.name));
        }
        if matches!(spec.revision.as_str(), "main" | "master" | "HEAD") {
            return Err(format!("{} uses a mutable revision", spec.name));
        }
    }
    Ok(())
}

fn run_project(args: &Args, spec: &ProjectSpec) -> Result<ProjectReport, String> {
    let root = args.work_dir.join(&spec.name);
    if root.exists() {
        fs::remove_dir_all(&root).map_err(|error| format!("remove {}: {error}", root.display()))?;
    }
    command(
        Command::new("git")
            .args([
                "clone",
                "--quiet",
                "--depth",
                "1",
                "--branch",
                &spec.revision,
            ])
            .arg(&spec.repository)
            .arg(&root),
        "clone corpus project",
    )?;
    let resolved_revision = stdout(command(
        Command::new("git")
            .current_dir(&root)
            .args(["rev-parse", "HEAD"]),
        "resolve corpus revision",
    )?);
    if !root.join(&spec.license_path).is_file() {
        return Err(format!(
            "{} does not contain declared license file {}",
            spec.name, spec.license_path
        ));
    }

    let files = tracked_go_files(&root, &spec.include_paths)?;
    let options = FormatOptions::default();
    let go = Config::default().go;
    let mut totals = GoFormatStats::default();
    let mut changed = Vec::new();
    let mut diagnostics = 0usize;
    for relative in &files {
        let path = root.join(relative);
        let source = fs::read_to_string(&path)
            .map_err(|error| format!("read {}: {error}", path.display()))?;
        let formatted = format_go_source(&source, &options, &go)
            .map_err(|error| format!("format {}: {error}", path.display()))?;
        add_stats(&mut totals, formatted.stats);
        diagnostics += formatted.diagnostics.len();
        if formatted.output != source {
            fs::write(&path, formatted.output)
                .map_err(|error| format!("write {}: {error}", path.display()))?;
            changed.push(relative.clone());
        }
    }

    for relative in &changed {
        command(
            Command::new("gofmt").arg("-w").arg(root.join(relative)),
            "gofmt corpus output",
        )?;
    }
    let gofmt_clean = changed.iter().all(|relative| {
        command(
            Command::new("gofmt").arg("-l").arg(root.join(relative)),
            "check gofmt output",
        )
        .map(|output| output.stdout.is_empty())
        .unwrap_or(false)
    });

    let mut idempotent = true;
    for relative in &files {
        let path = root.join(relative);
        let source = fs::read_to_string(&path)
            .map_err(|error| format!("read {}: {error}", path.display()))?;
        let formatted = format_go_source(&source, &options, &go)
            .map_err(|error| format!("reformat {}: {error}", path.display()))?;
        idempotent &= formatted.output == source;
    }

    let mut test = Command::new(&spec.test_command[0]);
    test.current_dir(&root).args(&spec.test_command[1..]);
    test.envs(&spec.environment);
    let test_output = test
        .output()
        .map_err(|error| format!("test corpus project: {error}"))?;
    let tests_passed = test_output.status.success();
    if !tests_passed {
        eprintln!(
            "{} tests failed ({}):\n{}{}",
            spec.name,
            test_output.status,
            String::from_utf8_lossy(&test_output.stdout),
            String::from_utf8_lossy(&test_output.stderr)
        );
    }

    let report = ProjectReport {
        name: spec.name.clone(),
        repository: spec.repository.clone(),
        requested_revision: spec.revision.clone(),
        resolved_revision,
        license: spec.license.clone(),
        go_files: files.len(),
        changed_files: changed.len(),
        discovered_expressions: totals.discovered_expressions,
        eligible_candidates: totals.eligible_candidates,
        formatted_expressions: totals.formatted_expressions,
        unchanged_sql_expressions: totals.unchanged_sql_expressions,
        unsupported_expressions: totals.unsupported_expressions,
        potential_false_positive_skips: totals.auto_parse_skips,
        dynamic_expressions: totals.dynamic_expressions,
        diagnostics,
        gofmt_clean,
        idempotent,
        tests_passed,
    };
    if !args.keep {
        fs::remove_dir_all(&root).map_err(|error| format!("remove {}: {error}", root.display()))?;
    }
    Ok(report)
}

fn tracked_go_files(root: &Path, include_paths: &[String]) -> Result<Vec<PathBuf>, String> {
    let output = command(
        Command::new("git")
            .current_dir(root)
            .args(["ls-files", "-z", "--", "*.go"]),
        "list tracked Go files",
    )?;
    let mut files = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .map(|path| PathBuf::from(String::from_utf8_lossy(path).into_owned()))
        .filter(|path| {
            include_paths.is_empty()
                || include_paths.iter().any(|prefix| {
                    path.to_string_lossy()
                        .replace('\\', "/")
                        .starts_with(prefix)
                })
        })
        .collect::<Vec<_>>();
    files.sort();
    Ok(files)
}

fn add_stats(total: &mut GoFormatStats, value: GoFormatStats) {
    total.discovered_expressions += value.discovered_expressions;
    total.eligible_candidates += value.eligible_candidates;
    total.formatted_expressions += value.formatted_expressions;
    total.unchanged_sql_expressions += value.unchanged_sql_expressions;
    total.unsupported_expressions += value.unsupported_expressions;
    total.auto_parse_skips += value.auto_parse_skips;
    total.dynamic_expressions += value.dynamic_expressions;
}

fn command(command: &mut Command, description: &str) -> Result<Output, String> {
    command
        .output()
        .map_err(|error| format!("{description}: {error}"))
        .and_then(|output| {
            if output.status.success() {
                Ok(output)
            } else {
                Err(format!(
                    "{description} failed ({}):\n{}{}",
                    output.status,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                ))
            }
        })
}

fn stdout(output: Output) -> String {
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}
