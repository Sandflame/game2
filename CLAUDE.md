# CLAUDE.md

Guide for working in this repository (for Claude and for humans).

## Project status
- Done: **M0** (plan), **M1** (scene, toon shading, movement), **M2** (targeting,
  GCD, hotbar, training dummies, rules/screen split), **M3** (effects, statuses,
  four classes, threat, sparring dummy, flame switching).
  **M4** (zones + portals, the Rootwarden trial: telegraphs, phases, adds,
  enrage, wipes, raises; particles, marker shader, hit flashes, boss
  animation, arena dressing, sounds). **M5** (levels 1–30 per class, XP,
  gear + loot, level sync, SQLite saving, character panel). **M6** (secondary
  class: 2 borrowed abilities + small stat bonus; party synergy bonuses;
  12-slot hotbar).
- Next: **M7** (hub city and the first world). See `MILESTONES.md`.
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
- `assets/data/client/` — client-only looks: `vfx.ron` (named looks: shape +
  particles + sound), `particles.ron` (particle presets), `sounds.ron`
  (sound files, game event → sound). Placeholder sounds in `assets/sounds/`
  are made by `tools/make_sounds.py`.

## Pinned versions (verified on crates.io 2026-10-03)
bevy 0.19.1 · lightyear 0.30.1 · avian3d 0.7.0 (later) · rusqlite 0.40.2
(`bundled`) · argon2 0.6.0 · ron 0.12.2 · serde 1 · bevy_egui 0.42.0 ·
bevy_hanabi 0.19.0 (`3d` feature only). Rust ≥ 1.95.
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

