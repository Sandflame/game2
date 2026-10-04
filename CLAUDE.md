# CLAUDE.md

Guide for working in this repository (for Claude and for humans).

## Project status
- Done: **M0** (plan), **M1** (workspace, 3D scene, toon shading, local movement).
- Next: **M2** (server + networking). See `MILESTONES.md`.
- Full design: `DESIGN.md`.

## What this is
Lanternflame: a small online tab-target RPG (FFXIV-style combat) for 4–8 friends,
written in Rust with Bevy. Server-authoritative; clients send inputs and
render.

## Architecture (summary — details in DESIGN.md §3)
- `shared/` — components, network protocol, data-file types, and **all
  game rules** (formulas, cooldowns, movement, hit tests). No rendering.
  Every rule gets unit tests here.
- `server/` — headless Bevy app (`MinimalPlugins`). Owns the truth and
  the SQLite database. Must build and run on Windows and Linux unchanged.
- `client/` — rendering, input, UI, effects. Predicts the local player's
  movement; interpolates everything else. Never decides outcomes.
- `assets/data/` — RON data files with every tunable number.
- `assets/data/client/` — client-only visual mappings (vfx names → effects).

## Pinned versions (verified on crates.io 2026-10-03)
bevy 0.19.1 · lightyear 0.30.1 · avian3d 0.7.0 (later) · rusqlite 0.40.2
(`bundled`) · argon2 0.6.0 · ron 0.12.2 · serde 1 · bevy_egui 0.42.0 ·
bevy_hanabi 0.19.0 (from M5). Rust ≥ 1.95.
Upgrade only between milestones, all together, after checking crates.io.

## Commands
```sh
cargo build                                   # whole workspace
cargo clippy --all-targets -- -D warnings     # lints (must be clean)
cargo test                                    # all tests
cargo run -p client                           # run the game client
cargo run -p server                           # run the headless server
cargo test -p shared                          # just the game-rule tests
cargo fmt                                     # format code

# Render a screenshot without a screen (Linux, software rendering):
LANTERNFLAME_SCREENSHOT=shot.png xvfb-run -a -s "-screen 0 1280x720x24" ./target/debug/client
```

### Environment variables
- `LANTERNFLAME_ASSETS=<dir>` — use this `assets` folder instead of searching
  (search order: next to the program, current folder, project folder).
- `LANTERNFLAME_SCREENSHOT=<file.png>` — client saves a screenshot after
  ~120 frames and quits (`client/src/devtools.rs`).

### Linux build dependencies
`libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev` (and for headless
screenshots: `xvfb mesa-vulkan-drivers libxkbcommon-x11-0`). Windows needs nothing
extra beyond the Rust toolchain.

## Code map
- `shared/src/data.rs` — RON loading, `Validate` trait, `find_assets_dir()`.
- `shared/src/config.rs` — `GameConfig` (`assets/data/config/*.ron`).
- `shared/src/level.rs` — `Level` geometry: ground, boxes, cylinders; collision.
- `shared/src/movement.rs` — `step()`: the one movement function used everywhere.
- `client/src/toon.rs` — `ToonMaterial` (extends StandardMaterial), outline
  material, `ToonAssets::spawn_part()` helper. Shaders in `assets/shaders/`.
- `client/src/world.rs` — builds visuals for a `Level` (visual keys → placeholder meshes).
- `client/src/player.rs` — input → fixed-tick `step()` → interpolated `Transform`.
- `client/src/camera.rs` — FFXIV-style follow camera.
- `server/src/main.rs` — M1 placeholder: validates data and exits.

## Conventions
- **Game logic belongs in `shared`** as plain functions where possible
  (`fn resolve_damage(...) -> u32`), wrapped by thin Bevy systems. Plain
  functions are easy to test.
- **No magic numbers in code.** Tunable values go in `assets/data/*.ron`.
  Only true constants (e.g. protocol IDs) live in code.
- **Logic/visual split:** logic components never reference meshes,
  materials, or effects. The client attaches visuals by observing new
  entities. Data refers to visuals by string key.
- **Server-authoritative:** the client sends inputs/requests only.
- Data files are validated at load; errors must name the file and field.
- Use `std::path::PathBuf` for paths (Windows + Linux).
- Database access goes through one module in `server/` and runs off the
  main game thread.
- Errors: `anyhow` in binaries' setup code, typed errors in `shared`.
  No `unwrap()` outside tests except where truly impossible (comment why).
- Keep milestone scope tight; don't build "later" features early, but
  don't block them (see DESIGN.md §10).
- Commit at the end of each milestone with a summary of what was built.

## Development branch
`claude/multiplayer-rpg-rust-jwkfhc`
