{
  description = "inferno-control: Rust port of network-audio-controller + Dioxus GUI (fts-ui)";

  inputs = {
    dioxus-flake.url = "github:FastTrackStudios/Dioxus-Flake";
    nixpkgs.follows = "dioxus-flake/nixpkgs";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, dioxus-flake, nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs { inherit system; };

        # Workspace contains a path-dep on `../FastTrackStudio/fts-ui` that
        # only exists in dev checkouts. Strip the GUI member so `cargo
        # metadata` doesn't try to resolve it during a CLI-only nix build.
        cliSrc = pkgs.runCommand "inferno-control-cli-src" { } ''
          cp -r ${./.} $out
          chmod -R u+w $out
          ${pkgs.gnused}/bin/sed -i \
            -e '/inferno-control-gui/d' \
            -e '/^fts-ui /d' \
            -e '/^dioxus /d' \
            -e '/^dioxus-primitives /d' \
            -e '/^lucide-dioxus /d' \
            -e '/^clsx /d' \
            -e '/^tw_merge /d' \
            $out/Cargo.toml
          rm -rf $out/crates/inferno-control-gui
        '';

        inferno-control = pkgs.rustPlatform.buildRustPackage {
          pname = "inferno-control";
          version = "0.1.0";
          src = cliSrc;
          cargoLock = {
            lockFile = "${cliSrc}/Cargo.lock";
            allowBuiltinFetchGit = true;
          };
          cargoBuildFlags = [ "-p" "inferno-control" ];
          cargoTestFlags = [ "-p" "inferno-control" "-p" "inferno-net" ];
          # mDNS unit tests bind UDP sockets which can fail in Nix's
          # network-isolated sandbox; protocol/parser tests still run.
          checkFlags = [ "--skip" "mdns" ];
          meta = with pkgs.lib; {
            description = "CLI to discover and control Dante / Inferno-AoIP audio devices";
            homepage = "https://github.com/FastTrackStudios/inferno-control";
            license = licenses.mit;
            mainProgram = "inferno-control";
            platforms = platforms.unix;
          };
        };
      in {
        # Pull the full Dioxus dev shell (Rust toolchain + GTK/WebKit/openssl/
        # tailwindcss/wasm-bindgen/etc) from FastTrackStudios/Dioxus-Flake so
        # `inferno-control-gui` builds against the same toolchain as fts-ui,
        # and layer on inferno-control-specific tools (cargo-nextest for
        # capn's pre-push hook to actually be able to run tests).
        devShells.default = pkgs.mkShell {
          inputsFrom = [ dioxus-flake.devShells.${system}.default ];
          packages = [ pkgs.cargo-nextest ];
        };
        devShells.mobile = pkgs.mkShell {
          inputsFrom = [
            (dioxus-flake.devShells.${system}.mobile or dioxus-flake.devShells.${system}.default)
          ];
          packages = [ pkgs.cargo-nextest ];
        };

        # Hermetic build of the CLI. The GUI is not yet packaged here — it
        # depends on a sibling fts-ui checkout and on the Dioxus-Flake
        # bundling pipeline (tailwind, dx bundle); see #15 for follow-up.
        packages.inferno-control = inferno-control;
        packages.default = inferno-control;
      });
}
