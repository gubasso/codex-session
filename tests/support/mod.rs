#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    clippy::new_without_default,
    missing_docs,
    unreachable_pub
)] // Shared helper methods are used selectively by each integration test file.

use std::path::{Path, PathBuf};

pub mod color;

pub fn fixture_path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

pub fn hermetic(td: &tempfile::TempDir) -> assert_cmd::Command {
    let mut cmd = assert_cmd::Command::cargo_bin("codex-session").unwrap();
    cmd.env_clear()
        .env("HOME", td.path())
        .env("XDG_CONFIG_HOME", td.path().join("config"))
        .env("XDG_STATE_HOME", td.path().join("state"))
        .env("XDG_CACHE_HOME", td.path().join("cache"))
        .env("XDG_RUNTIME_DIR", td.path().join("runtime"))
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("NO_COLOR", "1");
    cmd
}

pub fn with_stub_child(cmd: &mut assert_cmd::Command, fixture: &str) {
    let path = fixture_path(fixture);
    assert!(path.is_file(), "fixture missing: {}", path.display());
    cmd.env("CODEX_SESSION_CHILD_BIN", path);
}

pub struct TestEnv {
    pub tmp: tempfile::TempDir,
    pub home: PathBuf,
    pub cache: PathBuf,
    pub config_home: PathBuf,
    pub state_home: PathBuf,
    pub runtime: PathBuf,
    pub fake_bin: PathBuf,
    pub argc_file: PathBuf,
    pub argv_file: PathBuf,
}

impl TestEnv {
    pub fn new() -> Self {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("home");
        let cache = tmp.path().join("cache");
        let config_home = tmp.path().join("config");
        let state_home = tmp.path().join("state");
        let runtime = tmp.path().join("runtime");
        let fake_bin = tmp.path().join("bin");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::create_dir_all(config_home.join("codex-session")).unwrap();
        std::fs::create_dir_all(&state_home).unwrap();
        std::fs::create_dir_all(&runtime).unwrap();
        Self {
            argc_file: tmp.path().join("codex.argc"),
            argv_file: tmp.path().join("codex.argv"),
            tmp,
            home,
            cache,
            config_home,
            state_home,
            runtime,
            fake_bin,
        }
    }

    pub fn cmd(&self) -> assert_cmd::Command {
        let mut cmd = assert_cmd::Command::cargo_bin("codex-session").unwrap();
        let path = format!("{}:/usr/bin:/bin", self.fake_bin.display());
        color::clear_color_env(
            cmd.env_clear()
                .env("HOME", &self.home)
                .env("XDG_CACHE_HOME", &self.cache)
                .env("XDG_CONFIG_HOME", &self.config_home)
                .env("XDG_STATE_HOME", &self.state_home)
                .env("XDG_RUNTIME_DIR", &self.runtime)
                .env("PATH", path),
        );
        cmd
    }

    pub fn cmd_with_path(&self, path: &str) -> assert_cmd::Command {
        let mut cmd = assert_cmd::Command::cargo_bin("codex-session").unwrap();
        color::clear_color_env(
            cmd.env_clear()
                .env("HOME", &self.home)
                .env("XDG_CACHE_HOME", &self.cache)
                .env("XDG_CONFIG_HOME", &self.config_home)
                .env("XDG_STATE_HOME", &self.state_home)
                .env("XDG_RUNTIME_DIR", &self.runtime)
                .env("PATH", path),
        );
        cmd
    }

    pub fn cmd_without_home(&self) -> assert_cmd::Command {
        let mut cmd = assert_cmd::Command::cargo_bin("codex-session").unwrap();
        let path = format!("{}:/usr/bin:/bin", self.fake_bin.display());
        let alt_home = self.tmp.path().join("alt-home");
        std::fs::create_dir_all(&alt_home).unwrap();
        color::clear_color_env(
            cmd.env_clear()
                .env("HOME", alt_home)
                .env("XDG_CACHE_HOME", &self.cache)
                .env("XDG_CONFIG_HOME", &self.config_home)
                .env("XDG_STATE_HOME", &self.state_home)
                .env("XDG_RUNTIME_DIR", &self.runtime)
                .env("PATH", path),
        );
        cmd
    }

