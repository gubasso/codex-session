//! Layered configuration.

pub(crate) mod error;

pub(crate) use error::ConfigError;

use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct Config {
    pub(crate) log: LogConfig,
    pub(crate) paths: PathsConfig,
    pub(crate) child: ChildConfig,
    #[serde(skip)]
    pub(crate) sources: ConfigSources,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct LogConfig {
    pub(crate) verbose: u8,
    pub(crate) mirror_stderr: bool,
    pub(crate) format: LogFormat,
    pub(crate) file: Option<Utf8PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct PathsConfig {
    pub(crate) base_config: Utf8PathBuf,
    pub(crate) target_config: Utf8PathBuf,
    pub(crate) cache_dir: Utf8PathBuf,
    pub(crate) state_dir: Utf8PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct ChildConfig {
    pub(crate) bin: Option<Utf8PathBuf>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum LogFormat {
    #[default]
    Json,
    Pretty,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ConfigSources {
    pub(crate) user: Option<Utf8PathBuf>,
    pub(crate) project: Option<Utf8PathBuf>,
    pub(crate) env_prefix: &'static str,
    pub(crate) cli: String,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct CliOverrides {
    pub(crate) config_file: Option<Utf8PathBuf>,
    pub(crate) values: CliValueOverrides,
}

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct CliValueOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) log: Option<LogOverrides>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) child: Option<ChildOverrides>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct LogOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) verbose: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) mirror_stderr: Option<bool>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct ChildOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) bin: Option<Utf8PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct FileConfig {
    log: Option<FileLogConfig>,
    paths: Option<FilePathsConfig>,
    child: Option<FileChildConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct FileLogConfig {
    verbose: Option<u8>,
    mirror_stderr: Option<bool>,
    format: Option<LogFormat>,
    file: Option<Utf8PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct FilePathsConfig {
    base_config: Option<Utf8PathBuf>,
    target_config: Option<Utf8PathBuf>,
    cache_dir: Option<Utf8PathBuf>,
    state_dir: Option<Utf8PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct FileChildConfig {
    bin: Option<Utf8PathBuf>,
}

impl CliOverrides {
    pub(crate) fn from_global(global: &crate::cli::GlobalArgs) -> Self {
        let log = LogOverrides {
            verbose: (global.verbose > 0).then_some(global.verbose),
            mirror_stderr: global.log_stderr.then_some(true),
        };
        let values = CliValueOverrides {
            log: (log.verbose.is_some() || log.mirror_stderr.is_some()).then_some(log),
            child: None,
        };

        Self {
            config_file: global.config.clone(),
            values,
        }
    }
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            verbose: 0,
            mirror_stderr: false,
            format: LogFormat::Json,
            file: None,
        }
    }
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            base_config: Utf8PathBuf::new(),
            target_config: Utf8PathBuf::new(),
            cache_dir: Utf8PathBuf::new(),
            state_dir: Utf8PathBuf::new(),
        }
    }
}

impl PathsConfig {
    pub(crate) fn stamp_file(&self) -> Utf8PathBuf {
        self.cache_dir.join("last-merge")
    }
}

impl Config {
    fn defaults() -> Result<Self, ConfigError> {
        let Some(base_dirs) = directories::BaseDirs::new() else {
            return Err(ConfigError::NoXdg);
        };
        let Some(project_dirs) = directories::ProjectDirs::from("", "", "codex-session") else {
            return Err(ConfigError::NoXdg);
        };

        let home_dir = Utf8PathBuf::try_from(base_dirs.home_dir().to_path_buf())?;
        let cache_dir = Utf8PathBuf::try_from(project_dirs.cache_dir().to_path_buf())?;
        let state_dir = Utf8PathBuf::try_from(
            project_dirs
                .state_dir()
                .unwrap_or_else(|| project_dirs.data_local_dir())
                .to_path_buf(),
        )?;

        let paths = PathsConfig {
            base_config: home_dir.join(".codex/config.base.toml"),
            target_config: home_dir.join(".codex/config.toml"),
            cache_dir,
            state_dir: state_dir.clone(),
        };

        Ok(Self {
            log: LogConfig {
                file: Some(state_dir.join("codex-session.log")),
                ..LogConfig::default()
            },
            paths,
            child: ChildConfig::default(),
            sources: ConfigSources::default(),
        })
    }

