# Phase 03 — Config layer: `figment` + `directories` + `camino`

**One-line summary.** Introduce `src/config/` with a layered `figment`
loader, `directories`-based XDG path resolution, `Config` threaded
through `AppContext`. Replace ad-hoc env reads in
`src/domain/paths.rs`.

## Prerequisites

Phases 01 and 02 must be complete. Top-level `version`, `config`,
and passthrough must be working.

## Goal

After this phase:

- `src/config/` exists with `mod.rs` (loader + `Config` struct) and
  `error.rs` (`ConfigError`).
- `Config` is loaded once in `main` (before logging init) and
  threaded through `AppContext`.
- Layered merge: `defaults → user → project → env → CLI`.
  - **defaults**: hard-coded in the binary.
  - **user**: `~/.config/codex-session/config.toml` (resolved via
    `directories::ProjectDirs`).
  - **project**: `./.codex-session/config.toml` if present, walking
    upward from `cwd` until either found or filesystem root reached.
  - **env**: `CODEX_SESSION_*` prefix, double-underscore for nested.
  - **CLI**: the `--config <path>` global flag and any
    overrides expressible as flags.
- `deny_unknown_fields` on the `Config` struct — unknown TOML keys
  are rejected with a clear "where + why" error (per
  `cli-design/03-config-precedence.md:26-30`).
- `domain/paths.rs::CodexPaths` is removed (or shrunk to types only).
  Path resolution moves into `config/` and is exposed on `AppContext`
  as `ctx.paths: Paths`.
- `Cargo.toml` gains: `figment`, `toml`, `directories`, `camino`.
- The current `CODEX_SESSION_CHILD_BIN`, `CODEX_SESSION_LOG_FILE`,
  `CODEX_SESSION_LOG_DIR` env vars continue to work as overrides
  (they are now interpreted by the `figment::providers::Env`
  provider). New env vars all use `CODEX_SESSION_` prefix per
  `cli-design/03-config-precedence.md:62-66`.

## Spec rationale

- Precedence `cli > env > project > user > defaults` —
  `cli-design/03-config-precedence.md:7-43`.
- Per-key source provenance, `deny_unknown_fields` —
  `cli-design/03-config-precedence.md:21-30`.
- XDG via `directories`, not `dirs` —
  `cli-design/03-config-precedence.md:44-58`,
  `rust/cli-spec/07-dependencies.md:24, 75`.
- `figment` with `env, toml` features —
  `rust/cli-spec/07-dependencies.md:23`.
- `Config` is immutable after construction; lives on `AppContext` —
  `cli-design/03-config-precedence.md:71-79`,
  `cli-design/00-architecture.md` ("AppContext").
- Env var conventions (`<APP>_*` prefix, `__` for nested) —
  `cli-design/03-config-precedence.md:60-67`.

## Current state (verify before planning)

- `src/domain/paths.rs:24-83` (`CodexPaths::from_env`): hand-rolled
  reads of `HOME`, `XDG_CACHE_HOME`, `XDG_STATE_HOME`,
  `CODEX_SESSION_LOG_FILE`, `CODEX_SESSION_LOG_DIR`. Falls back to
  `/tmp/codex-session/...` for log file.
- `src/adapters/process.rs:46-66` reads `CODEX_SESSION_CHILD_BIN`
  directly via `std::env::var_os`.
- `src/context.rs::AppContext` holds only `fs`, `process`, `paths`,
  `ui`. No `Config`.
- `Cargo.toml:14-24` does not list `figment`, `toml`, `directories`,
  or `camino`.

## Target state

### `src/config/mod.rs`

