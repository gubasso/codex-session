//! Layered configuration.
//!
//! What this is: the wrapper's layered configuration schema and loader.
//! What this is not: command dispatch or user-facing rendering.

#![allow(clippy::result_large_err)] // Phase 08 keeps full figment provenance on ConfigError.

pub(crate) mod error;

pub(crate) use error::ConfigError;

use camino::Utf8PathBuf;
use clap::ValueEnum;
use figment::{Figment, providers::Format as _};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
#[allow(clippy::struct_field_names)]
pub(crate) struct Config {
    pub(crate) log: LogConfig,
    pub(crate) paths: PathsConfig,
    pub(crate) child: ChildConfig,
    #[serde(rename = "config-recipe")]
    pub(crate) config_recipe: ConfigRecipeConfig,
    pub(crate) account: AccountConfig,
    #[serde(skip)]
    pub(crate) sources: ConfigSources,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct LogConfig {
    pub(crate) verbose: u8,
    pub(crate) mirror_stderr: bool,
    pub(crate) format: LogFormat,
    pub(crate) stderr_format: Option<LogFormat>,
    /// Directory hint for the rotating log files.
    pub(crate) file: Option<Utf8PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
#[allow(clippy::struct_field_names)]
pub(crate) struct PathsConfig {
    pub(crate) runtime_dir: Option<Utf8PathBuf>,
    pub(crate) cache_dir: Utf8PathBuf,
    pub(crate) state_dir: Utf8PathBuf,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct ChildConfig {
    pub(crate) bin: Option<Utf8PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct ConfigRecipeConfig {
    pub(crate) default: Option<String>,
    pub(crate) config_dir: Utf8PathBuf,
    pub(crate) recipes_dir: Utf8PathBuf,
    pub(crate) configs_dir: Utf8PathBuf,
    #[serde(skip)]
    pub(crate) active: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct AccountConfig {
    pub(crate) pinned: Option<crate::services::account::AccountId>,
    pub(crate) registry_dir: Option<Utf8PathBuf>,
    pub(crate) quota_ttl_secs: u64,
    pub(crate) weekly_floor: f64,
    pub(crate) five_hour_threshold: f64,
    pub(crate) five_hour_weight: f64,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq, ValueEnum)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) config_recipe: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct LogOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) verbose: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) mirror_stderr: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) stderr_format: Option<LogFormat>,
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
    #[serde(rename = "config-recipe")]
    config_recipe: Option<FileConfigRecipeConfig>,
    account: Option<FileAccountConfig>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct FileLogConfig {
    verbose: Option<u8>,
    mirror_stderr: Option<bool>,
    format: Option<LogFormat>,
    stderr_format: Option<LogFormat>,
    file: Option<Utf8PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
#[allow(clippy::struct_field_names)]
struct FilePathsConfig {
    runtime_dir: Option<Utf8PathBuf>,
    cache_dir: Option<Utf8PathBuf>,
    state_dir: Option<Utf8PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct FileChildConfig {
    bin: Option<Utf8PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct FileConfigRecipeConfig {
    default: Option<String>,
    config_dir: Option<Utf8PathBuf>,
    recipes_dir: Option<Utf8PathBuf>,
    configs_dir: Option<Utf8PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(deny_unknown_fields, default)]
struct FileAccountConfig {
    pinned: Option<String>,
    registry_dir: Option<Utf8PathBuf>,
    quota_ttl_secs: Option<u64>,
    weekly_floor: Option<f64>,
    five_hour_threshold: Option<f64>,
    five_hour_weight: Option<f64>,
}

impl CliOverrides {
    pub(crate) fn from_global(global: &crate::cli::GlobalArgs) -> Self {
        let log = LogOverrides {
            verbose: (global.verbose > 0).then_some(global.verbose),
            mirror_stderr: global.log_stderr.then_some(true),
            stderr_format: global.log_format,
        };
        let values = CliValueOverrides {
            log: (log.verbose.is_some()
                || log.mirror_stderr.is_some()
                || log.stderr_format.is_some())
            .then_some(log),
            child: None,
            config_recipe: global.config_recipe.clone(),
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
            stderr_format: None,
            file: None,
        }
    }
}

impl Default for PathsConfig {
    fn default() -> Self {
        Self {
            runtime_dir: None,
            cache_dir: Utf8PathBuf::new(),
            state_dir: Utf8PathBuf::new(),
        }
    }
}

impl Default for ConfigRecipeConfig {
    fn default() -> Self {
        Self {
            default: None,
            config_dir: Utf8PathBuf::new(),
            recipes_dir: Utf8PathBuf::new(),
            configs_dir: Utf8PathBuf::new(),
            active: None,
        }
    }
}

impl Default for AccountConfig {
    fn default() -> Self {
        Self {
            pinned: None,
            registry_dir: None,
            quota_ttl_secs: 30,
            weekly_floor: 10.0,
            five_hour_threshold: 50.0,
            five_hour_weight: 0.70,
        }
    }
}

impl Config {
    fn defaults() -> Result<Self, ConfigError> {
        let Some(project_dirs) = directories::ProjectDirs::from("", "", "codex-session") else {
            return Err(ConfigError::NoXdg);
        };
        let cache_dir = Utf8PathBuf::try_from(project_dirs.cache_dir().to_path_buf())?;
        let config_dir = Utf8PathBuf::try_from(project_dirs.config_dir().to_path_buf())?;
        let state_dir = Utf8PathBuf::try_from(
            project_dirs
                .state_dir()
                .unwrap_or_else(|| project_dirs.data_local_dir())
                .to_path_buf(),
        )?;

        let paths = PathsConfig {
            runtime_dir: std::env::var("XDG_RUNTIME_DIR")
                .ok()
                .map(Utf8PathBuf::from)
                .map(|path| path.join("codex-session")),
            cache_dir,
            state_dir,
        };

        let config_recipe = ConfigRecipeConfig {
            default: None,
            recipes_dir: config_dir.join("config-recipes"),
            configs_dir: config_dir.join("configs"),
            config_dir,
            active: None,
        };

        Ok(Self {
            log: LogConfig::default(),
            paths,
            child: ChildConfig::default(),
            config_recipe,
            account: AccountConfig::default(),
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
        config.config_recipe.active = resolve_active_config_recipe(&config, &cli.values);
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

pub(crate) fn resolve_home_dir() -> Result<Utf8PathBuf, ConfigError> {
    let Some(base_dirs) = directories::BaseDirs::new() else {
        return Err(ConfigError::NoHomeDir);
    };
    Utf8PathBuf::try_from(base_dirs.home_dir().to_path_buf()).map_err(ConfigError::from)
}

fn apply_file_layer(config: &mut Config, path: &Utf8PathBuf) -> Result<(), ConfigError> {
    let parsed = Figment::from(figment::providers::Toml::file_exact(path.as_std_path()))
        .extract::<FileConfig>()
        .map_err(|source| map_figment_file_error(path, source))?;
    apply_file_config(config, parsed)
}

fn apply_file_config(config: &mut Config, layer: FileConfig) -> Result<(), ConfigError> {
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
        if let Some(stderr_format) = log.stderr_format {
            config.log.stderr_format = Some(stderr_format);
        }
        if let Some(file) = log.file {
            config.log.file = Some(file);
        }
    }

    if let Some(paths) = layer.paths {
        if let Some(runtime_dir) = paths.runtime_dir {
            config.paths.runtime_dir = Some(runtime_dir);
        }
        if let Some(cache_dir) = paths.cache_dir {
            config.paths.cache_dir = cache_dir;
        }
        if let Some(state_dir) = paths.state_dir {
            config.paths.state_dir = state_dir;
        }
    }

    if let Some(child) = layer.child
        && let Some(bin) = child.bin
    {
        config.child.bin = Some(bin);
    }

    if let Some(config_recipe) = layer.config_recipe {
        if let Some(default) = config_recipe.default {
            config.config_recipe.default = Some(default);
        }
        // When a layer sets `config_dir`, derive `recipes_dir` /
        // `configs_dir` from it unless the same layer also overrides them
        // explicitly. This keeps the documented invariant that pointing
        // `config_dir` at a fresh tree re-roots the whole config-recipe lookup.
        if let Some(config_dir) = config_recipe.config_dir {
            if config_recipe.recipes_dir.is_none() {
                config.config_recipe.recipes_dir = config_dir.join("config-recipes");
            }
            if config_recipe.configs_dir.is_none() {
                config.config_recipe.configs_dir = config_dir.join("configs");
            }
            config.config_recipe.config_dir = config_dir;
        }
        if let Some(recipes_dir) = config_recipe.recipes_dir {
            config.config_recipe.recipes_dir = recipes_dir;
        }
        if let Some(configs_dir) = config_recipe.configs_dir {
            config.config_recipe.configs_dir = configs_dir;
        }
    }

    if let Some(account) = layer.account {
        if let Some(value) = account.pinned {
            config.account.pinned = Some(parse_account_id_field("account.pinned", value)?);
        }
        if let Some(dir) = account.registry_dir {
            config.account.registry_dir = Some(dir);
        }
        if let Some(value) = account.quota_ttl_secs {
            config.account.quota_ttl_secs = value;
        }
        if let Some(value) = account.weekly_floor {
            config.account.weekly_floor = value;
        }
        if let Some(value) = account.five_hour_threshold {
            config.account.five_hour_threshold = value;
        }
        if let Some(value) = account.five_hour_weight {
            config.account.five_hour_weight = value;
        }
    }

    Ok(())
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
        // Keep env parsing manual here instead of delegating to
        // `figment::providers::Env`: the wrapper now owns a `config-recipe`
        // namespace, and we do not want to engage figment's separate
        // config-recipe-key machinery.
        //
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
            // `LOG_DIR` is back-compat with the bash wrapper. Both vars now
            // behave as directory hints because the appender owns the basename.
            "LOG_DIR" if std::env::var_os("CODEX_SESSION_LOG_FILE").is_none() => {
                config.log.file = Some(Utf8PathBuf::from(value));
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
            "LOG_STDERR_FORMAT" => {
                config.log.stderr_format = Some(parse_log_format(key, value)?);
            }
            "PATHS_CACHE_DIR" => {
                config.paths.cache_dir = Utf8PathBuf::from(value);
            }
            "PATHS_STATE_DIR" => {
                config.paths.state_dir = Utf8PathBuf::from(value);
            }
            "PATHS_RUNTIME_DIR" => {
                config.paths.runtime_dir = Some(Utf8PathBuf::from(value));
            }
            "CONFIG_RECIPE" => {
                config.config_recipe.active = Some(value.to_owned());
            }
            "ACCOUNT_PINNED" => {
                config.account.pinned = Some(parse_account_id_env(key, value)?);
            }
            "ACCOUNT_REGISTRY_DIR" => {
                config.account.registry_dir = Some(Utf8PathBuf::from(value));
            }
            "ACCOUNT_QUOTA_TTL_SECS" => {
                config.account.quota_ttl_secs =
                    parse_env_value::<u64>(key, value).map_err(ConfigError::from)?;
            }
            "ACCOUNT_WEEKLY_FLOOR" => {
                config.account.weekly_floor =
                    parse_env_value::<f64>(key, value).map_err(ConfigError::from)?;
            }
            "ACCOUNT_FIVE_HOUR_THRESHOLD" => {
                config.account.five_hour_threshold =
                    parse_env_value::<f64>(key, value).map_err(ConfigError::from)?;
            }
            "ACCOUNT_FIVE_HOUR_WEIGHT" => {
                config.account.five_hour_weight =
                    parse_env_value::<f64>(key, value).map_err(ConfigError::from)?;
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
        if let Some(stderr_format) = log.stderr_format {
            config.log.stderr_format = Some(stderr_format);
        }
    }

    if let Some(child) = cli.child.as_ref()
        && let Some(bin) = child.bin.as_ref()
    {
        config.child.bin = Some(bin.clone());
    }

    if let Some(config_recipe) = cli.config_recipe.as_ref() {
        config.config_recipe.active = Some(config_recipe.clone());
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

fn map_figment_file_error(path: &Utf8PathBuf, source: figment::Error) -> ConfigError {
    match &source.kind {
        figment::error::Kind::UnknownField(key, _) => ConfigError::UnknownKey {
            key: key.clone(),
            path: path.clone(),
        },
        _ => ConfigError::Parse {
            path: path.clone(),
            source,
        },
    }
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

fn parse_account_id_field(
    field: &'static str,
    value: String,
) -> Result<crate::services::account::AccountId, ConfigError> {
    value
        .parse::<crate::services::account::AccountId>()
        .map_err(|reason| ConfigError::AccountConfigParse {
            field,
            value,
            reason,
        })
}

fn parse_account_id_env(
    key: &str,
    value: &str,
) -> Result<crate::services::account::AccountId, ConfigError> {
    value
        .parse::<crate::services::account::AccountId>()
        .map_err(|_| ConfigError::EnvParse {
            key: key.to_owned(),
            value: value.to_owned(),
            expected: "account name matching [a-z0-9][a-z0-9_-]{0,31}",
        })
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

fn resolve_active_config_recipe(config: &Config, cli: &CliValueOverrides) -> Option<String> {
    if let Some(config_recipe) = cli.config_recipe.as_ref() {
        return Some(config_recipe.clone());
    }

    if let Some(config_recipe) = config.config_recipe.active.as_ref() {
        return Some(config_recipe.clone());
    }

    if let Some(config_recipe) = config.config_recipe.default.as_ref() {
        return Some(config_recipe.clone());
    }

    config
        .config_recipe
        .recipes_dir
        .join("default.yaml")
        .is_file()
        .then(|| "default".to_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::{Config, ConfigError};

    #[test]
    fn invalid_account_pinned_in_file_layer_errors() {
        let mut config = Config::defaults().unwrap();
        let layer = super::FileConfig {
            account: Some(super::FileAccountConfig {
                pinned: Some("BAD!".to_owned()),
                ..super::FileAccountConfig::default()
            }),
            ..super::FileConfig::default()
        };
        let err = super::apply_file_config(&mut config, layer).unwrap_err();
        assert!(matches!(
            err,
            ConfigError::AccountConfigParse {
                field: "account.pinned",
                ..
            }
        ));
    }

    #[test]
    fn invalid_account_pinned_in_env_layer_errors() {
        let err = super::parse_account_id_env("CODEX_SESSION_ACCOUNT_PINNED", "BAD!").unwrap_err();
        assert!(matches!(
            err,
            ConfigError::EnvParse { key, .. } if key == "CODEX_SESSION_ACCOUNT_PINNED"
        ));
    }

    #[test]
    fn account_config_round_trips() {
        let value = "\"work\"";
        let parsed: crate::services::account::AccountId = serde_json::from_str(value).unwrap();
        let encoded = serde_json::to_string(&parsed).unwrap();
        assert_eq!(encoded, value);
    }

    #[test]
    fn account_config_defaults_include_quota_thresholds() {
        let account = super::AccountConfig::default();
        assert_eq!(account.quota_ttl_secs, 30);
        assert!((account.weekly_floor - 10.0).abs() < f64::EPSILON);
        assert!((account.five_hour_threshold - 50.0).abs() < f64::EPSILON);
        assert!((account.five_hour_weight - 0.70).abs() < f64::EPSILON);
    }
}
