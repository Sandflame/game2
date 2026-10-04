# Lanternflame — Design Document

> Status: **approved plan** (open questions answered 2026-10-03).

This document describes *what* we are building and *how the pieces fit
together*. `MILESTONES.md` describes the *order* we build it in.
`CLAUDE.md` holds day-to-day commands and coding conventions.

---

## 1. The game in one paragraph

A small online RPG for 4–8 friends. You log in to a magitech hub city,
pick a flame for your lantern (which sets your class), team up, and run
short instanced dungeons, single-boss trials, and harder raids. Combat is
tab-target in the style of Final Fantasy XIV: pick a target, press hotbar
buttons, respect a 1.5 s global cooldown, and dodge glowing ground markers
from bosses. Levelling is fast (cap 30 per class). After the cap, power
comes from gear first and secondary-class levels second. Everything
tunable lives in data files so balancing never needs a recompile.

---

## 2. Verified technology choices (checked on crates.io, 2026-10-03)

| Purpose | Crate | Version | Why |
|---|---|---|---|
| Engine | `bevy` | **0.19.1** | Latest stable. Requires Rust ≥ 1.95 (this machine has 1.97). |
| Networking | `lightyear` | **0.30.1** | Depends on Bevy 0.19. The only maintained Bevy networking crate with **built-in client prediction, rollback and interpolation**, which you asked for. Also has "rooms" (who-sees-what), which we use for instances. |
| Physics / collision | `avian3d` | **0.7.0** | Depends on Bevy 0.19; lightyear has an official `avian3d` integration feature. **Not used until we need it** — see §2.1. |
| Database | `rusqlite` (feature `bundled`) | **0.40.2** | SQLite compiled into the program, so no install step on Windows or Linux. One `.db` file holds everything. |
| Password hashing | `argon2` | **0.6.0** | The current recommended password hash. |
| Data files | `serde` + `ron` | 1.0.229 / **0.12.2** | RON ("Rusty Object Notation") reads like Rust structs and allows comments, which matters for hand-edited balance files. |
| Menus / debug UI | `bevy_egui` | **0.42.0** | Depends on Bevy 0.19. Quick to build login screens and debug panels. In-game HUD (hotbar, cast bars, party list) uses Bevy's own UI. |
| Particles | `bevy_hanabi` | **0.19.0** | Depends on Bevy 0.19. GPU particles for spell effects. Added in Milestone 4b (`3d` feature only). Presets in `assets/data/client/particles.ron`. |

Rejected alternatives, briefly:

- **`bevy_replicon` (0.44.2)** — excellent and simpler than lightyear, but
  has no built-in prediction or interpolation. We would have to write
  those ourselves, which breaks your "no custom netcode" rule.
- **`bevy_rapier3d` (0.36.0)** — fine physics engine, but lightyear's
  ready-made integration is for avian.
- **`sqlx`** — async database library. More moving parts than we need for
  one small SQLite file.

**Version policy:** versions are pinned exactly in the workspace
`Cargo.toml`. Bevy makes breaking changes every release (roughly every
3–4 months), so we only upgrade between milestones, as a deliberate step,
and only when every crate above has a compatible release.

### 2.1 Plain-language note: why no physics engine at first

A tab-target RPG does not need bouncing boxes or ragdolls. Characters
walk, jump, and stop at walls. Boss attacks are shapes on the ground
(circle, cone, line, donut) and checking "is this player inside the
circle?" is a few lines of maths, not physics.

The hard part of online movement is *prediction*: your PC runs your
movement immediately, and if the server disagrees it rewinds and replays.
Rewinding a full physics simulation is the single most fragile part of
networked games. Rewinding our own small movement function is easy and
reliable.

So: movement is a plain function in `shared` (`fn step_movement(state,
input, dt, world_geometry) -> state`) that both the server and the client
run. Level collision starts as "flat ground plus simple box/cylinder
walls". If and when levels need real collision against complex meshes
(around Milestone 8), we add `avian3d` **only for collision queries**
("would this move hit a wall?"), not for simulating bodies.

---

## 3. Architecture

### 3.1 The three crates