```rust
//! Layered configuration.
//!
//! What this is: the immutable resolved Config + the figment loader.
//! What this is not: I/O during command execution. Config is built
//! once in main and threaded through AppContext.

pub(crate) mod error;
pub(crate) use error::ConfigError;

use camino::Utf8PathBuf;
use figment::{Figment, providers::{Format, Toml, Env, Serialized}};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct Config {
    /// Wrapper logging.
    pub(crate) log: LogConfig,
    /// Resolved file paths (XDG-driven, overridable via env/CLI).
    pub(crate) paths: PathsConfig,
    /// Child-binary resolution.
    pub(crate) child: ChildConfig,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct LogConfig {
    pub(crate) verbose: u8,
    pub(crate) mirror_stderr: bool,
    pub(crate) format: LogFormat,
    pub(crate) file: Option<Utf8PathBuf>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct PathsConfig {
    pub(crate) base_config: Utf8PathBuf,       // ~/.codex/config.base.toml
    pub(crate) target_config: Utf8PathBuf,     // ~/.codex/config.toml
    pub(crate) cache_dir: Utf8PathBuf,         // $XDG_CACHE_HOME/codex-session
    pub(crate) state_dir: Utf8PathBuf,         // $XDG_STATE_HOME/codex-session
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub(crate) struct ChildConfig {
    /// Explicit override; tried first.
    pub(crate) bin: Option<Utf8PathBuf>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
pub(crate) enum LogFormat {
    #[default] Json,
    Pretty,
}

impl Config {
    /// Build defaults from XDG-resolved paths.
    fn defaults() -> Result<Self, ConfigError> { /* ... */ }

    /// Layered load: defaults → user → project → env → CLI.
    pub(crate) fn load(cli_overrides: CliOverrides) -> Result<Self, ConfigError> {
        let defaults = Self::defaults()?;
        let mut figment = Figment::from(Serialized::defaults(&defaults));
        if let Some(user) = user_config_path()? {
            figment = figment.merge(Toml::file(user));
        }
        if let Some(project) = find_project_config(std::env::current_dir()?)? {
            figment = figment.merge(Toml::file(project));
        }
        figment = figment.merge(Env::prefixed("CODEX_SESSION_").split("__"));
        figment = figment.merge(Serialized::defaults(&cli_overrides));
        figment.extract().map_err(ConfigError::from)
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct CliOverrides {
    pub(crate) log: LogOverrides,
    pub(crate) child: ChildOverrides,
}
#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct LogOverrides {
    pub(crate) verbose: Option<u8>,
    pub(crate) mirror_stderr: Option<bool>,
    pub(crate) format: Option<LogFormat>,
}
#[derive(Debug, Clone, Default, Serialize)]
pub(crate) struct ChildOverrides {
    pub(crate) bin: Option<Utf8PathBuf>,
}

fn user_config_path() -> Result<Option<Utf8PathBuf>, ConfigError> {
    use directories::ProjectDirs;
    let Some(dirs) = ProjectDirs::from("", "", "codex-session") else { return Ok(None); };
    let p = Utf8PathBuf::try_from(dirs.config_dir().join("config.toml"))?;
    Ok(p.exists().then_some(p))
}

fn find_project_config(start: std::path::PathBuf) -> Result<Option<Utf8PathBuf>, ConfigError> {
    let mut here = start;
    loop {
        let candidate = here.join(".codex-session").join("config.toml");
        if candidate.is_file() {
            return Utf8PathBuf::try_from(candidate).map(Some).map_err(Into::into);
        }
        if !here.pop() { break; }
    }
    Ok(None)
}
```

### `src/config/error.rs`

```rust
//! Config-layer error type.

#[derive(Debug, thiserror::Error)]
pub(crate) enum ConfigError {
    #[error("config: missing XDG directories (no HOME?)")]
    NoXdg,
    #[error("config: parse error in {path}")]
    Parse { path: camino::Utf8PathBuf, #[source] source: figment::Error },
    #[error("config: unknown key {key} in {path}")]
    UnknownKey { key: String, path: camino::Utf8PathBuf },
    #[error("config: io error")]
    Io(#[from] std::io::Error),
    #[error("config: figment error")]
    Figment(#[from] figment::Error),
    #[error("config: non-utf8 path")]
    NonUtf8Path(#[from] camino::FromPathBufError),
}
```

`AppError::Config(#[from] ConfigError)` is added in Phase 08; for
this phase the loader returns `ConfigError` and `main` converts via
`AppError::Other(anyhow::anyhow!(...))` as a temporary bridge. Phase
08 finalizes the wiring.

