# Phase 05 — Recursion guard on exec path + env scrubbing

**One-line summary.** Move the inode-self-check from the `--version`
probe onto the actual exec path. Enforce `CODEX_SESSION_*` env
scrubbing for every child invocation. Add regression tests for both.

## Prerequisites

Phases 01–04 must be complete. `ChildInvocation`, `Spawner`,
`AppContext::resolved_child` and `AppContext::spawner` must exist.

## Goal

After this phase:

- `StdSpawner::resolve_child` performs the inode self-check
  unconditionally. If the resolved path canonicalizes to the same
  inode as `std::env::current_exe()`, return
  `SpawnerError::Recursion { path }`.
- `StdSpawner::exec` performs a defensive marker-env check **before**
  calling `Command::exec()`. If `CODEX_SESSION_REENTRY=1` is present
  in the inherited env, return `SpawnerError::Recursion`. (Otherwise
  set `CODEX_SESSION_REENTRY=1` on the child invocation, so a chained
  wrapper can detect the loop on the next layer.)

  Wait — `CODEX_SESSION_REENTRY` is in the scrub list from Phase 04.
  Adjust: keep the scrub list as-is, **but** set
  `CODEX_SESSION_REENTRY=1` after the scrub. The scrub clears any
  inherited value; the set bumps the child to `1`. The intent is:
  - Start of wrapper: if env has `CODEX_SESSION_REENTRY=1`, the
    wrapper is being invoked as the child of another wrapper —
    refuse and exit with `SpawnerError::Recursion`.
  - Before exec: scrub all `CODEX_SESSION_*` (cleans state), then
    set `CODEX_SESSION_REENTRY=1` so the next layer can detect.

- The inode self-check **inside `child_version_line`** can stay
  (defense in depth) but is no longer the *only* guard.

- Every `ChildInvocation` built anywhere in the codebase uses
  `ChildEnv::scrubbed_default()` (or an explicit superset). Verify
  by grep that no `ChildInvocation { env: ChildEnv { inherit: true,
  remove: vec![], ... } }` construction exists outside tests.

- A new test `tests/cmd_recursion_guard.rs` ensures:
  - When `CODEX_SESSION_CHILD_BIN` points at the wrapper binary
    itself, the wrapper exits with code 70 (`EX_SOFTWARE`) and a
    clear stderr message naming the offending path.
  - When the wrapper is invoked with `CODEX_SESSION_REENTRY=1`
    already in the environment, it refuses to start and exits with
    code 70.

- A new test `tests/cmd_env_scrub.rs` ensures:
  - When the parent env has `CODEX_SESSION_CHILD_BIN`,
    `CODEX_SESSION_LOG_FILE`, `CODEX_SESSION_LOG_DIR` set, the child
    process does **not** see any of them. Use the
    `tests/fixtures/echo-argv.sh` stub from Phase 04, extended to
    echo selected env keys, or a new
    `tests/fixtures/echo-env.sh`.

## Spec rationale

- Three valid recursion-guard techniques: strip from PATH, marker
  env var, inode self-check —
  `cli-design/06-cli-wrapper-design/process-and-posix.md:164-182`.
- Resolve once, log resolved absolute path under `--mywrap-trace`,
  fail fast with exit 127 (missing) / 126 (not executable) /
  custom for recursion — `process-and-posix.md:180-188`.
- Env scrubbing: wrapper-private env must not leak —
  `process-and-posix.md:343-345`, `checklist.md:25-39`.
- PATH-resolution / shim tests — `process-and-posix.md:316-321`.

## Current state (verify before planning)

Assuming Phases 01–04 complete:

- `StdSpawner::resolve_child` does not check for recursion. (Note in
  Phase 04 said "No recursion check here — that's Phase 05".)
- `StdSpawner::child_version_line` still contains the inode self-check
  (carried forward from `src/adapters/process.rs:74-94` in the
  pre-refactor tree).
- `ChildEnv::scrubbed_default()` removes the four
  `CODEX_SESSION_*` keys but does **not** set `CODEX_SESSION_REENTRY=1`
  on the child.
- `AppContext::resolved_child` is populated at startup (lazily for
  wrapper-only verbs); resolution failure surfaces via
  `print_and_exit`.

## Target state

### `StdSpawner::resolve_child` with inode guard

