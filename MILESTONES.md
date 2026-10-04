# Milestones

Each milestone ends with:

1. `cargo build`, `cargo clippy --all-targets -- -D warnings`, and
   `cargo test` all pass for the whole workspace.
2. Exact instructions for running and testing it.
3. A git commit, then a stop for your feedback.

**Order changed on 2026-10-04 (your request): single-player content first,
multiplayer near the end.** To keep multiplayer easy to add later, the game
already runs as two halves inside one program — the *rules* half (the
future server) and the *screen* half (the client) — talking only through
messages. See DESIGN.md §3.3.

Earlier changes to your original list:
- Split "hub + world + dungeon" into two milestones (it was about three
  times bigger than the others).
- Spell effects and ground-marker art moved into the boss milestone,
  since that's when they first matter.

---

## M0 — Plan ✅
- `DESIGN.md`, `MILESTONES.md`, `CLAUDE.md`.

## M1 — Workspace and a 3D scene ✅
- Cargo workspace with `shared`, `server`, `client`.
- Meadow with props, capsule character, WASD + jump, FFXIV-style camera.
- Toon shader (banded lighting + outlines).
- Movement rules in `shared` with unit tests; movement numbers in data.

## M2 — Tab targeting, global cooldown, hotbar, training dummy ✅
- **Rules/screen split:** game rules move into the `server` crate as a
  plugin the client runs in-process; the client only sends requests
  (move, use ability) and reads results.
- Training dummies placed by zone data; they reset when left alone.
- Tab/click targeting, Esc to clear, target ring, target frame, nameplates.
- Hotbar UI (10 slots, keys 1–0 or click) with GCD sweep, cooldowns,
  out-of-range tint, cast bar, input queue, animation lock.
- Three test abilities from a data file (instant GCD, cast-time GCD,
  off-GCD with cooldown); floating damage numbers; "not ready" messages.
- GCD/cooldown/cast/queue rules in `shared` with unit tests; a
  headless test that plays through the rules end to end.
- **Test:** hit the dummy; casts cancel when you move.

## M3 — Data-driven abilities, four classes, class switching ✅
- Ability, status, and class data files; effects system (DESIGN.md §5.3).
- Blademaster/Greatsword, Elementalist/Fire, Shield Knight/Bulwark,
  Priest/Mender, each with 5 core + 3 spec abilities.
- Statuses: buffs, debuffs, DoT/HoT, shields, threat basics.
- Lantern flame switching out of combat.
- Added: a sparring dummy that hits back (to test healing, shields and
  mitigation), critical hits, combos, simple defeat/auto-revive.
- **Test:** switch between all four classes and use every ability.

## M4 — First trial boss
- Arena trial (enter from a test portal), scaled for 1–4 players.
- Boss with a data-driven timeline: 2–3 phases, telegraphed markers
  (circle, cone, line, donut, stack, spread), unavoidable raid-wide and
  tank-buster hits, adds, enrage.
- Threat, death, raise, wipe-and-reset, victory.
- First real effects pass: ground-marker shader, particles
  (bevy_hanabi), hit flashes, bloom.
- **Test:** clear the trial solo; deliberately fail mechanics.
- *May be delivered in two check-ins (4a mechanics, 4b effects).*

## M5 — Levels, XP, gear, saving
- SQLite (`world.db`) with automatic upgrades and backup-before-upgrade.
- Save/load your character (no login yet — one local character).
- XP from kills and duties; level 1–30 per class; level sync in duties.
- Gear: slots, item level, stats; boss loot; equip UI. Armour shared by
  all classes, weapons per class.
- **Test:** play, quit, relaunch, and everything is still there.

## M6 — Secondary classes and party synergy
- Two secondary ability slots from another class (level-gated).
- Secondary-class level stat bonus.
- Role-coverage rules and compensating bonuses from `synergy.ron`
  (tested solo as a "party of one" until multiplayer arrives; the
  rules themselves get unit tests for every party mix).

## M7 — Hub city and the first world
- Magitech hub city with districts (only Rootwell open).
- Zones/instances system, loading, spawn points.
- The giant root and the scripted slide ride to the forest.
- Forest zone: regular enemies with aggro/leash, camps, NPCs.

## M8 — First dungeon
- Duty board to enter a dungeon (scaled 1–4).
- 5–10 minute dungeon: trash, two mini-bosses, final boss, loot.
- Collision against level geometry (`avian3d` for collision queries,
  only if the levels need it).

## M9 — Quests and skippable dialogue
- Quest and dialogue data files; quest log and tracker UI.
- Starter story chain: hub → root → forest → dungeon.

## M10 — Remaining specializations
- The other 10 specs, added as data (plus any new effect types).
- Spec-switching UI.

## M11 — Multiplayer
- Headless server program using lightyear (UDP).
- Clients enter `IP:port` and a name; the in-process rules half is
  swapped for the network connection.
- Your own movement predicted, others interpolated; parties (up to 8).
- Everything from M2–M10 works with friends.
- **Test:** one server + two clients on one PC, then a friend over the internet.

## M12 — Accounts and hosting
- Register/login with argon2-hashed passwords; characters per account.
- `HOSTING.md`: running the server on Windows, port forwarding, the
  virtual-LAN alternative, backing up/moving `world.db`, Linux.
- Release build script producing zips for server and client.
- **Test:** a friend follows the guide without your help.

---

## After M12 (from your "later" list)
Raids beyond trial-style bosses, 8-player raids, gathering/crafting
classes, pets, mounts, cosmetics, trading, duels/PvP, more classes,
customizable hotbars, fusion abilities.
