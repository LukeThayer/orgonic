# Orgonic Ert Simulator

A Bevy (Rust) game with a reproducible Nix dev environment. The current starter app
is a fly-around camera orbiting a shaded cube — the foundation to build the simulator on.

## Prerequisites

- [Nix](https://nixos.org/download) with flakes enabled
  (`experimental-features = nix-command flakes` in `~/.config/nix/nix.conf`).
- That's it. Nix provides the exact Rust toolchain and system dependencies.

## Run it

```bash
nix develop            # enter the dev shell (pinned Rust toolchain on PATH)
cargo dev              # fast iterative build + run (dynamic linking enabled)
```

Controls: **click** to capture the cursor, **WASD** move, **Q/E** down/up,
**mouse** look, **Shift** boost, **Esc** release cursor.

## Build a shippable release

```bash
nix develop
cargo build --release  # no dynamic linking — the binary is self-contained
```

## How it's put together

- `flake.nix` / `flake.lock` — pin the environment (Rust via oxalica rust-overlay,
  `pkg-config`, macOS SDK).
- `Cargo.toml` / `Cargo.lock` — pin the libraries (Bevy 0.19). The `dev` feature
  enables `bevy/dynamic_linking`; the dev profile compiles our code fast and
  dependencies optimized.
- `.cargo/config.toml` — the `cargo dev` alias.
- `src/main.rs` — builds the world (cube + light).
- `src/camera.rs` — `FlyCameraPlugin` (input → camera movement).

### Fast compiles

- **Dynamic linking** is on via `cargo dev` (never in release).
- **macOS uses the default linker** (`ld-prime`/`ld64`) — it beats LLD for Bevy, so
  no linker is configured. On **Linux/Windows** you'd add a `[target.*]` linker block
  to `.cargo/config.toml`; see Bevy's reference
  [`config_fast_builds.toml`](https://github.com/bevyengine/bevy/blob/latest/.cargo/config_fast_builds.toml).