```rust
fn resolve_child(&self, cfg: &ChildConfig) -> Result<Utf8PathBuf, SpawnerError> {
    let candidate: Utf8PathBuf = if let Some(p) = &cfg.bin {
        p.clone()
    } else {
        Utf8PathBuf::try_from(which::which("codex").map_err(|_| SpawnerError::NotFound {
            tried: Utf8PathBuf::from("codex"),
            path_searched: std::env::var_os("PATH"),
        })?)?
    };

    // Reject anything that isn't a regular executable file (current behavior
    // from adapters/process.rs:53-58 — keep it).
    let meta = std::fs::metadata(&candidate)
        .map_err(|_| SpawnerError::NotFound {
            tried: candidate.clone(),
            path_searched: None,
        })?;
    use std::os::unix::fs::PermissionsExt as _;
    if !meta.is_file() || meta.permissions().mode() & 0o111 == 0 {
        return Err(SpawnerError::NotExecutable { path: candidate });
    }

    // Recursion guard: compare canonicalized paths.
    let self_exe = std::env::current_exe().ok();
    if let Some(self_exe) = self_exe {
        let self_canon = self_exe.canonicalize().unwrap_or(self_exe);
        let cand_canon = candidate.canonicalize_utf8()
            .map(|p| p.into_std_path_buf())
            .unwrap_or_else(|_| candidate.clone().into_std_path_buf());
        if self_canon == cand_canon {
            return Err(SpawnerError::Recursion {
                path: candidate.clone(),
            });
        }
    }

    // Marker env: if we are already a re-entry, bail.
    if std::env::var("CODEX_SESSION_REENTRY").as_deref() == Ok("1") {
        return Err(SpawnerError::Recursion {
            path: candidate.clone(),
        });
    }

    Ok(candidate)
}
```

Note the canonicalization is best-effort — on EXDEV / missing dirs we
fall through to the literal path comparison, which is conservative.

### `ChildEnv::scrubbed_default` sets the marker

```rust
impl ChildEnv {
    pub(crate) fn scrubbed_default() -> Self {
        Self {
            inherit: true,
            remove: vec![
                "CODEX_SESSION_CHILD_BIN".into(),
                "CODEX_SESSION_LOG_FILE".into(),
                "CODEX_SESSION_LOG_DIR".into(),
                "CODEX_SESSION_REENTRY".into(),
            ],
            set: vec![
                ("CODEX_SESSION_REENTRY".into(),
                  std::ffi::OsString::from("1")),
            ],
        }
    }
}
```

Order matters: `into_command` applies `env_remove` then `env`, so the
final state is `REENTRY=1`. Verify by reading the
`ChildInvocation::into_command` body.

### Error rendering for `AppError::ChildRecursion`

The new variant from Phase 04:

```rust
#[error("child binary resolves to the wrapper itself")]
ChildRecursion {
    path: camino::Utf8PathBuf,
},
```

with:

```rust
impl AppError {
    pub(crate) fn exit_code(&self) -> u8 {
        match self {
            ...
            Self::ChildRecursion { .. } => 70, // EX_SOFTWARE
            ...
        }
    }
    pub(crate) fn kind(&self) -> &'static str {
        match self {
            ...
            Self::ChildRecursion { .. } => "child-recursion",
            ...
        }
    }
}
```

And `error::render(...)` must emit a clear message naming the offending
path and the override env var so a user can fix it:

```
error: child binary resolves to the wrapper itself
  resolved path: /usr/local/bin/codex-session
  override:      CODEX_SESSION_CHILD_BIN
  hint: unset CODEX_SESSION_CHILD_BIN or point it at the real `codex`.
```

## Tasks

1. **Move the inode-self-check into `StdSpawner::resolve_child`** as
    shown above. Add the `Recursion` SpawnerError to the resolver's
    return path.

2. **Add the marker-env check at the top of `resolve_child`** (or in
    `main`, before `Config::load`). Choose the spot that produces the
    clearest error message; the spawner is fine.

3. **Update `ChildEnv::scrubbed_default()`** to also set
    `CODEX_SESSION_REENTRY=1` after the scrub list.

4. **Add `AppError::ChildRecursion { path }`** variant (if not done
    in Phase 04 as a stub). Wire `from_spawner_error` to map
    `SpawnerError::Recursion`. Add to `kind()` and `exit_code()`.

5. **Update `src/error.rs` rendering.** The renderer should emit the
    hint and override-env-var name. Keep `err.kind` = `"child-recursion"`.