    pub(crate) fn load(cli: &CliOverrides) -> Result<Self, ConfigError> {
        let mut config = Self::defaults()?;
        let explicit = cli.config_file.clone();
        let user = if explicit.is_none() {
            user_config_path()?
        } else {
            None
        };
        let project = if explicit.is_none() {
            find_project_config(std::env::current_dir().map_err(ConfigError::CurrentDir)?)?
        } else {
            None
        };

        if let Some(path) = explicit.as_ref() {
            if !path.is_file() {
                return Err(ConfigError::ExplicitConfigMissing(path.clone()));
            }
            apply_file_layer(&mut config, path)?;
        } else {
            if let Some(path) = user.as_ref() {
                apply_file_layer(&mut config, path)?;
            }
            if let Some(path) = project.as_ref() {
                apply_file_layer(&mut config, path)?;
            }
        }

        apply_env_layer(&mut config)?;
        apply_cli_overrides(&mut config, &cli.values);

        config.sources = ConfigSources {
            user: explicit.or(user),
            project,
            env_prefix: "CODEX_SESSION_*",
            cli: format!("{:?}", cli.values),
        };
        Ok(config)
    }
}

fn apply_file_layer(config: &mut Config, path: &Utf8PathBuf) -> Result<(), ConfigError> {
    let contents = std::fs::read_to_string(path)?;
    let parsed = toml::from_str::<FileConfig>(&contents).map_err(|source| {
        extract_unknown_key(&source.to_string()).map_or_else(
            || ConfigError::Parse {
                path: path.clone(),
                source,
            },
            |key| ConfigError::UnknownKey {
                key,
                path: path.clone(),
            },
        )
    })?;
    apply_file_config(config, parsed);
    Ok(())
}

fn apply_file_config(config: &mut Config, layer: FileConfig) {
    if let Some(log) = layer.log {
        if let Some(verbose) = log.verbose {
            config.log.verbose = verbose;
        }
        if let Some(mirror_stderr) = log.mirror_stderr {
            config.log.mirror_stderr = mirror_stderr;
        }
        if let Some(format) = log.format {
            config.log.format = format;
        }
        if let Some(file) = log.file {
            config.log.file = Some(file);
        }
    }

    if let Some(paths) = layer.paths {
        if let Some(base_config) = paths.base_config {
            config.paths.base_config = base_config;
        }
        if let Some(target_config) = paths.target_config {
            config.paths.target_config = target_config;
        }
        if let Some(cache_dir) = paths.cache_dir {
            config.paths.cache_dir = cache_dir;
        }
        if let Some(state_dir) = paths.state_dir {
            config.paths.state_dir = state_dir;
        }
    }

    if let Some(child) = layer.child {
        if let Some(bin) = child.bin {
            config.child.bin = Some(bin);
        }
    }
}