```
game2/
├── Cargo.toml          workspace: pins all versions
├── shared/             pure game logic + network protocol (no rendering)
├── server/             headless program (no window, no GPU)
├── client/             window, rendering, input, UI, audio
└── assets/
    ├── data/           RON data files: abilities, classes, bosses, items, quests…
    ├── shaders/        WGSL shaders (toon, outlines, ground markers)
    ├── models/ textures/ audio/ fonts/
```

- **`shared`** — Components, messages, and *game rules*: damage formulas,
  cooldown tracking, XP curves, ability resolution, boss timelines,
  ground-marker hit tests, party-synergy rules, data-file definitions and
  validation. Everything here gets unit tests. It never touches rendering.
- **`server`** — The *rules half* ("authority"): a library with an
  `AuthorityPlugin` that owns the truth — positions, health, cooldowns,
  enemy AI, loot, the database — plus a headless program (`MinimalPlugins`,
  no window) that will run it for multiplayer. Runs on Windows and Linux
  from the same code.
- **`client`** — Sends inputs, renders what the server says, predicts the
  local player's movement, interpolates everyone else, shows UI and
  effects.

**Rule:** the client never decides outcomes. It can *show* a cast bar
instantly (so the game feels responsive), but the server decides whether
the cast happened, whether it hit, and for how much.

### 3.2 Logic and visuals are separate

Game entities carry only logic components (`Health`, `Position`,
`CastState`, `ClassId`…). The client *adds* visual components when an
entity appears (a mesh, a material, an effect). Ability and boss data
refer to visuals by name (`vfx: "fire_bolt"`), and a separate
client-only file maps those names to actual effects. Upgrading art means
editing the client's visual mapping and assets; game logic is untouched.

### 3.3 Single-player first: the in-process "link"

Multiplayer is built late (Milestone 11), so until then the client
program runs the authority **inside itself**. The two halves still only
talk through messages:

- The client puts `ClientRequest`s (join, movement input, use ability)
  into a `Link` outbox. The authority drains it every tick.
- The authority puts `ServerEvent`s (damage dealt, action rejected,
  cast started/interrupted) into the `Link` inbox. The client drains it
  every frame for floating numbers and messages.
- The client may **read** logic components (health, positions, cast
  state) — later these arrive by network replication — but **never
  changes them**.

Adding multiplayer means replacing the in-process `Link` with lightyear:
requests and events become network messages, and logic components are
replicated. Movement prediction for your own character is the main new
work at that point.

### 3.4 Networking model (Milestone 11)

- **Transport:** UDP via lightyear's netcode.io implementation. Clients
  connect by typing `IP:port`.
- **Tick:** the server simulates at a fixed **60 Hz** and sends state
  updates at **~20 Hz** (both configurable). Small player counts make
  this cheap.
- **Your own character:** *predicted*. Your keyboard input moves you on
  your screen immediately; the same input is sent to the server; if the
  server's result differs, the client quietly corrects (rollback and
  replay, handled by lightyear).
- **Other players and enemies:** *interpolated*. You see them ~100 ms in
  the past, smoothly sliding between known positions. This is what makes
  other people's movement look smooth instead of jittery.
- **Combat actions:** *not* predicted. Pressing an ability sends a request;
  the client immediately starts the GCD spinner and cast bar optimistically,
  and rolls back the UI if the server rejects it. Damage numbers appear
  when the server confirms. This is how FFXIV itself works and it keeps
  combat simple and cheat-proof.
- **Telegraph fairness:** a ground marker resolves on the server when its
  timer ends, using the server's view of your position. With friends on a
  normal connection (< 150 ms) this feels fair. We add a small grace
  margin (data-tunable, e.g. 0.15 m) at marker edges to forgive latency.

### 3.5 Instances and zones

The game is **separate zones**, not a seamless world. One server process
runs everything:

- Each zone or dungeon run is an **instance** with an `InstanceId`.
  Entities belong to exactly one instance.
- lightyear **rooms** make sure you only receive entities from your
  current instance.
- Game systems only let entities interact within the same instance.
- Moving between zones = server moves your entity to a new instance and
  the client loads the new scene behind a short loading screen.

With 4–8 players this all fits in one process with plenty of headroom.

### 3.6 Login and security (honest version)

