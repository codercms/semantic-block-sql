use std::env;
use std::fs;
use std::path::{Component, Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

use crate::{FormatOptions, NotEqualPolicy, SemicolonPolicy, SyntaxDiagnostics};

const DEFAULT_IGNORE_FILE: &str = ".semblockignore";

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Config {
    pub format: FormatOptions,
    pub discovery: DiscoveryConfig,
    pub go: GoConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveryConfig {
    pub respect_gitignore: bool,
    pub ignore_file: String,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            respect_gitignore: true,
            ignore_file: DEFAULT_IGNORE_FILE.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoConfig {
    pub enabled: bool,
    pub auto_detect: bool,
    pub raw_strings: bool,
    pub interpreted_strings: bool,
}

impl Default for GoConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            auto_detect: true,
            raw_strings: true,
            interpreted_strings: false,
        }
    }
}

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("failed to determine current directory: {0}")]
    CurrentDirectory(#[source] std::io::Error),
    #[error("failed to read config {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid config {path}: {message}")]
    Parse { path: PathBuf, message: String },
    #[error("invalid config: {0}")]
    Invalid(String),
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    dialect: Option<String>,
    #[serde(default)]
    format: FileFormatConfig,
    #[serde(default)]
    layout: LayoutConfig,
    #[serde(default)]
    discovery: FileDiscoveryConfig,
    #[serde(default)]
    go: FileGoConfig,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileFormatConfig {
    semicolon_policy: Option<SemicolonPolicy>,
    not_equal_policy: Option<NotEqualPolicy>,
    syntax_diagnostics: Option<SyntaxDiagnostics>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct LayoutConfig {
    soft_line_width: Option<usize>,
    hard_line_width: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileDiscoveryConfig {
    respect_gitignore: Option<bool>,
    ignore_file: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileGoConfig {
    enabled: Option<bool>,
    auto_detect: Option<bool>,
    raw_strings: Option<bool>,
    interpreted_strings: Option<bool>,
}

impl Config {
    pub fn load(explicit: Option<&Path>) -> Result<Self, ConfigError> {
        let path = match explicit {
            Some(path) => Some(path.to_path_buf()),
            None => find_default_config()?,
        };
        let Some(path) = path else {
            return Ok(Self::default());
        };

        let source = fs::read_to_string(&path).map_err(|source| ConfigError::Read {
            path: path.clone(),
            source,
        })?;
        let file: FileConfig = toml::from_str(&source).map_err(|error| ConfigError::Parse {
            path: path.clone(),
            message: error.to_string(),
        })?;
        Self::from_file(file)
    }

    fn from_file(file: FileConfig) -> Result<Self, ConfigError> {
        if file.dialect.as_deref().unwrap_or("postgresql") != "postgresql" {
            return Err(ConfigError::Invalid(
                "dialect must be \"postgresql\"".into(),
            ));
        }

        let mut config = Self::default();
        if let Some(value) = file.format.semicolon_policy {
            config.format.semicolon_policy = value;
        }
        if let Some(value) = file.format.not_equal_policy {
            config.format.not_equal_policy = value;
        }
        if let Some(value) = file.format.syntax_diagnostics {
            config.format.syntax_diagnostics = value;
        }
        if let Some(value) = file.layout.soft_line_width {
            config.format.soft_line_width = value;
        }
        if let Some(value) = file.layout.hard_line_width {
            config.format.hard_line_width = value;
        }
        if let Some(value) = file.discovery.respect_gitignore {
            config.discovery.respect_gitignore = value;
        }
        if let Some(value) = file.discovery.ignore_file {
            if value.trim().is_empty()
                || !matches!(
                    Path::new(&value)
                        .components()
                        .collect::<Vec<_>>()
                        .as_slice(),
                    [Component::Normal(_)]
                )
            {
                return Err(ConfigError::Invalid(
                    "discovery.ignore_file must be a non-empty file name".into(),
                ));
            }
            config.discovery.ignore_file = value;
        }
        if let Some(value) = file.go.enabled {
            config.go.enabled = value;
        }
        if let Some(value) = file.go.auto_detect {
            config.go.auto_detect = value;
        }
        if let Some(value) = file.go.raw_strings {
            config.go.raw_strings = value;
        }
        if let Some(value) = file.go.interpreted_strings {
            config.go.interpreted_strings = value;
        }

        if config.format.soft_line_width == 0 {
            return Err(ConfigError::Invalid(
                "layout.soft_line_width must be greater than zero".into(),
            ));
        }
        if config.format.hard_line_width < config.format.soft_line_width {
            return Err(ConfigError::Invalid(
                "layout.hard_line_width must be greater than or equal to soft_line_width".into(),
            ));
        }
        if config.go.interpreted_strings {
            return Err(ConfigError::Invalid(
                "go.interpreted_strings is not supported by the MVP".into(),
            ));
        }
        Ok(config)
    }
}

fn find_default_config() -> Result<Option<PathBuf>, ConfigError> {
    let mut directory = env::current_dir().map_err(ConfigError::CurrentDirectory)?;
    loop {
        let candidate = directory.join("semblock.toml");
        if candidate.is_file() {
            return Ok(Some(candidate));
        }
        if !directory.pop() {
            return Ok(None);
        }
    }
}