### `AppContext` shape

```rust
pub(crate) struct AppContext {
    pub(crate) config: std::sync::Arc<crate::config::Config>,
    pub(crate) paths: crate::config::PathsConfig,   // sugar for ctx.config.paths
    pub(crate) fs: crate::adapters::fs::StdFs,
    pub(crate) process: crate::adapters::process::StdProcess,
    pub(crate) ui: crate::ui::Ui,
}
```

(Phase 04 will replace `process: StdProcess` with `spawner: StdSpawner`
and add a resolved-child field; do not over-build that here.)

### `Cargo.toml` additions

```toml
figment      = { version = "0.10", features = ["env", "toml"] }
toml         = "0.8"
directories  = "5"
camino       = { version = "1", features = ["serde1"] }
```

Keep existing entries unchanged.

## Tasks

1. **Add `Cargo.toml` deps** (above). Run `cargo build` to populate
    the lockfile.

2. **Create `src/config/mod.rs` and `src/config/error.rs`** per the
    target above. The `defaults()` function builds default paths via
    `directories::ProjectDirs` and `directories::BaseDirs`:

    - `paths.cache_dir`: `$XDG_CACHE_HOME/codex-session` (via
      `ProjectDirs::cache_dir()`).
    - `paths.state_dir`: `$XDG_STATE_HOME/codex-session` (via
      `ProjectDirs::state_dir()` — note `state_dir` is `Option`,
      fall back to `data_local_dir()` when unavailable).
    - `paths.base_config`, `paths.target_config`: keep
      `~/.codex/config.base.toml` and `~/.codex/config.toml` as
      hard-coded defaults (these are paths of the **child** codex,
      not the wrapper — they're an interop contract with the
      existing stow-managed config, per the repo's existing
      behavior).
    - `log.file`: `<state_dir>/codex-session.log` by default.
    - `log.format`: `LogFormat::Json` by default.

    If `ProjectDirs::from` returns `None` (no HOME), return
    `ConfigError::NoXdg`.

3. **Update `src/main.rs`**:

    - Read the root `Cli` with `Cli::parse()` (already done by Phase
      01).
    - Build `CliOverrides` from `cli.global` + any `--config <path>`
      flag (add this flag to `GlobalArgs` if not already present).
    - Call `Config::load(cli_overrides)`.
    - Wrap in `Arc<Config>`.
    - Build `AppContext` with the new fields.
    - On `ConfigError`, render via the existing `print_and_exit`
      path (use `AppError::Other(anyhow::anyhow!(err))` as the
      temporary bridge — Phase 08 replaces this with
      `AppError::Config(err)`).

4. **Add `--config <PATH>` to `GlobalArgs`** in `src/cli/mod.rs`:

    ```rust
    /// Override the user/project config file with an explicit path.
    #[arg(long, value_name = "PATH", global = true)]
    pub(crate) config: Option<camino::Utf8PathBuf>,
    ```

    In the loader, when `cli_overrides.config_file` is `Some(path)`,
    skip the user and project layers and merge only that file
    (between defaults and env).

5. **Replace `src/domain/paths.rs` consumers.** Every reference to
    `ctx.paths.base / target / cache_dir / stamp / log_file` should
    now use `ctx.config.paths.*`. The `stamp` path is derived from
    `paths.cache_dir.join("last-merge")` — provide a method
    `PathsConfig::stamp_file(&self) -> Utf8PathBuf` to keep callers
    succinct.

