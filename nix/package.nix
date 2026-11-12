# Builds codex-session with the nixpkgs of the caller. A flake that pins
# this repository as a source (flake = false) calls it with
# `pkgs.callPackage "${src}/nix/package.nix" { }`, and this repository's
# own flake exposes it as `packages.default`.
{
  lib,
  stdenv,
  rustPlatform,
  installShellFiles,
}:

let
  cargoToml = lib.importTOML ../Cargo.toml;
  inherit (cargoToml) package;
in
rustPlatform.buildRustPackage {
  pname = package.name;
  inherit (package) version;

  # Only the files that the build reads: the crate, its lock file, and
  # VERSION, which src/domain/version.rs embeds with include_str!. A commit
  # that changes only docs, tests, or hooks then keeps the same source hash
  # and causes no rebuild. The `files:` pattern of the nix-build hook in
  # .pre-commit-config.yaml follows this list, so keep the two in step.
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../VERSION
      ../src
    ];
  };
  # Cargo.lock holds no git source, so no outputHashes are needed.
  cargoLock.lockFile = ../Cargo.lock;

  # The test suite needs python3, a terminal, and stub binaries that the
  # build sandbox lacks. The pre-commit and pre-push hooks and CI own the
  # tests. This derivation proves that the crate builds.
  doCheck = false;

  nativeBuildInputs = [ installShellFiles ];

  # `completion` prints from the clap definition before the wrapper reads
  # its configuration or creates its log directory, so it runs in the
  # sandbox with no writable HOME. A cross build cannot run the binary, so
  # it skips the completion. installShellCompletion:
  # https://nixos.org/manual/nixpkgs/stable/#installshellfiles
  postInstall = lib.optionalString (stdenv.buildPlatform.canExecute stdenv.hostPlatform) ''
    installShellCompletion --cmd codex-session \
      --bash <("$out/bin/codex-session" completion bash)
  '';

  meta = {
    inherit (package) description;
    homepage = package.repository;
    # Cargo.toml declares `MIT OR Apache-2.0`. A list is how nixpkgs
    # spells a disjunction, kept in step with Cargo.toml by hand.
    license = [
      lib.licenses.mit
      lib.licenses.asl20
    ];
    mainProgram = "codex-session";
  };
}
