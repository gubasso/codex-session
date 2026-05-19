default:
    @just --list

# Build a release binary.
build:
    cargo build --release

# Run the binary, forwarding args (e.g. `just run -- self help`).
run *ARGS:
    cargo run -- {{ARGS}}

# Watch sources and re-run tests on change (requires cargo-watch).
watch:
    cargo watch -x 'nextest run'

# Format all Rust sources in place.
fmt:
    cargo fmt --all

# Auto-fix formatting and clippy lints. Mirrors the pre-commit auto-fix hooks.
fix:
    cargo fmt --all
    cargo clippy --fix --allow-dirty --allow-staged --all-features -- -W clippy::all

# Local quality gates.
lint:
    cargo fmt --check
    cargo clippy --all-targets --all-features -- -D warnings
    just lint-print

# Enforce the stdout/stderr ownership rule.
lint-print:
    @! rg -n '(println!|print!|eprint(ln)?!)' \
        --glob '!src/ui/**' --glob '!src/error.rs' --glob '!src/logging.rs' \
        --glob '!tests/**' src
    @! rg -n '(stdout|stderr)\(\)' \
        --glob '!src/ui/**' --glob '!src/error.rs' --glob '!src/logging.rs' \
        --glob '!tests/**' src

# Preferred test runner (cargo-nextest).
test:
    cargo nextest run

# Default aggregate: what CI and contributors should run before pushing.
check: lint test

# Install codex-session into ~/.cargo/bin (overwrites any prior cargo install).
install:
    cargo install --path . --force

# Remove the cargo-installed codex-session binary.
uninstall:
    cargo uninstall codex-session

# Remove Cargo build artifacts. Safe to run anytime.
clean:
    cargo clean

# Run pre-commit-stage hooks.
precommit:
    pre-commit run --all-files

# Run pre-commit AND pre-push stage hooks (adds bats, cargo-machete, cargo-audit, gitleaks).
precommit-all:
    pre-commit run --all-files --hook-stage pre-commit
    pre-commit run --all-files --hook-stage pre-push

# Security audit of the dependency graph. Not part of `check`.
audit:
    cargo audit

# Detect unused dependencies. Not part of `check`.
machete:
    cargo machete

# Run cargo-deny checks (requires cargo install --locked cargo-deny).
deny:
    cargo deny check
