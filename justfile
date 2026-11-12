# justfile — thin wrapper over the git hooks, which the host hook runner
# prek runs.
#
# The hooks of .pre-commit-config.yaml are the source of truth for what runs
# as a quality gate.
# Recipes that gate code (test, lint, audit, machete, deny, check) call
# the corresponding hook ids from .pre-commit-config.yaml. Inner-loop
# recipes (build, run, watch, fmt, fix, clean) stay as direct
# cargo invocations because they are about fast local iteration, not
# gating, and several differ from the hook entries (e.g. `fmt --check`
# in lint is non-mutating while the `cargo-fmt` hook mutates).
#
# No recipe installs the binary. nix-secrets installs the host copy from
# nix/package.nix. `build` and `run` leave the binary in target/.
#
# The global host tools are checked once before any recipe runs. just
# evaluates every top-level assignment first, so a missing tool stops the
# run with the checker's message. Each recipe that runs the hook runner
# checks prek as its first line. host-requirements.toml lists them all.
_host := shell("exobrain host-check git nix")

default:
    @just --list

# --- Build / run / clean (direct cargo, inner-loop) ---

# Build a release binary into target/release/ (never onto PATH).
build:
    cargo build --release

# Run the binary, forwarding args (e.g. `just run -- self help`).
run *ARGS:
    cargo run -- {{ARGS}}

# Watch sources and re-run unit tests on change (requires cargo-watch).
watch:
    cargo watch -x 'nextest run --profile pre-commit'

# Remove Cargo build artifacts. Safe to run anytime.
clean:
    cargo clean

# --- Formatting / fix (direct cargo, mutate-on-purpose) ---

# Format all Rust sources in place.
fmt:
    cargo fmt --all

# Auto-fix formatting and clippy lints. Mirrors the auto-fix hooks.
fix:
    cargo fmt --all
    cargo clippy --fix --allow-dirty --allow-staged --all-features -- -W clippy::all

# --- Quality gates (delegate to the hooks) ---

# Lint gate (fmt --check + clippy-strict hook + local lint-print).
lint:
    exobrain host-check prek
    cargo fmt --check
    prek run --all-files clippy-strict
    just lint-print

# Stdout/stderr ownership rule (local; not a hook yet).
lint-print:
    @! rg -n '(println!|print!|eprint(ln)?!)' \
        --glob '!src/ui/**' --glob '!src/error.rs' --glob '!src/logging.rs' \
        --glob '!tests/**' src
    @! rg -n '(stdout|stderr)\(\)' \
        --glob '!src/ui/**' --glob '!src/error.rs' --glob '!src/logging.rs' \
        --glob '!tests/**' src

# Fast unit tests. Delegates to the hook.
test-unit:
    exobrain host-check prek
    prek run --all-files cargo-nextest-unit

# Integration tests (wrapper-to-stubbed-child seam, no real codex).
# Delegates to the hook. See .pre-commit-config.yaml for classification.
test-integration:
    exobrain host-check prek
    prek run --all-files --hook-stage pre-push cargo-nextest-integration

# All gated tests (unit + integration). Live tests are excluded — they
# require real credentials + network; run `just test-live` separately.
test: test-unit test-integration

# Live API tests — hit real endpoints. Requires real OAuth credentials +
# network. Delegates to the SoT hook (pre-push stage).
test-live:
    exobrain host-check prek
    prek run --all-files --hook-stage pre-push cargo-nextest-live

# Aggregate gate: lint + all tests. What contributors run before pushing.
check: lint test

# Run all pre-commit-stage hooks.
precommit:
    exobrain host-check prek
    prek run --all-files

# Run pre-commit AND pre-push stage hooks (full gate, mirrors what CI runs).
precommit-all:
    exobrain host-check prek
    prek run --all-files --hook-stage pre-commit
    prek run --all-files --hook-stage pre-push

# Security audit (delegates to the cargo-audit hook).
audit:
    exobrain host-check prek
    prek run --all-files --hook-stage pre-push cargo-audit

# Detect unused dependencies (delegates to the cargo-machete hook).
machete:
    exobrain host-check prek
    prek run --all-files --hook-stage pre-push cargo-machete

# cargo-deny checks (delegates to the cargo-deny hook).
deny:
    exobrain host-check prek
    prek run --all-files --hook-stage pre-push cargo-deny