    pub fn make_fake_codex(&self) {
        use std::os::unix::fs::PermissionsExt as _;

        std::fs::create_dir_all(&self.fake_bin).unwrap();
        let codex = self.fake_bin.join("codex");
        #[allow(clippy::similar_names, clippy::uninlined_format_args)]
        let script = format!(
            "#!/usr/bin/env bash\nprintf '%s\\n' \"$#\" > '{}'\nprintf '' > '{}'\n\
for arg in \"$@\"; do\n  printf '%s\\n' \"$arg\" >> '{}'\ndone\nexit 0\n",
            self.argc_file.display(),
            self.argv_file.display(),
            self.argv_file.display(),
        );
        std::fs::write(&codex, script).unwrap();
        let mut perms = std::fs::metadata(&codex).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&codex, perms).unwrap();
    }

    pub fn make_fake_codex_printing_stdout(&self, stdout: &str) {
        Self::write_executable(
            &self.fake_bin.join("codex"),
            &format!("#!/usr/bin/env bash\nprintf '%s' '{stdout}'\n"),
        );
    }

    pub fn make_fake_codex_in_dir(&self, dir_name: &str, script_body: &str) -> PathBuf {
        let dir = self.tmp.path().join(dir_name);
        std::fs::create_dir_all(&dir).unwrap();
        let codex = dir.join("codex");
        Self::write_executable(&codex, script_body);
        dir
    }

    pub fn make_non_executable_codex_in_dir(&self, dir_name: &str, contents: &str) -> PathBuf {
        let dir = self.tmp.path().join(dir_name);
        std::fs::create_dir_all(&dir).unwrap();
        let codex = dir.join("codex");
        std::fs::write(&codex, contents).unwrap();
        dir
    }

    pub fn install_profile(&self, name: &str, manifest: &str, layers: &[(&str, &str)]) {
        self.write_profile_manifest(name, manifest);
        for (layer_name, body) in layers {
            self.write_settings_layer(layer_name, body);
        }
    }

    pub fn write_profile_manifest(&self, name: &str, manifest: &str) {
        let path = self.profiles_dir().join(format!("{name}.yaml"));
        Self::write_file(&path, manifest);
    }

    pub fn write_settings_layer(&self, name: &str, body: &str) {
        let path = self.settings_dir().join(format!("{name}.toml"));
        Self::write_file(&path, body);
    }

    pub fn write_cache_settings(&self, body: &str) {
        Self::write_file(&self.cache_settings_path(), body);
    }

    pub fn argc(&self) -> String {
        std::fs::read_to_string(&self.argc_file).unwrap()
    }

    pub fn argv(&self) -> Vec<String> {
        std::fs::read_to_string(&self.argv_file)
            .unwrap_or_default()
            .lines()
            .map(ToOwned::to_owned)
            .collect()
    }

    pub fn wrapper_user_config_path(&self) -> PathBuf {
        self.config_home.join("codex-session/config.toml")
    }

    pub fn xdg_runtime(&self) -> PathBuf {
        self.runtime.clone()
    }

    pub fn profiles_dir(&self) -> PathBuf {
        self.config_home.join("codex-session/profiles")
    }

    pub fn settings_dir(&self) -> PathBuf {
        self.config_home.join("codex-session/settings")
    }

    pub fn cache_settings_path(&self) -> PathBuf {
        self.cache.join("codex-session/settings.toml")
    }

    pub fn session_root(&self) -> PathBuf {
        self.runtime.join("codex-session")
    }

    pub fn session_dir(&self) -> PathBuf {
        let sessions_dir = self.session_root().join("sessions");
        let mut entries = std::fs::read_dir(&sessions_dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        entries.sort();
        entries.into_iter().next().unwrap()
    }

    pub fn normalize_text(&self, text: &str) -> String {
        let mut normalized = text.replace(self.tmp.path().to_string_lossy().as_ref(), "<TMP>");
        if self.session_root().exists() {
            normalized = self.normalize_session_paths(&normalized);
        }
        normalized
    }

    pub fn normalize_session_paths(&self, text: &str) -> String {
        let session_root = self.session_root();
        let sessions_dir = session_root.join("sessions");
        let mut normalized = text.to_owned();
        if session_root.exists() {
            normalized =
                normalized.replace(session_root.to_string_lossy().as_ref(), "<SESSION_ROOT>");
        }
        if sessions_dir.exists() {
            normalized =
                normalized.replace(sessions_dir.to_string_lossy().as_ref(), "<SESSIONS_DIR>");
        }
        if self.session_root().join("sessions").exists() {
            for entry in std::fs::read_dir(self.session_root().join("sessions")).unwrap() {
                let path = entry.unwrap().path();
                normalized = normalized.replace(path.to_string_lossy().as_ref(), "<SESSION_DIR>");
            }
        }
        normalized
    }

    pub fn normalize_json(&self, value: &mut serde_json::Value) {
        match value {
            serde_json::Value::String(text) => {
                *text = self.normalize_text(text);
                if text.ends_with('Z') && text.contains('T') && text.contains(':') {
                    "<STARTED_AT>".clone_into(text);
                }
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    self.normalize_json(item);
                }
            }
            serde_json::Value::Object(entries) => {
                for (key, value) in entries.iter_mut() {
                    if key == "started-at" {
                        *value = serde_json::Value::String("<STARTED_AT>".to_owned());
                    } else {
                        self.normalize_json(value);
                    }
                }
            }
            serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            }
        }
    }

    pub fn touch_older(path: &Path) {
        Self::set_mtime(path, 1_577_836_800);
    }

    pub fn touch_newer(path: &Path) {
        Self::set_mtime(path, 1_893_456_000);
    }

    fn set_mtime(path: &Path, seconds: i64) {
        let mtime = filetime::FileTime::from_unix_time(seconds, 0);
        filetime::set_file_mtime(path, mtime).unwrap();
    }

    fn write_executable(path: &Path, contents: &str) {
        use std::os::unix::fs::PermissionsExt as _;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
        let mut perms = std::fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms).unwrap();
    }

    fn write_file(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, contents).unwrap();
    }
}
