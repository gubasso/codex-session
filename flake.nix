{
  description = "codex-session rust CLI dev shell (toolchain from rust-toolchain.toml)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      nixpkgs,
      rust-overlay,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };
        # Reads the channel and the rustfmt and clippy components from
        # rust-toolchain.toml. flake.lock fixes the version, so a local run
        # and CI get the same compiler and the same clippy lints.
        toolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;
      in
      {
        # `nix fmt` formats flake.nix.
        formatter = pkgs.nixfmt;

        # Each dependency has one owner, the most specific file that declares
        # it natively. pre-commit pins each hook tool that it can build an
        # environment for (gitleaks, typos, markdownlint-cli2, ...). This shell
        # holds the toolchain, pre-commit, just, the tools of the hooks that keep
        # `language: system` (each names its owner in .pre-commit-config.yaml),
        # the tools that recipes call, and nodejs, which the node hook builds on.
        # It holds no language library.
        devShells.default = pkgs.mkShell {
          packages = [
            toolchain
            # The meta hooks check-hooks-apply and check-useless-excludes run
            # `python -m pre_commit.meta_hooks...`. They import pre_commit only
            # through the PYTHONPATH that this devshell exports, so a commit
            # runs in the devshell: `direnv exec . git commit`.
            pkgs.pre-commit
            pkgs.just
            # language: system hooks.
            pkgs.taplo
            pkgs.dprint
            pkgs.lychee
            pkgs.cargo-nextest
            pkgs.cargo-audit
            pkgs.cargo-deny
            # Recipes and DEVELOPMENT.md: `just lint-print`, `just watch`, and
            # `cargo insta review`.
            pkgs.ripgrep
            pkgs.cargo-watch
            pkgs.cargo-insta
            # The signal tests start `python3` from PATH.
            pkgs.python3
            # The markdownlint-cli2 hook builds on this nodejs
            # (default_language_version in .pre-commit-config.yaml).
            pkgs.nodejs
          ];
          # The greeting goes to standard error, because `nix develop --command`
          # shares the standard output of the command.
          shellHook = ''echo "codex-session dev shell ready (toolchain from rust-toolchain.toml)" >&2'';
        };
      }
    );
}
