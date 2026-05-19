# justfile — thin wrapper over pre-commit hooks.
#
# pre-commit is the source of truth for what runs as a quality gate.
# Recipes that gate code (test, lint, audit, machete, deny, check) call
# the corresponding hook ids from .pre-commit-config.yaml. Inner-loop
# recipes (build, run, watch, fmt, fix, clean, install) stay as direct
# cargo invocations because they are about fast local iteration, not
# gating, and several differ from the hook entries (e.g. `fmt --check`
# in lint is non-mutating while the pre-commit `cargo-fmt` hook mutates).

default:
    @just --list

# --- Build / run / clean (direct cargo, inner-loop) ---

# Build a release binary.
build:
    cargo build --release

# Run the binary, forwarding args (e.g. `just run -- self help`).
run *ARGS:
    cargo run -- {{ARGS}}

# Watch sources and re-run unit tests on change (requires cargo-watch).
watch:
    cargo watch -x 'nextest run --profile pre-commit'

# Install codex-session into ~/.cargo/bin.
install:
    cargo install --path .

# Remove the cargo-installed codex-session binary.
uninstall:
    cargo uninstall codex-session

# Remove Cargo build artifacts. Safe to run anytime.
clean:
    cargo clean

# --- Formatting / fix (direct cargo, mutate-on-purpose) ---

# Format all Rust sources in place.
fmt:
    cargo fmt --all

# Auto-fix formatting and clippy lints. Mirrors the pre-commit auto-fix hooks.
fix:
    cargo fmt --all
    cargo clippy --fix --allow-dirty --allow-staged --all-features -- -W clippy::all

# --- Quality gates (delegate to pre-commit) ---

# Lint gate (fmt --check + clippy-strict via pre-commit + local lint-print).
lint:
    cargo fmt --check
    pre-commit run --all-files clippy-strict
    just lint-print

# Stdout/stderr ownership rule (local; not in pre-commit yet).
lint-print:
    @! rg -n '(println!|print!|eprint(ln)?!)' \
        --glob '!src/ui/**' --glob '!src/error.rs' --glob '!src/logging.rs' \
        --glob '!tests/**' src
    @! rg -n '(stdout|stderr)\(\)' \
        --glob '!src/ui/**' --glob '!src/error.rs' --glob '!src/logging.rs' \
        --glob '!tests/**' src

# Fast unit tests (29 tests, ~20 ms). Delegates to pre-commit hook.
test-unit:
    pre-commit run --all-files cargo-nextest-unit

# Integration tests (127 tests; wrapper-to-stubbed-child seam, no real codex).
# Delegates to pre-commit hook. See .pre-commit-config.yaml for classification.
test-integration:
    pre-commit run --all-files --hook-stage pre-push cargo-nextest-integration

# All tests (unit + integration) via pre-commit. No E2E in this repo yet —
# when E2E lands it will run in CI only, never in this recipe.
test: test-unit test-integration

# Aggregate gate: lint + all tests. What contributors run before pushing.
check: lint test

# Run all pre-commit-stage hooks.
precommit:
    pre-commit run --all-files

# Run pre-commit AND pre-push stage hooks (full gate, mirrors what CI runs).
precommit-all:
    pre-commit run --all-files --hook-stage pre-commit
    pre-commit run --all-files --hook-stage pre-push

# Security audit (delegates to pre-commit cargo-audit hook).
audit:
    pre-commit run --all-files --hook-stage pre-push cargo-audit

# Detect unused dependencies (delegates to pre-commit cargo-machete hook).
machete:
    pre-commit run --all-files --hook-stage pre-push cargo-machete

# cargo-deny checks (delegates to pre-commit cargo-deny hook).
deny:
    pre-commit run --all-files --hook-stage pre-push cargo-deny
