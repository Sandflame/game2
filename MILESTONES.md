# Milestones

Each milestone ends with:

1. `cargo build`, `cargo clippy --all-targets -- -D warnings`, and
   `cargo test` all pass for the whole workspace.
2. Exact instructions for running and testing it.
3. A git commit, then a stop for your feedback.

I changed your list in three ways (marked **Changed**):

- Split Milestone 8 (hub + world + dungeon) into two — it was about three
  times bigger than the others.
- Started a short `HOSTING.md` at Milestone 2, so you can test with
  friends over the internet early instead of waiting until the end.
- Moved "spell effects and ground-marker art" into the boss milestone,
  since that's when they first matter.

---

## M0 — Plan *(this step)*
- `DESIGN.md`, `MILESTONES.md`, `CLAUDE.md`.
- **Done when:** you have reviewed and approved the plan.

## M1 — Workspace and a 3D scene
- Cargo workspace with `shared`, `server` (prints "hello" and exits for
  now), `client`.
- Client: window, ground plane, a few placeholder props, a capsule
  character moved with WASD + jump, FFXIV-style follow camera
  (right-drag to rotate, wheel to zoom).
- Toon shader (banded lighting + outlines) on all objects.
- Movement logic lives in `shared` as a pure function, with unit tests
  (it will be reused by the server in M2).
- Data-file loading skeleton: `assets/data/config/movement.ron`
  (walk speed, jump height) loaded and validated.
- **Test:** `cargo run -p client`, walk around, check the look.

## M2 — Server and two clients
- Headless server with lightyear (UDP). Clients enter `IP:port` and a
  display name (no password yet) in a simple connect screen.
- Your own movement is predicted; other players are interpolated.
- Disconnects/reconnects handled cleanly.
- Data-file hash check at connect.
- **Changed:** short `HOSTING.md` — run the server, open the port,
  or use a virtual LAN.
- **Test:** one server + two clients on one PC; then a friend over the
  internet. Optional "fake lag" setting to see prediction working.

## M3 — Tab targeting, GCD, hotbar, training dummy
- Training dummy entity with health that resets.
- Tab/click targeting, target frame, nameplates.
- Hotbar UI (10 slots, keys 1–0) with GCD spinner, cooldown overlays,
  cast bar, input queue, animation lock.
- 2–3 hard-coded test abilities (instant, cast-time, off-GCD) resolved
  on the server; floating damage numbers.
- GCD/cooldown/cast rules in `shared` with unit tests.
- **Test:** two players hitting the dummy; casts cancel when moving.

## M4 — Data-driven abilities, four classes, class switching
- Ability, status, and class data files; effects system (§5.3 of design).
- Blademaster/Greatsword, Elementalist/Fire, Shield Knight/Bulwark,
  Priest/Mender, each with 5 core + 3 spec abilities.
- Statuses: buffs, debuffs, DoT/HoT, shields, threat basics.
- Lantern flame switching out of combat (with a placeholder effect).
- Data validation with clear error messages.
- **Test:** switch between all four classes and use every ability on the
  dummy; heal and shield each other; edit a number in a RON file and see
  it change after restart.

## M5 — First trial boss
- Arena trial instance (enter from a test portal), party of 1–4.
- Boss with a data-driven timeline: 2–3 phases, telegraphed markers
  (circle, cone, line, donut, stack, spread), unavoidable raid-wide and
  tank-buster hits, adds, enrage.
- Threat, death, raise, wipe-and-reset, victory.
- Scaling by party size.
- **Changed:** first real effects pass: ground-marker shader, particle
  effects for abilities (bevy_hanabi), hit flashes, bloom.
- **Test:** clear the trial solo and with friends; deliberately fail
  mechanics to see damage and wipes.
- *Note:* this is the biggest combat milestone and may be delivered in two
  check-ins (5a: mechanics, 5b: effects).

## M6 — Accounts, saves, levels, gear
- SQLite (`world.db`) with automatic migrations and backup-before-upgrade.
- Register/login with argon2-hashed passwords; one character per
  account (more later if wanted).
- Save/load position, class levels, XP, gear, hotbar choices.
- XP from kills and duties; level 1–30 per class; level sync in duties.
- Basic gear: slots, item level, stats; boss loot; equip UI. Armour
  shared by all classes, weapons per class.
- **Test:** register, play, quit, restart the server, log back in with
  everything intact; copy `world.db` to another folder and run from it.

## M7 — Secondary classes and party synergy
- Two secondary ability slots chosen from another class (level-gated).
- Secondary-class level stat bonus.
- Party system (invite, leave, party list UI), supporting up to 8
  players so 8-player raids can come later.
- Role-coverage calculation and compensating bonuses from `synergy.ron`.
- **Test:** compare a solo player's bonuses with a full mixed party.

## M8 — **Changed:** Hub city and zone travel
- Magitech hub city with districts (only Rootwell open, others gated).
- Instances/zones system with rooms, loading, and spawn points.
- Rootwell's giant root and the scripted slide ride to the forest.
- Placeholder forest zone with the ancient tree.
- NPCs you can click (no quests yet).
- **Test:** walk from the hub through the root into the forest and back.

## M9 — **Changed:** The forest world and first dungeon
- Forest zone filled out: regular enemies with aggro/leash, a few camps.
- Duty finder board: queue a 4-player (scaled 1–4) dungeon.
- First dungeon, 5–10 minutes: trash, two mini-bosses, final boss, loot.
- Collision against level geometry (adds `avian3d` for collision
  queries if the levels need it).
- **Test:** run the dungeon start to finish with friends; time it.

## M10 — Quests and skippable dialogue
- Quest and dialogue data files; quest log and tracker UI.
- Starter story chain from hub → root → forest → dungeon.
- Skip key; progress saved in the database.
- **Test:** play the story chain as a new character.

## M11 — Remaining specializations
- The other 10 specs, added as data (plus any new effect types they need).
- Spec-switching UI.
- **Test:** every spec usable; balance check on the dummy and trial.

## M12 — Hosting guide and release build
- Full `HOSTING.md` for your friends: Windows server setup, port
  forwarding, virtual LAN option, backing up/moving `world.db`,
  running on Linux.
- Release build script producing a zip for server and client.
- **Test:** a friend follows the guide without your help.

---

## After M12 (from your "later" list)
Raids (beyond the trial-style boss), 8-player raids, gathering/crafting classes, pets,
mounts, cosmetics, trading, duels/PvP, more classes, customizable
hotbars, fusion abilities. Each becomes its own milestone when we get
there.
