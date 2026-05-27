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

# Install codex-session into ~/.cargo/bin (+ bash completions if running bash).
install:
    cargo install --path .
    @if echo "$SHELL" | grep -q 'bash$$'; then \
        mkdir -p ~/.local/share/bash-completion/completions && \
        codex-session completion bash > ~/.local/share/bash-completion/completions/codex-session && \
        echo 'Installed bash completions for codex-session'; \
    fi

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

# Fast unit tests. Delegates to pre-commit hook.
test-unit:
    pre-commit run --all-files cargo-nextest-unit

# Integration tests (wrapper-to-stubbed-child seam, no real codex).
# Delegates to pre-commit hook. See .pre-commit-config.yaml for classification.
test-integration:
    pre-commit run --all-files --hook-stage pre-push cargo-nextest-integration

# All gated tests (unit + integration). Live tests are excluded — they
# require real credentials + network; run `just test-live` separately.
test: test-unit test-integration

# Live API tests — hit real endpoints (WHAM usage). Requires real OAuth
# credentials + network. NOT a git hook — binary(/live/) is explicitly
# excluded from the pre-push nextest profile. Calls cargo nextest directly
# (no pre-commit hook exists for this tier).
# See: .config/nextest.toml [profile.live], docs/wham-usage-api-spec.md §7.
test-live:
    CODEX_SESSION_LIVE_TESTS=1 cargo nextest run --profile live --all-features

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