- Username + password. Passwords are hashed with **argon2** before being
  stored; the server never stores the actual password.
- The connection is encrypted by netcode.io, but the key is built into the
  game program, so this is **"friends-and-family" security, not bank
  security**. It stops casual snooping, not a determined attacker who has
  your client program. Tell your friends not to reuse an important
  password. For a private game among friends this is a reasonable
  trade-off; doing proper TLS-style key exchange is much more work and
  can be added later if the game ever goes public.
- The server rejects clients whose game data files don't match its own
  (compared by a hash at connect time), so nobody accidentally plays with
  different ability numbers.

### 3.7 Persistence

- One SQLite file, `world.db`, next to the server program (path
  configurable). Copy the file to move the world to another PC.
  Until multiplayer (M11) the game keeps it in the player's data folder:
  `%APPDATA%\Lanternflame\world.db` on Windows,
  `~/.local/share/lanternflame/world.db` on Linux (`LANTERNFLAME_DB` overrides).
- Database work runs on a background thread so a slow disk never freezes
  combat.
- Characters are saved on logout, whenever their progress changes (level,
  loot, gear, class, zone), and every 60 seconds (for their position).
- Schema changes are applied automatically at startup using numbered
  migration steps tracked in SQLite's `user_version`. Old `world.db` files
  upgrade themselves; the server makes a backup copy first.
- Linux vs Windows: rusqlite with `bundled` compiles SQLite into the
  program, and we only use `std::path` for file paths, so no code changes
  are needed between platforms.

### 3.8 Hosting reality check

Running the server on your Windows PC works, but **your friends can only
reach it if your router forwards the UDP port** to your PC (and Windows
Firewall allows it). This is the most common thing that goes wrong. The
hosting guide will cover:

1. Port forwarding (the "normal" way), and
2. a no-router-config alternative: a free virtual LAN tool (e.g.
   Tailscale or ZeroTier). Everyone installs it, and you connect using the
   host's virtual IP. Often easier for friend groups.

`HOSTING.md` arrives with accounts in Milestone 12.

---

## 4. Data files

All tunable numbers live in `assets/data/` as RON files. Both server and
client load the same files (server for the rules, client for tooltips and
UI). Data is validated at startup: unknown ability IDs, missing classes,
negative cooldowns, etc. stop the server with a clear error message
naming the file and the problem.

Example ability (illustrative, final shape decided in Milestone 4):

```ron
// assets/data/abilities/blademaster.ron
[
    (
        id: "bm_cleave",
        name: "Cleave",
        description: "A heavy swing at your target.",
        on_gcd: true,
        cast_time: 0.0,          // seconds; 0 = instant
        cooldown: 0.0,           // own cooldown, separate from the GCD
        range: 3.5,              // metres
        target: Enemy,
        effects: [
            (effect: Damage(amount: 220)),
        ],
        combo: Some((after: "bm_slash", amount: 320)),
        vfx: "slash_heavy",      // looked up by the client only
        icon: "icons/bm_cleave.png",
    ),
]
```

Planned data files (each arrives in the milestone that needs it):

| File(s) | Contents |
|---|---|
| `config/combat.ron` | GCD length (1.5 s), animation lock, grace margins, crit/damage formula constants |
| `abilities/*.ron` | Every ability: timing, range, targeting, effects, visuals key |
| `statuses.ron` | Buffs/debuffs: duration, stacks, stat modifiers, damage-over-time |
| `classes/*.ron` | Class: role lean, base stats per level, the 5 core abilities, specializations (3 abilities each), which abilities may be lent as secondary |
| `progression.ron` | XP per level (cap 30), XP from kills/duties/quests |
| `synergy.ron` | Role-coverage rules and compensating bonuses |
| `items/*.ron` | Gear: slot, item level, stats |
| `enemies/*.ron` | Mobs and bosses: stats, ability lists, AI timeline |
| `duties/*.ron` | Dungeons, trials, raids: map, encounters, loot tables, timers |
| `zones/*.ron` | Zones: scene, spawn points, exits/portals, NPCs |
| `quests/*.ron`, `dialogue/*.ron` | Quest steps and dialogue lines |
| `client/vfx.ron` *(client only)* | Maps visuals keys to particle/shader effects |