fn apply_env_layer(config: &mut Config) -> Result<(), ConfigError> {
    for (key, value) in std::env::vars_os() {
        let Some(key) = key.to_str() else { continue };
        if !key.starts_with("CODEX_SESSION_") {
            continue;
        }
        let Some(value) = value.to_str() else {
            continue;
        };
        // Accept both flat (`LOG_VERBOSE`) and double-underscore-nested
        // (`LOG__VERBOSE`) leaf names. The flat form matches user intuition
        // and the bash wrapper's historical contract; the nested form
        // matches the canonical figment convention (cli-design
        // `03-config-precedence.md`). Either works.
        let canonical = key["CODEX_SESSION_".len()..].replace("__", "_");
        match canonical.as_str() {
            "CHILD_BIN" => {
                config.child.bin = Some(Utf8PathBuf::from(value));
            }
            "LOG_FILE" => {
                config.log.file = Some(Utf8PathBuf::from(value));
            }
            // `LOG_DIR` is back-compat with the bash wrapper. It picks the
            // directory and appends `codex-session.log`. `LOG_FILE` always
            // wins when both are set.
            "LOG_DIR" if std::env::var_os("CODEX_SESSION_LOG_FILE").is_none() => {
                config.log.file = Some(Utf8PathBuf::from(value).join("codex-session.log"));
            }
            "LOG_VERBOSE" => {
                config.log.verbose =
                    parse_env_value::<u8>(key, value).map_err(ConfigError::from)?;
            }
            "LOG_MIRROR_STDERR" => {
                config.log.mirror_stderr =
                    parse_env_value::<bool>(key, value).map_err(ConfigError::from)?;
            }
            "LOG_FORMAT" => {
                config.log.format = parse_log_format(key, value)?;
            }
            "PATHS_BASE_CONFIG" => {
                config.paths.base_config = Utf8PathBuf::from(value);
            }
            "PATHS_TARGET_CONFIG" => {
                config.paths.target_config = Utf8PathBuf::from(value);
            }
            "PATHS_CACHE_DIR" => {
                config.paths.cache_dir = Utf8PathBuf::from(value);
            }
            "PATHS_STATE_DIR" => {
                config.paths.state_dir = Utf8PathBuf::from(value);
            }
            _ => {}
        }
    }
    Ok(())
}

fn apply_cli_overrides(config: &mut Config, cli: &CliValueOverrides) {
    if let Some(log) = cli.log.as_ref() {
        if let Some(verbose) = log.verbose {
            config.log.verbose = verbose;
        }
        if let Some(mirror_stderr) = log.mirror_stderr {
            config.log.mirror_stderr = mirror_stderr;
        }
    }

    if let Some(child) = cli.child.as_ref() {
        if let Some(bin) = child.bin.as_ref() {
            config.child.bin = Some(bin.clone());
        }
    }
}

fn user_config_path() -> Result<Option<Utf8PathBuf>, ConfigError> {
    let Some(dirs) = directories::ProjectDirs::from("", "", "codex-session") else {
        return Err(ConfigError::NoXdg);
    };
    let path = Utf8PathBuf::try_from(dirs.config_dir().join("config.toml"))?;
    Ok(path.is_file().then_some(path))
}

fn find_project_config(start: std::path::PathBuf) -> Result<Option<Utf8PathBuf>, ConfigError> {
    let mut here = start;
    loop {
        let candidate = here.join(".codex-session").join("config.toml");
        if candidate.is_file() {
            return Utf8PathBuf::try_from(candidate)
                .map(Some)
                .map_err(Into::into);
        }
        if !here.pop() {
            break;
        }
    }
    Ok(None)
}

fn extract_unknown_key(message: &str) -> Option<String> {
    let needle = "unknown field `";
    let start = message.find(needle)? + needle.len();
    let rest = &message[start..];
    let end = rest.find('`')?;
    Some(rest[..end].to_owned())
}

fn parse_log_format(key: &str, value: &str) -> Result<LogFormat, ConfigError> {
    match value {
        "json" => Ok(LogFormat::Json),
        "pretty" => Ok(LogFormat::Pretty),
        other => Err(ConfigError::EnvParse {
            key: key.to_owned(),
            value: other.to_owned(),
            expected: "json or pretty",
        }),
    }
}

fn parse_env_value<T>(key: &str, value: &str) -> Result<T, crate::config::error::EnvParseError>
where
    T: std::str::FromStr,
{
    value
        .parse::<T>()
        .map_err(|_| crate::config::error::EnvParseError {
            key: key.to_owned(),
            value: value.to_owned(),
            expected: std::any::type_name::<T>(),
        })
}
