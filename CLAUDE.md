# CLAUDE.md

Guide for working in this repository (for Claude and for humans).

## Project status
- Done: **M0** (plan), **M1** (scene, toon shading, movement), **M2** (targeting,
  GCD, hotbar, training dummies, rules/screen split).
- Next: **M3** (data-driven abilities, four classes). See `MILESTONES.md`.
- Order: single-player content first; multiplayer is **M11** (user's choice, 2026-10-04).
- Full design: `DESIGN.md`.

## What this is
Lanternflame: a small online tab-target RPG (FFXIV-style combat) for 4–8 friends,
written in Rust with Bevy. Server-authoritative; clients send inputs and
render.

## Architecture (summary — details in DESIGN.md §3)
- `shared/` — components, network protocol, data-file types, and **all
  game rules** (formulas, cooldowns, movement, hit tests). No rendering.
  Every rule gets unit tests here.
- `server/` — the **authority** (rules half) as a library (`AuthorityPlugin`),
  plus a headless binary for multiplayer later. Owns the truth (and later the
  SQLite database). Must build and run on Windows and Linux unchanged.
- `client/` — rendering, input, UI, effects. **Runs `AuthorityPlugin`
  in-process** until multiplayer (M11). Never decides outcomes.
- The halves talk only through `shared::protocol::Link`: the client pushes
  `ClientRequest`s, the authority pushes `ServerEvent`s. The client may *read*
  logic components (they'll be replicated later) but never writes them.
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
- `LANTERNFLAME_SCREENSHOT=<file.png>` — client saves a screenshot after 2 s
  of game time and quits (`client/src/devtools.rs`).
- `LANTERNFLAME_DEMO=1` (with the above) — scripted fight on a dummy; saves
  `<file>-1.png` (mid-cast) and `<file>-2.png` (after the hit).

### Linux build dependencies
`libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev` (and for headless
screenshots: `xvfb mesa-vulkan-drivers libxkbcommon-x11-0`). Windows needs nothing
extra beyond the Rust toolchain.

## Code map
- `shared/src/data.rs` — RON loading, `Validate` trait, `find_assets_dir()`.
- `shared/src/config.rs` — `GameConfig` (`assets/data/config/*.ron`).
- `shared/src/level.rs` — `Level` geometry: ground, boxes, cylinders; collision.
- `shared/src/movement.rs` — `step()`: the one movement function used everywhere.
- `shared/src/combat.rs` — `AbilityDef`, `CombatConfig`, `ActionState` (GCD,
  cooldowns, casts, animation lock, queue), `Health`, damage, range.
- `shared/src/components.rs` — logic components (`PlayerId`, `Motion`, `Faction`, `Hotbar`…).
- `shared/src/protocol.rs` — `ClientRequest`, `ServerEvent`, `Link`.
- `shared/src/targeting.rs` — Tab-target ordering.
- `shared/src/gamedata.rs` — `GameData`: loads config, abilities, enemies; checks references.
- `server/src/lib.rs` — `AuthorityPlugin`, tick order (`AuthoritySystems`).
- `server/src/{requests,actions,characters}.rs` — handling requests, using
  abilities/applying damage, spawning/moving characters, dummy resets.
- `server/tests/authority.rs` — headless end-to-end rules tests with their own data.
- `client/src/session.rs` — local player id, joining, `Received` event messages.
- `client/src/characters.rs` — placeholder bodies per `VisualKey`, interpolation,
  sending movement, hit wobble, lantern glow.
- `client/src/targeting.rs` — Tab/click/Esc targeting, target ring.
- `client/src/hud/` — hotbar, unit frames + cast bar, nameplates, damage numbers, messages.
- `client/src/toon.rs` — `ToonMaterial` (extends StandardMaterial), outline
  material, `ToonAssets::spawn_part()` helper. Shaders in `assets/shaders/`.
- `client/src/world.rs` — builds visuals for a `Level` (visual keys → placeholder meshes).
- `client/src/camera.rs` — FFXIV-style follow camera.
- `server/src/main.rs` — placeholder until M11: validates data and exits.

## Conventions
- **Game logic belongs in `shared`** as plain functions where possible
  (`fn resolve_damage(...) -> u32`), wrapped by thin Bevy systems. Plain
  functions are easy to test.
- **No magic numbers in code.** Tunable values go in `assets/data/*.ron`.
  Only true constants (e.g. protocol IDs) live in code.
- **Logic/visual split:** logic components never reference meshes,
  materials, or effects. The client attaches visuals by observing new
  entities. Data refers to visuals by string key.
- **Server-authoritative:** the client sends inputs/requests only, through `Link`.
- Visible things are built from `VisualKey` strings in the client; logic never
  references meshes.
- Clippy `type_complexity` and `too_many_arguments` are allowed workspace-wide
  (normal for Bevy systems).
- Dependencies build without debug info (`Cargo.toml` profile) to save disk space.
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