Later: hot-reloading data while the server runs (handy for balancing).

---

## 5. Combat

### 5.1 Targeting and hotbar
- **Tab** cycles hostile targets in front of you, nearest first; click
  selects; **Esc** clears; **F1–F4** target party members (heals).
- Hotbar of 10 slots, keys **1–0**. Layout is fixed in v1:
  slots 1–5 core class, 6–8 specialization, 9–10 secondary class.
  Stored as a list in the data model so customizable hotbars later are a
  UI change, not a redesign.

### 5.2 Timing rules
- **Global cooldown (GCD):** 1.5 s, shared by all "weaponskills/spells".
- **Off-GCD abilities (oGCD):** have their own cooldown, can be woven
  between GCDs. A short **animation lock** (≈0.6 s, data-tunable) after
  any action stops infinite button mashing.
- **Cast times:** moving cancels a cast (FFXIV rule). Instant abilities
  can be used while moving.
- **Input queue:** pressing an ability in the last 0.5 s of the GCD
  queues it (feels much better online). Tunable.

### 5.3 Effects (the building blocks)
Abilities are lists of **effects**, so new abilities are new data, not
new code. Initial effect types:

`Damage`, `Heal`, `Shield` (absorb), `ApplyStatus` (buff/debuff/DoT/HoT/
slow), `Dash` (move self), `Knockback`, `Taunt`/`Threat`, `GroundAoE`
(spawn a zone), `ResetCooldown`, `Resource` (gain/spend class resource).

Each effect has a **target selector** (self, target, party within X m,
cone in front, enemies in circle…). If a future ability needs a genuinely
new behaviour, we add one new effect type in code; everything else stays
data.

### 5.4 Stats and formulas
Small and readable on purpose:

- Primary: **Power %** (damage and healing; 100% = listed amounts), **Vitality** (max HP).
- Secondary: **Crit**, **Haste** (shortens GCD and casts), **Guard**
  (damage reduction).
- **Plain numbers, not "potency" (decided 2026-10-04):** abilities list the
  damage/healing they do at 100% power. `damage = amount × power% × buffs`,
  then ×1.5 on a critical hit. Tooltips show the final numbers. Crit chance
  and size are in `config/combat.ron`.
- **Stat sources are a list** (`base from class level`, `gear`,
  `secondary class bonus`, `party synergy`, `status effects`, and later
  `pet`, `fusion`). Pets slot in later as just another source.

### 5.5 Threat
Enemies attack whoever has the most threat. Durable classes generate
extra threat. Because roles are *soft*, if nobody is durable, bosses
spread attacks or use more party-wide damage, and the synergy system
gives everyone extra mitigation (§6.3).

### 5.6 Death
Downed players can be raised by sustain classes (and by any class via a
long-cooldown emergency ability, so no party mix is required). In a duty,
a full wipe resets the encounter; players return to the arena entrance
with no penalty other than time. No XP loss, no gear damage —
low frustration.

---

## 6. Classes, specializations, roles

### 6.1 The lantern
Your character carries a lantern. The **flame** in it is your class.
Switching flame = switching class: allowed anywhere **out of combat**,
takes ~2 s with a visual effect. Each class has its own level (1–30) and
its own saved hotbar/spec choice. **Armour and accessories are shared by
all classes; weapons are not** — each class has its own weapon slot, and
switching flame swaps to that class's weapon automatically.

The lantern is **not visible by default** (decided 2026-10-04). When you
change flame, your character holds it out in both hands, the flame
changes colour, and it is put away again a moment later. A "show lantern at
all times" option (lantern panel) keeps it at your side. Later it may be
shown as part of gear or in other ways.

### 6.2 First four classes

| Class | Lean | Specializations (★ = built first) |
|---|---|---|
| **Blademaster** | Damage | ★ Greatsword · Scissors · One-handed sword · Dual blades |
| **Elementalist** | Damage | ★ Fire (damage) · Ice (shields & slows) · Storm (burst) |
| **Shield Knight** | Durable | ★ Bulwark (defense) · Oath (holy, light healing) · Vanguard (damage) |
| **Priest** | Sustain | ★ Mender (healing) · Warden (barriers) · Judge (damage that heals) |

