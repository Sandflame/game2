# CLAUDE.md

Guide for working in this repository (for Claude and for humans).

## Project status
- Current milestone: **M0 — planning** (no code yet). See `MILESTONES.md`.
- Full design: `DESIGN.md`.

## What this is
A small online tab-target RPG (FFXIV-style combat) for 4–8 friends,
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
```
(Server/client command-line options are documented here as they are added.)

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