# Render a screenshot without a screen (Linux, software rendering).
# Run `cargo build` first: `cargo test`/`clippy` don't rebuild target/debug/client.
LANTERNFLAME_SCREENSHOT=shot.png xvfb-run -a -s "-screen 0 1280x720x24" ./target/debug/client
```

### Environment variables
- `LANTERNFLAME_ASSETS=<dir>` — use this `assets` folder instead of searching
  (search order: next to the program, current folder, project folder).
- `LANTERNFLAME_SCREENSHOT=<file.png>` — client saves a screenshot after 2 s
  of game time and quits (`client/src/devtools.rs`).
- `LANTERNFLAME_DB=<file>` — use this save file instead of the default
  (`%APPDATA%\Lanternflame\world.db` / `~/.local/share/lanternflame/world.db`).
  Screenshot demos never use a save file.
- `LANTERNFLAME_DEMO=trial` (with the above) — scripted scene: switch to
  Elementalist, walk through the portal, pull the Rootwarden, dodge a marker.
  `LANTERNFLAME_DEMO=classes` — lantern panel + sparring dummy.
  `LANTERNFLAME_DEMO=progress` — character panel + a bramble sprout. Saves
  `<file>-1.png`, `<file>-2.png`, …

### Linux build dependencies
`libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev` (and for headless
screenshots: `xvfb mesa-vulkan-drivers libxkbcommon-x11-0`). Windows needs nothing
extra beyond the Rust toolchain.

## Code map
- `shared/src/data.rs` — RON loading, `Validate` trait, `find_assets_dir()`.
- `shared/src/config.rs` — `GameConfig` (`assets/data/config/*.ron`).
- `shared/src/level.rs` — `Level` = one zone: geometry, collision, enemy spawns,
  portals, `revive_in_place`, optional encounter.
- `shared/src/movement.rs` — `step()`: the one movement function used everywhere.
- `shared/src/combat.rs` — `CombatConfig`, `ActionState` (GCD, cooldowns, casts,
  animation lock, queue, combos), `Health`, `Reject`, range.
- `shared/src/abilities.rs` — `AbilityDef`: timing + list of `EffectEntry`
  (`Effect` × `Recipients`), combos, tooltip summary.
- `shared/src/statuses.rs` — `StatusDef`, `Modifiers`, `Statuses` component
  (apply/refresh, shields absorbing, ticks, expiry).
- `shared/src/formulas.rs` — damage/healing maths (`amount × power% × buffs`,
  crits), `Rng` (SplitMix64). No "potency": abilities list plain amounts.
- `shared/src/describe.rs` — plain-language tooltips with the player's real numbers.
- `shared/src/threat.rs` — `ThreatTable` (top, taunt, forget).
- `shared/src/classes.rs` — `ClassDef`, `Role`, specializations (+ role weights),
  `lendable` abilities, `SecondaryChoice`/`Secondaries`, `check_secondary`,
  `CurrentClass`, `Stats`, hotbar layout.
- `shared/src/synergy.rs` — `SynergyDef` (`synergy.ron`): which bonus status a party
  missing each role gets; `coverage`/`bonuses`.
- `shared/src/components.rs` — logic components (`PlayerId`, `Motion`, `Faction`, `Hotbar`…).
- `shared/src/protocol.rs` — `ClientRequest`, `ServerEvent`, `Link`.
- `shared/src/targeting.rs` — Tab-target ordering.
- `shared/src/gamedata.rs` — `GameData`: loads config, abilities, statuses, classes,
  enemies, encounters, items, progression, synergy; checks every cross-file
  reference. `Zones` (all zones), `GameData::hotbar`: slots 1–8 class, 9–0
  secondary (borrowed), -/= shared lantern abilities (`HOTBAR_SLOTS = 12`).
- `shared/src/telegraphs.rs` — ground marker shapes, placements, `covers()` hit tests,
  the `Telegraph` component.
- `shared/src/encounters.rs` — boss fight data (phases, timelines, enrage, xp, loot) and `Progress`.
- `shared/src/progression.rs` — `ProgressionDef` (`progression.ron`: XP curve, per-level
  health/power), `ClassLevels` (per class level + xp), `effective_level` (level sync).
- `shared/src/items.rs` — `ItemDef` (`items/*.ron`), `Slot`, `Bag`, `Equipment` (shared
  armour + one weapon per class), `equip`/`unequip`/`discard`, `character_stats`
  (class + level + gear → `Stats`), `roll_loot`.
- `server/src/lib.rs` — `AuthorityPlugin`, tick order (`AuthoritySystems`).
- `server/src/requests.rs` — reads `ClientRequest`s.
- `server/src/actions.rs` — target validation (same zone only), using/queueing
  abilities, placing telegraphs when casts start, finishing casts.
- `server/src/effects.rs` — effects landing (damage, shared damage, heal, shield,
  status, taunt, raise), telegraphs going off/following players, status ticks, threat.
- `server/src/encounters.rs` — boss fight director (pull, phases, timeline queue,
  adds, enrage, victory, wipe → reset at the entrance).
- `server/src/enemies.rs` — enemy spawning, `EnemyBrain` rotations, facing, idle resets.
- `server/src/classes.rs` — flame changes (class switching).
- `server/src/progression.rs` — kill XP (everyone on the threat table), boss rewards,
  gear + secondary requests, `refresh_stats` (stats + hotbar from class/level/gear/
  secondary/zone sync), `apply_synergy` (party = same zone until M11; lasting
  statuses), new-character gear, save ↔ components, saving on change / every
  `autosave_every` / on exit.
- `server/src/database.rs` — SQLite (`world.db`): background thread, numbered
  `MIGRATIONS` tracked in `user_version`, backup before upgrading, load/save.
- `server/src/characters.rs` — players: joining, movement, combat clock, defeat,
  revive (only where `revive_in_place`), regen, portals (`interact`), forgetting
  characters who left a zone.
- `server/tests/authority.rs` — headless end-to-end rules tests with their own data.
- `client/src/session.rs` — local player id, joining, `Received` event messages.
- `client/src/characters.rs` — placeholder bodies per `VisualKey`, interpolation,
  sending movement, hit wobble, lantern (hidden by default; held up while the
  flame changes; `LanternSettings::always_show`), flame colours.
- `client/src/targeting.rs` — Tab/click/Esc targeting, target ring.
- `client/src/hud/` — hotbar (combo glow, tooltips), unit frames (class, shield,
  status chips) + cast bar, nameplates, floating numbers, messages, lantern panel (L),
  options menu (O, or Esc with nothing targeted: volume slider, mute, quit),
  character panel (C: levels, stats, secondary, party bonuses, worn gear, bag) + XP
  bar, secondary flame picker (`secondary.rs`, inside the lantern panel).
- `client/src/toon.rs` — `ToonMaterial` (extends StandardMaterial), outline
  material, `ToonAssets::spawn_part()` helper. Shaders in `assets/shaders/`.
- `client/src/world.rs` — `CurrentZone`, rebuilds scenery on zone change, portals,
  zone `border` dressing and `ambience` particles, hides things in other zones
  (`ElsewhereZone`).
- `client/src/telegraphs.rs` — ground markers with `MarkerMaterial`
  (`assets/shaders/marker.wgsl`, shape maths in the shader); bursts and a fading
  flash when one goes off.
- `client/src/vfx.rs` — looks from `vfx.ron`; `Looks` SystemParam plays one by name
  (shape, particles, sound); checks every look/particle/sound reference.
- `client/src/particles.rs` — hanabi effects built from `particles.ron`, warmed up at
  start; bursts clean themselves up.
- `client/src/audio.rs` — `Sounds` SystemParam, event sounds, `SoundVolume` (0–100,
  squared for loudness), M to mute.
- `client/src/settings.rs` — the player's settings file (volume, mute, lantern):
  `%APPDATA%\Lanternflame\settings.ron` or `~/.config/lanternflame/settings.ron`.
- `client/src/animation.rs` — hit flashes, `BossRig` (sway, wind-up, slam, sink), `Hop`.
- `client/src/hud/banner.rs` — big banners (boss speech, victory, wipes) + portal prompt.
- `client/src/camera.rs` — FFXIV-style follow camera; stays inside zones with a
  `border`; `CameraShake`.
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
- **Numbers players see must be simple**: plain amounts, Power as a percentage,
  buffs as +/- percentages (user request, 2026-10-04). Don't copy FFXIV's maths.
- Data files are validated at load; errors must name the file and field.
  Cross-file references (ability → status, class → ability…) are checked in
  `GameData::check_references`.
- Everything is per zone: recipients, targeting and threat only work within the
  same `Zone`. New characters must get a `Zone` component.
- Boss fights are data (`assets/data/encounters/`); telegraphed attacks are
  abilities with a `telegraph` and effects `to: InTelegraph`.
- New ability behaviour = new data. Only add an `Effect`/`Recipients` variant
  when no combination of existing ones can express it.
- Server tests (`server/tests/authority.rs`) use their own RON data in the test
  file, so balancing `assets/data` never breaks them.
- Player stats and hotbars are only ever set by `progression::refresh_stats` (it
  reacts to class, level, gear, secondary and zone changes); don't assign
  `Stats`/`Hotbar` elsewhere.
- Lasting statuses (synergy) have `expires = INFINITY`; the HUD shows no timer.
- Use `std::path::PathBuf` for paths (Windows + Linux).
- Database access goes through one module in `server/` (`database.rs`) and runs
  off the main game thread. Schema changes = a new entry in `MIGRATIONS`
  (never edit a released one). Without a `Database` resource nothing is saved
  (tests, demos).
- Errors: `anyhow` in binaries' setup code, typed errors in `shared`.
  No `unwrap()` outside tests except where truly impossible (comment why).
- Keep milestone scope tight; don't build "later" features early, but
  don't block them (see DESIGN.md §10).
- Commit at the end of each milestone with a summary of what was built.

## Development branch
`claude/multiplayer-rpg-rust-jwkfhc`