The ★ picks are the "most classic" version of each class, so the first
playable party covers all three role leans.

Each class defines in data: role lean, base stats, 5 core abilities,
specializations (3 abilities each), and a short list of abilities that
may be borrowed as a **secondary class** (2 slots). Spec abilities can
also *modify* core abilities (e.g. Fire spec makes "Bolt" apply a burn) —
expressed as data overrides.

### 6.2b Lantern abilities (decided 2026-10-04)
Every class also has two shared abilities, so no party mix (or solo
player) is stuck: **Second Wind** (heal yourself, long cooldown) and
**Rekindle** (raise a fallen friend). They live in `config/player.ron`
(`shared_abilities`). Since M6 the hotbar has 12 slots: 1–8 class, 9 and 0
borrowed from the secondary class, `-` and `=` the lantern abilities.

### 6.3 Party synergy
Each class+spec has role weights (e.g. Shield Knight Oath = 0.7 durable,
0.3 sustain). The server sums the party's coverage per role. If a role is
under-covered, everyone gets a compensating bonus from `synergy.ron`:

- No sustain → everyone gains a small heal-over-time after each GCD, and
  potions/emergency heals recharge faster.
- No durable → everyone takes less damage, bosses spread threat.
- No damage → everyone deals more damage.

Bonuses are smaller than having the role, so mixed parties are rewarded
but never required. Solo players count as a "party of one" and get all
relevant bonuses (this also makes solo questing pleasant).

### 6.4 Levelling and progression
- Cap 30 per class. XP curve designed so a class reaches 30 in a few
  hours (numbers in `progression.ron`).
- Secondary-class abilities require that class at a minimum level;
  higher secondary-class level gives a small stat bonus to your main
  class. This is the "second source of power" at cap.
- Gear is the main source of power at cap: item level from dungeons,
  trials, raids. Armour/accessories are shared across classes; each class
  has its own weapon (weapons are class-specific items).
- Duties have **level sync**: if you are over-levelled you are scaled
  down, so friends at different levels can always play together.

---

## 7. Enemies and bosses

### 7.1 Boss timelines (data-driven)
Bosses run a **timeline** like FFXIV encounters, written in data:

```ron
phases: [
    ( until_hp: 0.60, timeline: [
        (at: 3.0,  ability: "root_slam"),      // telegraphed circle on a player
        (at: 8.0,  ability: "bark_burst"),     // unavoidable party damage
        (at: 14.0, ability: "thorn_cone"),     // telegraphed cone
        (loop_to: 0.0, at: 20.0),
    ]),
    ( until_hp: 0.0, on_enter: "summon_saplings", timeline: [ … ]),
]
```

- **Telegraphed attacks:** ground markers (circle, cone, line, donut,
  "stack on player", "spread from player"), with a visible fill timer.
  Dodge or take heavy damage.
- **Unavoidable attacks:** party-wide or tank-buster damage that tests
  healing/shields/mitigation.
- Phase changes at HP thresholds, adds, enrage timer.
- Marker hit-tests are pure geometry functions in `shared` with unit tests.

### 7.2 Regular enemies
Simple state machine: idle → aggro on proximity → chase → use abilities →
leash back if pulled too far. Data defines their abilities and stats.

---

## 8. World and content

### 8.1 The hub city
A magitech city split into **districts**. Each district takes on the look
of the world it leads to and has a unique way there:

| District | World | Travel |
|---|---|---|
| **Rootwell** | Grassland & forest (starter) | Walk into the giant root, slide down its tunnels |
| Harbour | (later) | Boat |
| Railyard | (later) | Train |
| Skydock | (later) | Spaceship |
| Clockwork Quarter | (later) | Time machine |

Only Rootwell is built in this plan; others are placeholders (closed
gates) so the layout is ready.

**The root slide** is a short scripted ride along a fixed path
(a spline), not physics: you walk in, the camera follows you down a
swirling tunnel for ~8 s, the next zone loads during the ride, and you
pop out at the foot of the ancient tree. This hides the loading screen
and is much simpler than real sliding physics.