6. **Delete `src/domain/paths.rs`** entirely (or shrink it to just
    re-exports if there are external consumers — there shouldn't be).

7. **Migrate child-binary resolution.** `src/adapters/process.rs:46-66`
    currently reads `CODEX_SESSION_CHILD_BIN` from env at call time.
    Move that read into `Config::load` (it becomes `child.bin`).
    `resolve_codex` now consults `config.child.bin` first, then falls
    back to `which::which("codex")`. (Resolving once and caching the
    result happens in Phase 04 / 05; for now `resolve_codex` still
    runs each time.)

8. **Add `src/cli/config.rs` integration.** `config status` should
    now display:

    ```
    base:   <ctx.config.paths.base_config>
    target: <ctx.config.paths.target_config>
    stamp:  <ctx.config.paths.cache_dir>/last-merge
    merged: yes|no
    sources:
      defaults
      user:    <path-or-none>
      project: <path-or-none>
      env:     CODEX_SESSION_*
      cli:     <CliOverrides {...}>
    ```

    The "sources" block shows provenance — easy now because the
    figment merge order is fixed. Keep the JSON shape extensible.

9. **Provenance-rich error rendering.** When figment reports an
    error, extract the file path and (where available) line number
    and surface them in `ConfigError::Parse { path, source }`. The
    error renderer in `src/error.rs` will display these structured
    fields in Phase 08; this phase only needs to populate them.

10. **Update existing tests** that asserted specific log-path
    fallback behavior (`tests/cmd_logging.rs` probably touches
    `CODEX_SESSION_LOG_FILE`). They still work — the env var is
    now honored via the figment `Env` provider — but assertions on
    "log_path_degraded" message text may need adjusting. Verify
    every test still passes.

## Acceptance criteria

- [ ] `cargo check` passes.
- [ ] `cargo clippy --all-targets -- -D warnings` passes.
- [ ] `cargo test` passes (snapshots may need `cargo insta review`).
- [ ] `src/config/mod.rs` and `src/config/error.rs` exist.
- [ ] `src/domain/paths.rs` is deleted (or trivial).
- [ ] `AppContext` has an `Arc<Config>` field.
- [ ] `Cargo.toml` lists `figment`, `toml`, `directories`, `camino`.
- [ ] `codex-session config status` shows the new sources block.
- [ ] `CODEX_SESSION_LOG_FILE=/tmp/foo.log codex-session config status`
  shows `/tmp/foo.log` as the log file (env layer wins over
  defaults).
- [ ] `--config /tmp/x.toml codex-session ...` reads that file as
  the only TOML layer (replacing user + project).
- [ ] A user TOML file with an unknown key produces a
  `ConfigError::UnknownKey` (or `Parse` with the figment error
  identifying the unknown key) and exit code 78
  (`EX_CONFIG`).
- [ ] `~/.config/codex-session/config.toml` with
  `[log] verbose = 2` makes default verbosity `debug` without any
  `-vv` flag.

## Tests

- Add `tests/cmd_config_precedence.rs`: table-driven, sets defaults
  / user file / project file / env / CLI, asserts the resolved
  value matches expectations. Use `tempfile::TempDir` for the
  per-test HOME / XDG_CONFIG_HOME / cwd. Reuse `tests/support`
  env-clear helper.

- Update `tests/cmd_config_status.rs` snapshot to include the
  sources block.

- Update `tests/cmd_logging.rs` if it asserts on degraded-path
  text (the `/tmp` fallback message goes away — figment will not
  emit it).

## Out of scope

- Replacing `Process` with `Spawner` or introducing
  `ChildInvocation`. (Phase 04.)
- Recursion guard / env scrubbing. (Phase 05.)
- Switching the log sink to `tracing-appender`. (Phase 06.)
- Adding `Config(#[from] ConfigError)` to `AppError`. (Phase 08 —
  use `AppError::Other(anyhow::anyhow!(err))` as a bridge here.)
- Project-config schema validation beyond `deny_unknown_fields`.
  No business-level invariants in `Config` (those live in
  `domain/`).

## References

- `cli-design/03-config-precedence.md` — precedence, XDG, env conventions, immutability.
- `rust/cli-spec/05-config.md:19-32, 60-99` — figment loader, deny_unknown_fields.
- `rust/cli-spec/07-dependencies.md:23-26, 71-77` — figment, toml, directories, camino, no-`dirs`.
- `figment` docs: <https://docs.rs/figment/latest/figment/>.
- `directories` docs: <https://docs.rs/directories/latest/directories/struct.ProjectDirs.html>.
- `camino` docs: <https://docs.rs/camino/latest/camino/>.
