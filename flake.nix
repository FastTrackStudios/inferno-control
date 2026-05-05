{
  description = "inferno-control: Rust port of network-audio-controller + Dioxus GUI (fts-ui)";

  inputs = {
    dioxus-flake.url = "github:FastTrackStudios/Dioxus-Flake";
    nixpkgs.follows = "dioxus-flake/nixpkgs";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs = { self, dioxus-flake, nixpkgs, flake-utils, ... }:
    flake-utils.lib.eachDefaultSystem (system: {
      # Pull the full Dioxus dev shell (Rust toolchain + GTK/WebKit/openssl/
      # tailwindcss/wasm-bindgen/etc) from FastTrackStudios/Dioxus-Flake so
      # `inferno-control-gui` builds against the same toolchain as fts-ui.
      devShells.default = dioxus-flake.devShells.${system}.default;
      devShells.mobile = dioxus-flake.devShells.${system}.mobile or dioxus-flake.devShells.${system}.default;
    });
}
