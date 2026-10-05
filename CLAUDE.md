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
  12-slot hotbar). **M7** (Lanternhold hub with closed district gates and
  townsfolk, the root slide ride, Whisperwood forest; enemies that notice,
  chase, assist, leash and respawn at home). **M8** (the Tangled Burrow
  dungeon: 3 bosses, walls, about 3 minutes; instanced zones; the dungeon
  board; the way out after a boss falls; bosses topple when defeated).
  **M9** (quests + skippable dialogue as data, starter chain hub → root →
  forest → dungeon, quest tracker and log; minimap + big map with N/E/S/W).
  **M10** (all 13 specializations; stacking statuses, slows, lunges, damage
  that heals; spec switching in the lantern panel, saved per class).
- Next: **M11** in three stages (see `MILESTONES.md`): 1) real character
  models (KayKit, longer and slimmer), five races (Humans, Elves, Drakes,
  Demons, Lynari), customization, five gear tiers (Common to Legendary), accounts + character list + character creation; 2) networking;
  3) parties + chat. The art direction is agreed: `docs/art-direction.md`
  (samples in `docs/art/`, made by `client/examples/art_samples.rs`).
- Order: single-player content first; multiplayer is **M11** (user's choice, 2026-10-04).
- Full design: `DESIGN.md` (all user decisions: §12 and §13).

## Working with the user
- The user is not an experienced programmer: explain in plain language,
  give exact steps, and push back honestly on ideas that won't work well.
- They play on **Windows** (PowerShell, project in
  `Documents\game2`). To try a pushed change: `git pull`, then
  `cargo run -p client`.
- Milestone by milestone (or stage by stage): at the end `cargo build`,
  `cargo clippy --all-targets -- -D warnings` and `cargo test` pass; explain
  how to run and test it; commit and push; stop for feedback. Show
  screenshots of visual work (see the screenshot commands below).
- Don't open pull requests unless asked.
- Keep this file, `DESIGN.md` and `MILESTONES.md` up to date: new threads
  only know what is written here.

## Working in parallel threads
The user may split work across several Claude threads (sessions) on this
repository. Each thread starts knowing nothing of earlier chats, only these
files.
- One area per thread, so threads don't edit the same files. Natural splits:
  **characters & gear art** (`client/` visuals, `assets/models/`,
  `docs/art-direction.md`); **accounts & networking** (`server/`,
  `shared/protocol.rs`, database migrations); **content** (zones, quests,
  enemies in `assets/data/`).
- Each thread works on its own branch, named after its area. Merging into the
  main development branch `claude/multiplayer-rpg-rust-jwkfhc` is done by one
  thread at a time, after build, clippy and tests pass.
- Shared hot spots that need care: `shared/src/protocol.rs`,
  `server/src/database.rs` `MIGRATIONS` (two threads must never add the
  same migration number), `CLAUDE.md`, `MILESTONES.md`.

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
  (sound files, game event → sound), `models.ron` (character models: body
  proportions, bodies, weapons, what each class/spec wears and holds,
  townsfolk, animation clips). Placeholder sounds in `assets/sounds/`
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
  `LANTERNFLAME_DEMO=progress` — character panel + a bramble sprout.
  `LANTERNFLAME_DEMO=world` — hub, talk to a townsperson, root slide,
  Whisperwood, a thornwolf pack, the burrow's mouth.
  `LANTERNFLAME_DEMO=dungeon` — the dungeon board, then a quick (cheating)
  tour of the Tangled Burrow and back.
  `LANTERNFLAME_DEMO=quests` — take Ilsa's quest, the map, Fen, the quest log.
  `LANTERNFLAME_DEMO=specs` — specializations in the lantern panel, then
  Scissors: five Snips and a Shear on a dummy.
  `LANTERNFLAME_DEMO=models` — each class's model up close from the front,
  the lantern held up while the flame changes.
  Leave ~1.5 s after a `Shot` before changing what's on screen: software
  rendering is slow and the shot is taken at the end of the frame.
  Each demo starts in its own zone
  (`devtools::demo_start_zone`); demos may cheat (teleport, defeat). Saves
  `<file>-1.png`, `<file>-2.png`, …

### Linux build dependencies
`libasound2-dev libudev-dev libwayland-dev libxkbcommon-dev` (and for headless
screenshots: `xvfb mesa-vulkan-drivers libxkbcommon-x11-0`). Windows needs nothing
extra beyond the Rust toolchain.

## Code map
- `shared/src/data.rs` — RON loading, `Validate` trait, `find_assets_dir()`.
- `shared/src/config.rs` — `GameConfig` (`assets/data/config/*.ron`).
- `shared/src/level.rs` — `Level` = one zone: geometry, collision (walls are
  `Box` obstacles), enemy spawns, portals (`to`/`ride`/`closed`/`back`/`board`,
  `arrive_yaw`, `visual`), `npcs`, `decorations` (looks only), `revive_in_place`,
  `level_sync`, `encounters`, `instanced` + `exit`, board `listing`.
  `base_zone()`: instance ids are `zone#n`.
- `shared/src/enemy_ai.rs` — notice / leash / approach / stop-distance rules.
- `shared/src/rides.rs` — `RideDef` (`rides/*.ron`): scripted rides along a
  Catmull-Rom path (the root slide).
- `shared/src/movement.rs` — `step()`: the one movement function used everywhere.
- `shared/src/combat.rs` — `CombatConfig`, `ActionState` (GCD, cooldowns, casts,
  animation lock, queue, combos), `Health`, `Reject`, range.
- `shared/src/abilities.rs` — `AbilityDef`: timing + list of `EffectEntry`
  (`Effect` × `Recipients`: Damage, Heal, Shield, ApplyStatus, Taunt,
  SharedDamage, Raise, StackedDamage, HealingDamage, Lunge), combos.
- `shared/src/statuses.rs` — `StatusDef` (`max_stacks`), `Modifiers` (incl.
  `move_speed`, once per stack), `Statuses` component (apply/refresh/stack,
  shields absorbing, ticks, expiry).
- `shared/src/formulas.rs` — damage/healing maths (`amount × power% × buffs`,
  crits), `Rng` (SplitMix64). No "potency": abilities list plain amounts.
- `shared/src/describe.rs` — plain-language tooltips with the player's real numbers.
- `shared/src/threat.rs` — `ThreatTable` (top, taunt, forget).
- `shared/src/classes.rs` — `ClassDef`, `Role`, specializations (+ role weights),
  `lendable` abilities, `SecondaryChoice`/`Secondaries`, `check_secondary`,
  `CurrentClass`, `ChosenSpecs` (each class's chosen spec), `Stats`, hotbar layout.
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
- `shared/src/quests.rs` — `QuestDef` (`quests/*.ron`: giver, offer dialogue,
  `after`, steps with `Goal` Talk/Defeat/Reach/Win, xp, items), `DialogueDef`
  (`dialogue/*.ron`: lines of `who`/`says`), `QuestLog` component (`talk`,
  `record(Deed)`, `offers`, `marker` `!`/`?`, `tidy`), `step_text`.
- `shared/src/encounters.rs` — boss fight data (phases, timelines, enrage, xp, loot,
  `exit_portal`) and `Progress`.
- `shared/src/progression.rs` — `ProgressionDef` (`progression.ron`: XP curve, per-level
  health/power), `ClassLevels` (per class level + xp), `effective_level` (level sync).
- `shared/src/items.rs` — `ItemDef` (`items/*.ron`), `Slot`, `Bag`, `Equipment` (shared
  armour + one weapon per class), `equip`/`unequip`/`discard`, `character_stats`
  (class + level + gear → `Stats`), `roll_loot`.
- `server/src/lib.rs` — `AuthorityPlugin`, tick order (`AuthoritySystems`).
- `server/src/requests.rs` — reads `ClientRequest`s.
- `server/src/actions.rs` — target validation (same zone only), using/queueing
  abilities, placing telegraphs when casts start, finishing casts.
- `server/src/effects.rs` — effects landing (damage, shared/stacked/healing
  damage, heal, shield, status + stacks, taunt, raise, lunge → `apply_lunges`),
  telegraphs going off/following players, status ticks, threat. Slows are
  applied where characters move (`characters.rs`, `enemies.rs`).
- `server/src/encounters.rs` — boss fight director (pull, phases, timeline queue,
  adds, enrage, victory → `ExitPortal`, wipe → reset at the entrance). A zone can
  hold several fights (a dungeon's bosses).
- `server/src/enemies.rs` — enemy spawning (`EnemyHome`, `Roaming`), `notice_players`
  (aggro + assist), `move_enemies` (chase, leash → `Returning` home and heal),
  `EnemyBrain` rotations (waits until in range), facing, idle resets / respawn at home.
- `server/src/travel.rs` — `Travel` (`go_to` a zone or a group's copy of it),
  `use_portal` (closed gates speak, boards open the list, rides, `back`), `CameFrom`
  (where to return to), `enter_from_board`, NPCs (`Npc`, `talk` cycles lines),
  `Riding` + `advance_rides`.
- `server/src/instances.rs` — `fill_zone` (enemies, boss fights, people), filling
  ordinary zones at startup, `Instances` (a fresh `zone#n` copy per group,
  removed when empty; defeated enemies stay down).
- `server/src/quests.rs` — `PendingDeeds` (kills, arrivals, boss wins) →
  `record_deeds`; `talked` (called from `travel::talk`: quest step or offer,
  else the person's idle lines); quest rewards go through `PendingRewards`.
- `server/src/classes.rs` — flame changes (class switching; the new class uses
  its remembered spec) and spec changes (`PendingSpecs`, out of combat).
- `server/src/progression.rs` — kill XP (everyone on the threat table), boss rewards,
  gear + secondary requests, `refresh_stats` (stats + hotbar from class/level/gear/
  secondary/zone sync), `apply_synergy` (party = same zone until M11; lasting
  statuses), new-character gear, save ↔ components, saving on change / every
  `autosave_every` / on exit.
- `server/src/database.rs` — SQLite (`world.db`): background thread, numbered
  `MIGRATIONS` tracked in `user_version`, backup before upgrading, load/save.
- `server/src/characters.rs` — players: joining (a save inside a dungeon copy
  loads at its `exit`), movement, combat clock, defeat, revive (only where
  `revive_in_place`), `recover_wipes` (everyone down outside a boss fight → back
  to the entrance), regen, `interact` (portals, exit portals, talking),
  forgetting characters who left a zone.
- `server/tests/authority.rs` — headless end-to-end rules tests with their own data.
- `client/src/session.rs` — local player id, joining, `Received` event messages.
- `client/src/characters.rs` — bodies per `VisualKey` (players and townsfolk get
  a `ModelLook`; dummies and monsters are still built from shapes), interpolation,
  sending movement, hit wobble, lantern (hidden by default; held up while the
  flame changes; `LanternSettings::always_show`), flame colours.
- `client/src/targeting.rs` — Tab/click/Esc targeting, target ring.
- `client/src/hud/` — hotbar (combo glow, tooltips), unit frames (class, shield,
  status chips with stack counts) + cast bar, nameplates, floating numbers, messages,
  lantern panel (L: flames, the current class's specializations, secondary),
  options menu (O, or Esc with nothing targeted: volume slider, mute, quit),
  character panel (C: levels, stats, secondary, party bonuses, worn gear, bag) + XP
  bar, secondary flame picker (`secondary.rs`, inside the lantern panel), speech box +
  zone fade (`speech.rs`), `[E]` prompt for portals, exit portals and people
  (`banner.rs`), dungeon board list (`board.rs`), conversations (`dialogue.rs`:
  E/click next, Esc skip), quest tracker + quest log J + `!`/`?` markers
  (`journal.rs`), minimap + big map M with compass, quest-gold doorways and
  people (`map.rs`). Mute is Ctrl+M.
- `client/src/toon.rs` — `ToonMaterial` (extends StandardMaterial), outline
  material (also follows skeletons of animated models),
  `ToonAssets::spawn_part()` helper. Shaders in `assets/shaders/`.
- `client/src/models.rs` — real character models (`models.ron`): loads a KayKit
  body under the character (turned to face -Z), swaps in toon materials (one set
  per character, for hit flashes) + outlines, hides the pack's hand items and
  listed parts, `Stretch` bones after animation (longer, slimmer; the head undoes
  the stretch below it), weapons on `handslot.r/.l` by class + spec, one
  `AnimationGraph` per file, animation from logic state (defeated, riding,
  attack/release one-shots from `AbilityUsed`/`AbilityLanded`, flame change,
  casting, airborne, running, idle per weapon `Style`). Body swaps on class
  change keep the old model a couple of frames (`OldModel`) so the lantern can
  move hands. `ModelPending` holds back hit-flash material collection.
- `client/src/world.rs` — `CurrentZone`, rebuilds scenery on zone change (sky/fog
  per `ground`; `"none"` = no floor), portals and exit portals, decorations, zone
  `border` dressing and `ambience` particles, hides things in other zones
  (`ElsewhereZone`).
- `client/src/props.rs` — scenery builders by `visual` key: houses, towers,
  fountain, giant root, ancient tree, pines, rocks, lamps, campfires, city wall,
  root tunnel (built along the ride path), root entrance, district gates,
  dungeon board, burrow walls/glowcaps/entrance.
- `client/src/creatures.rs` — wolves and walking mushrooms (`WolfLook`/`CapLook`
  colours, `scaled` for pups and bosses), the Rotheart, townsfolk bodies.
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
- `client/src/animation.rs` — hit flashes, `BossRig` (sway, wind-up, slam; when
  defeated it topples backwards while its ground roots draw in), `Hop`.
- `client/src/hud/banner.rs` — big banners (boss speech, victory, wipes) + portal prompt.
- `client/src/camera.rs` — FFXIV-style follow camera; stays inside zones with a
  `border` and in front of box walls; faces the arrival direction on entering a
  zone; swings behind the rider on rides; `CameraShake`.
- `server/src/main.rs` — placeholder until M11: validates data and exits.
- `client/examples/art_samples.rs` — the approved art direction as a runnable
  scene (`cargo run -p client --example art_samples`): KayKit bodies with
  stretched bones, toon materials + skinned outlines, race parts, gear tiers,
  `blade()`/`tube()` mesh helpers. Bodies are in the game now (`models.rs`);
  race parts and gear tiers still only live here.
- `assets/models/kaykit/` — KayKit Adventurers (CC0, see `CREDITS.md`).
- `docs/art-direction.md`, `docs/art/` — the agreed look, with pictures.

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
- Enemies only move if their data gives `move_speed`; aggro/assist/leash radii
  of 0 mean "never". Roaming enemies use `reset_after` only to get back up after
  defeat; alive, they leash instead.
- New characters start in `hub` (Lanternhold); "party" (synergy) is still
  everyone in the same zone until M11.
- Say **dungeon** and **trial**, never "duty" (user request, 2026-10-04).
- Quests and dialogue are data. People have a unique `id` in zone data;
  quests name people, enemies, zones and boss fights by id (checked at load).
  Talking always records progress on the server; the client only shows the
  lines, so skipping never loses anything. Quest progress is saved
  (migration 3).
- Dungeons and trials are `instanced` zones with an `exit`, a `back` portal at
  the entrance and an `exit_portal` on the last boss, so you always return to
  where you came in. Each must be enterable by walking to it in the world as
  well as from the dungeon board (`listing`). Use `Zones::get` (it understands
  `zone#n` ids), never the map directly.
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