6. **(Optional but recommended) Add `--codex-session-trace` global
    flag** (long-only, namespaced per the spec). When set, log the
    resolved child path and the scrubbed env at `info` level so users
    can debug recursion errors without `-vv`. This costs nothing and
    matches `process-and-posix.md:180-182` ("log the resolved
    absolute path under `--mywrap-trace`").

7. **Add `tests/fixtures/echo-env.sh`**:

    ```bash
    #!/usr/bin/env bash
    # Stub child: writes selected env keys as JSON to stdout.
    set -euo pipefail
    keys=(CODEX_SESSION_CHILD_BIN CODEX_SESSION_LOG_FILE
          CODEX_SESSION_LOG_DIR CODEX_SESSION_REENTRY)
    printf '{'
    first=1
    for k in "${keys[@]}"; do
      [ $first -eq 1 ] && first=0 || printf ','
      v="${!k:-}"
      printf '"%s":"%s"' "$k" "$v"
    done
    printf '}\n'
    ```

    Make executable in the source tree.

8. **Add `tests/cmd_recursion_guard.rs`**:

    - Test A: copy the wrapper binary to a tempdir as `codex` (or
      use a symlink — `std::os::unix::fs::symlink`), set
      `CODEX_SESSION_CHILD_BIN=<tempdir>/codex`, invoke
      `codex-session foo`, assert exit 70 and stderr contains
      "resolves to the wrapper itself".
    - Test B: invoke `CODEX_SESSION_REENTRY=1 codex-session foo`,
      assert exit 70.

9. **Add `tests/cmd_env_scrub.rs`**:

    - Set parent env: `CODEX_SESSION_CHILD_BIN=<echo-env.sh>`,
      `CODEX_SESSION_LOG_FILE=/tmp/x.log`,
      `CODEX_SESSION_LOG_DIR=/tmp/xdir`.
    - Invoke `codex-session foo`.
    - Parse the JSON output of the stub.
    - Assert: `CODEX_SESSION_CHILD_BIN`, `CODEX_SESSION_LOG_FILE`,
      `CODEX_SESSION_LOG_DIR` are empty strings in the child env.
    - Assert: `CODEX_SESSION_REENTRY=1` in the child env.

10. **Re-run `tests/cmd_passthrough_argv.rs`** from Phase 04. The
    snapshot should still pass (argv translation is unchanged); the
    env diff in any env-snapshot may need regeneration via
    `cargo insta review`.

## Acceptance criteria

- [ ] `cargo check` passes.
- [ ] `cargo clippy --all-targets -- -D warnings` passes.
- [ ] `cargo test` passes.
- [ ] Setting `CODEX_SESSION_CHILD_BIN` to a path equal to the
  wrapper's `current_exe()` causes exit 70 with the documented
  error message.
- [ ] Invoking the wrapper with `CODEX_SESSION_REENTRY=1` in the
  parent env causes exit 70.
- [ ] In the child env, `CODEX_SESSION_CHILD_BIN`,
  `CODEX_SESSION_LOG_FILE`, `CODEX_SESSION_LOG_DIR` are all
  absent / empty.
- [ ] In the child env, `CODEX_SESSION_REENTRY=1` is set.
- [ ] `tests/cmd_recursion_guard.rs` exists and passes.
- [ ] `tests/cmd_env_scrub.rs` exists and passes.

## Tests

- `tests/cmd_recursion_guard.rs` (above).
- `tests/cmd_env_scrub.rs` (above).
- `tests/fixtures/echo-env.sh` (committed, executable).

## Out of scope

- Stripping `current_exe()`'s parent dir from the child's `PATH`.
  (Mentioned in the spec as an alternative guard — we use inode +
  marker instead. PATH-stripping can be added later if a real
  shim deployment ships.)
- The bigger `--codex-session-trace` infrastructure beyond the
  optional flag in Task 6. Tracing-level expansion is Phase 06's
  domain.
- Signal-forwarding tests. (Phase 09.)

## References

- `cli-design/06-cli-wrapper-design/process-and-posix.md:154-188, 343-345` — recursion guard + env scrubbing.
- `cli-design/06-cli-wrapper-design/checklist.md:25-39` — checklist items.
- pyenv shim recursion failure mode: <https://github.com/pyenv/pyenv/issues/2696>.
- pyenv shim pattern: <https://www.mungingdata.com/python/how-pyenv-works-shims/>.
