#![allow(
    dead_code,
    clippy::unwrap_used,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    clippy::new_without_default
)] // Shared helper methods are used selectively by each integration test file.

use std::path::{Path, PathBuf};

pub struct TestEnv {
    pub tmp: tempfile::TempDir,
    pub home: PathBuf,
    pub cache: PathBuf,
    pub config_home: PathBuf,
    pub state_home: PathBuf,
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
        let fake_bin = tmp.path().join("bin");
        std::fs::create_dir_all(home.join(".codex")).unwrap();
        std::fs::create_dir_all(&cache).unwrap();
        std::fs::create_dir_all(&config_home).unwrap();
        std::fs::create_dir_all(&state_home).unwrap();
        Self {
            argc_file: tmp.path().join("codex.argc"),
            argv_file: tmp.path().join("codex.argv"),
            tmp,
            home,
            cache,
            config_home,
            state_home,
            fake_bin,
        }
    }

    pub fn cmd(&self) -> assert_cmd::Command {
        let mut cmd = assert_cmd::Command::cargo_bin("codex-session").unwrap();
        let path = format!("{}:/usr/bin:/bin", self.fake_bin.display());
        cmd.env_clear()
            .env("HOME", &self.home)
            .env("XDG_CACHE_HOME", &self.cache)
            .env("XDG_CONFIG_HOME", &self.config_home)
            .env("XDG_STATE_HOME", &self.state_home)
            .env("PATH", path);
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

    pub fn cmd_with_path(&self, path: &str) -> assert_cmd::Command {
        let mut cmd = assert_cmd::Command::cargo_bin("codex-session").unwrap();
        cmd.env_clear()
            .env("HOME", &self.home)
            .env("XDG_CACHE_HOME", &self.cache)
            .env("XDG_CONFIG_HOME", &self.config_home)
            .env("XDG_STATE_HOME", &self.state_home)
            .env("PATH", path);
        cmd
    }

    pub fn cmd_without_home(&self) -> assert_cmd::Command {
        let mut cmd = assert_cmd::Command::cargo_bin("codex-session").unwrap();
        let path = format!("{}:/usr/bin:/bin", self.fake_bin.display());
        let alt_home = self.tmp.path().join("alt-home");
        std::fs::create_dir_all(&alt_home).unwrap();
        cmd.env_clear()
            .env("HOME", alt_home)
            .env("XDG_CACHE_HOME", &self.cache)
            .env("XDG_CONFIG_HOME", &self.config_home)
            .env("XDG_STATE_HOME", &self.state_home)
            .env("PATH", path);
        cmd
    }

    pub fn install_base(&self) {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/base.toml");
        std::fs::copy(src, self.home.join(".codex/config.base.toml")).unwrap();
    }

    pub fn install_target_with_local(&self) {
        let src =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/target-with-local.toml");
        std::fs::copy(src, self.home.join(".codex/config.toml")).unwrap();
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

    pub fn stamp_path(&self) -> PathBuf {
        self.cache.join("codex-session/last-merge")
    }

    pub fn target_path(&self) -> PathBuf {
        self.home.join(".codex/config.toml")
    }

    pub fn base_path(&self) -> PathBuf {
        self.home.join(".codex/config.base.toml")
    }

    pub fn wrapper_user_config_path(&self) -> PathBuf {
        self.config_home.join("codex-session/config.toml")
    }

    pub fn normalize_text(&self, text: &str) -> String {
        text.replace(self.tmp.path().to_string_lossy().as_ref(), "<TMP>")
    }

    pub fn normalize_json(&self, value: &mut serde_json::Value) {
        match value {
            serde_json::Value::String(text) => {
                *text = self.normalize_text(text);
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    self.normalize_json(item);
                }
            }
            serde_json::Value::Object(entries) => {
                for value in entries.values_mut() {
                    self.normalize_json(value);
                }
            }
            serde_json::Value::Null | serde_json::Value::Bool(_) | serde_json::Value::Number(_) => {
            }
        }
    }

    #[allow(clippy::unused_self)]
    pub fn touch_older(&self, path: &Path) {
        Self::set_mtime(path, 1_577_836_800);
    }

    #[allow(clippy::unused_self)]
    pub fn touch_newer(&self, path: &Path) {
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
}
