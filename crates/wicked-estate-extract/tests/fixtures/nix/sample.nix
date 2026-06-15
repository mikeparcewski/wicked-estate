# sample.nix — integration corpus fixture for the Nix extractor
# Demonstrates mkDerivation, let bindings, buildInputs, and shellHook.

{ pkgs ? import <nixpkgs> {}
, lib  ? pkgs.lib
}:

let
  # Project metadata
  pname   = "wicked-estate";
  version = "0.1.0";

  # Pinned Rust toolchain (stable)
  rustToolchain = pkgs.rust-bin.stable."1.79.0".default.override {
    extensions = [ "rust-src" "clippy" "rustfmt" ];
  };

  # Helper: a shell script wrapper installed into $out/bin
  makeWrapper = name: script:
    pkgs.writeShellScriptBin name script;

  # Dev-only utility wrappers
  cargoDeny  = makeWrapper "cargo-deny-check"  "exec cargo deny check \"$@\"";
  wickedTest = makeWrapper "wicked-test"        "exec cargo test --workspace \"$@\"";

in

pkgs.mkDerivation rec {
  inherit pname version;

  src = lib.cleanSource ./.;

  nativeBuildInputs = [
    rustToolchain
    pkgs.pkg-config
  ];

  buildInputs = [
    pkgs.sqlite
    pkgs.openssl
  ] ++ lib.optionals pkgs.stdenv.isDarwin [
    pkgs.darwin.apple_sdk.frameworks.Security
    pkgs.darwin.apple_sdk.frameworks.SystemConfiguration
  ];

  buildPhase = ''
    runHook preBuild
    cargo build --release --bin wicked-estate
    runHook postBuild
  '';

  installPhase = ''
    runHook preInstall
    install -Dm755 target/release/wicked-estate $out/bin/wicked-estate
    runHook postInstall
  '';

  # Development shell (used by `nix develop`)
  shellHook = ''
    export RUST_BACKTRACE=1
    export RUST_LOG=''${RUST_LOG:-info}
    echo "wicked-estate dev shell — $(rustc --version)"
  '';

  meta = with lib; {
    description = "Code-graph parser for LLM agents";
    homepage    = "https://github.com/example/wicked-estate";
    license     = licenses.mit;
    maintainers = [ maintainers.mikep ];
    platforms   = platforms.unix;
  };
}