### 8.2 Content types
| Type | Players | Length | Structure |
|---|---|---|---|
| Dungeon | 4 | 5–10 min | Short path, 2 trash pulls, 2 mini-bosses, 1 final boss |
| Trial | 1–4 | 3–8 min | One multi-phase boss in an arena |
| Raid | 4 | 10–15 min | Harder trial-style bosses; tighter mechanics, enrage timers |

All duties scale by party size so you can practise alone or with
whoever is online. **Raids are 4 players for now; 8-player raids are
planned.** To keep that door open: parties hold up to 8 players, each
duty's data declares its `max_party` (4 today), scaling formulas accept
1–8, and the party-list UI and synergy rules work for any size.

### 8.3 Quests and dialogue
Quests are lists of steps in data (`talk to NPC`, `kill N`, `enter zone`,
`complete duty`, `interact with object`). Dialogue is a list of lines
with speaker names and optional choices. Any player can skip dialogue
with one key; quest progress still records.

---

## 9. Art direction

- **Toon shading:** a custom material that extends Bevy's standard
  material: lighting is snapped into 2–3 bands (lit / shadow / rim),
  plus black outlines drawn with the "inverted hull" trick (a slightly
  bigger copy of the mesh drawn inside-out in black). Cheap, robust, and
  looks anime.
- **Characters:** simple placeholders (capsules with a head and a lantern)
  at first; later free CC0 low-poly characters (e.g. from Quaternius or
  KayKit packs) recoloured.
- **Effort goes into effects:** GPU particles (`bevy_hanabi`), animated
  ground-marker shaders (pulsing edges, filling sweep), bloom, coloured
  point lights on spells, screen shake on big hits.
- Each visual effect is registered under a name (`client/vfx.ron`), so
  replacing a placeholder never touches gameplay code.

---

## 10. Planned for later — and how we avoid blocking it

| Future feature | What we do now so it's easy later |
|---|---|
| Gathering & crafting classes | Class definitions already have a `kind` field (`Combat` now, `Gathering`/`Crafting` later). Items have an open-ended `category`. |
| Pets (hatch, breed, talents) | Stats come from a list of sources (§5.4); a pet is another source plus an entity that can use abilities from data. |
| Mounts | Movement speed is a stat; movement function already takes a "mode". |
| Cosmetic appearance | Gear has separate `stats` and `appearance` fields from day one. |
| Trading | Items are database rows with unique IDs (no copy bugs when moved). |
| Duels / arena PvP | Hostility is a rule function (`can_harm(a, b)`), not hard-coded "players vs monsters". |
| More classes | Classes are data. |
| Customizable hotbars | Hotbar layout is already a saved list. |
| Fusion abilities | Abilities can declare `requires: [class/spec conditions]`. |

---

## 11. Things that are harder than they sound (and what we do instead)

1. **"Raids doable with 4"** — fine, but tuning for 1–4 players *and* still
   hard is real work. We scale HP/damage by party size from data and
   expect to tune by playing.
2. **Hosting from a home PC** — router port forwarding is the main hurdle.
   Mitigation: virtual-LAN alternative in the hosting guide (§3.8).
3. **Prediction + physics** — avoided by keeping movement our own small
   shared function (§2.1).
4. **Sliding down the root** — done as a scripted ride, not physics (§8.1).
5. **All four specializations per class** — that's 14 specs. We build one
   per class first and save the rest for Milestone 11 (as you planned).
6. **Bevy upgrades** — every 3–4 months Bevy breaks things. We pin
   versions and upgrade only between milestones.
7. **Compile times** — Bevy is big. First build takes several minutes;
   later builds are fast. We set dependency optimisation so the game runs
   smoothly even in debug builds.

---

## 12. Decisions (answered 2026-10-03)

1. **Name:** Lanternflame.
2. **Server tick:** 60 Hz simulation, ~20 Hz network updates.
3. **Gear:** armour and accessories shared by all classes; weapons are
   per class.
4. **Jumping:** yes — simple jump, no fall damage.
5. **Camera:** FFXIV-style (right-drag rotates camera and turns the
   character, left-drag orbits the camera only, WASD moves, wheel zooms).
6. **Party size:** duties are 4 players now; 8-player raids planned later
   (parties already support 8).
