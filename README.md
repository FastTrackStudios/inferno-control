# inferno-control

Rust port of [`network-audio-controller`](https://github.com/chris-ritsen/network-audio-controller) — a control library and CLI/GUI for Dante network audio devices. Intended to pair with the [`inferno`](https://github.com/salanki/inferno) virtual-soundcard project.

## Layout

- `crates/inferno-net` — discovery + Dante control protocol library
- `crates/inferno-control` — CLI binary
- `crates/inferno-control-gui` — Dioxus GUI (planned, parity with Dante Controller)

## Status

Scaffold + mDNS discovery only. Protocol codec is a placeholder.

## Usage

```sh
cargo run -p inferno-control -- device list
cargo run -p inferno-control -- device list --json
```
