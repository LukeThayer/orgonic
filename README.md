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

## Edit particle effects

Particle effects are [Sprinkles](https://github.com/doceazedo/sprinkles) `.ron` files in
`assets/`. The dev shell ships the Sprinkles visual editor, so designing one is:

```bash
nix develop            # the editor comes with the shell
sprinkles              # launch the visual editor
```

In the editor, **Open** an effect from this repo's `assets/` (e.g. `assets/rain.ron`),
tweak it, and **Cmd+S** — it saves back to the same file in place. The bundled examples
under `~/.sprinkles/examples` are read-only starting points; editing one prompts a
Save As, so point that at `assets/` to add a new effect.

Everything in `assets/` today is still an unmodified copy of one of those stock examples —
so the erts are wearing off-the-shelf VFX until someone designs a real one.

The game does *not* hot-reload — restart `cargo dev` to see a saved change. (To change
that, add `"bevy/file_watcher"` to the `dev` feature in `Cargo.toml`.)

To use an effect, hand its path to `spawn_ert` — see `src/ert/flame_ert.rs`:

```rust
let effect = asset_server.load("3d-explosion.ron");
```

> The first `nix develop` after pulling this builds the editor from source (it's a full
> Bevy app, so give it a few minutes). It's cached in the Nix store afterwards, so every
> later `nix develop` is instant. If you only want the binary: `nix run .#sprinkles-editor`.

## How it's put together

- `flake.nix` / `flake.lock` — pin the environment (Rust via oxalica rust-overlay,
  `pkg-config`, macOS SDK) and build the `sprinkles` editor from `bevy_sprinkles_editor`
  with that same pinned toolchain.
- `Cargo.toml` / `Cargo.lock` — pin the libraries (Bevy 0.19, `avian3d` physics,
  `bevy_sprinkles` particles). The `dev` feature enables `bevy/dynamic_linking`; the dev
  profile compiles our code fast and dependencies optimized.
- `.cargo/config.toml` — the `cargo dev` alias.
- `assets/*.ron` — Sprinkles particle effects (edit with `sprinkles`, above).
- `src/main.rs` — builds the world (cube + light) and registers the plugins.
- `src/camera.rs` — `FlyCameraPlugin` (input → camera movement).
- `src/ert.rs` — `ErtPlugin`: the shared ert core (physics body + range sensor +
  `Particles3d` effect) and the `attract` behaviour every kind obeys.
- `src/ert/*.rs` — one sub-plugin per kind of ert, each picking its own effect and stats.

### Fast compiles

- **Dynamic linking** is on via `cargo dev` (never in release).
- **macOS uses the default linker** (`ld-prime`/`ld64`) — it beats LLD for Bevy, so
  no linker is configured. On **Linux/Windows** you'd add a `[target.*]` linker block
  to `.cargo/config.toml`; see Bevy's reference
  [`config_fast_builds.toml`](https://github.com/bevyengine/bevy/blob/latest/.cargo/config_fast_builds.toml).
