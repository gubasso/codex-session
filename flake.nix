{
  description = "codex-session rust CLI dev shell (toolchain from rust-toolchain.toml)";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";
    flake-utils.url = "github:numtide/flake-utils";
    # The store guard of the canon (exobrain): `exobrain store-guard --daily`
    # in .envrc warns about working-tree copies in the Nix store.
    exobrain-tech = {
      url = "github:gubasso/exobrain-tech/v1.1.0";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs =
    {
      nixpkgs,
      rust-overlay,
      flake-utils,
      exobrain-tech,
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

        # The package that nix-secrets builds with its own nixpkgs, through
        # `pkgs.callPackage "${src}/nix/package.nix" { }`. It uses the
        # rustPlatform of nixpkgs, not the toolchain of the devshell.
        packages.default = pkgs.callPackage ./nix/package.nix { };

        # Each dependency has one owner, the most specific file that declares
        # it natively. .pre-commit-config.yaml pins each hook tool that the hook
        # runner can build an environment for (gitleaks, typos,
        # markdownlint-cli2, ...). The host supplies the hook runner prek
        # (host-requirements.toml). This shell holds the toolchain, just, the
        # tools of the hooks that keep
        # `language: system` (each names its owner in .pre-commit-config.yaml),
        # the tools that recipes call, and nodejs, which the node hook builds on.
        # It holds no language library.
        devShells.default = pkgs.mkShell {
          packages = [
            toolchain
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
          ]
          # exobrain, the store guard that .envrc runs once a day. exobrain-tech
          # builds it for x86_64-linux only, so other systems keep their shells.
          ++ pkgs.lib.optionals (system == "x86_64-linux") [
            exobrain-tech.packages.${system}.default
          ];
          # The greeting goes to standard error, because `nix develop --command`
          # shares the standard output of the command.
          shellHook = ''echo "codex-session dev shell ready (toolchain from rust-toolchain.toml)" >&2'';
        };
      }
    );
}
