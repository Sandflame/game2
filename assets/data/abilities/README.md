# Ability files

Each `.ron` file is a list of abilities. Fields:

| Field | Meaning |
|---|---|
| `id` | Unique name used by classes and combos |
| `name`, `description` | Shown in tooltips |
| `on_gcd` | `true`: uses the 1.5 s global cooldown. `false`: off-GCD, weave between GCDs |
| `cast_time` | Seconds; leave out (or 0) for instant. Moving cancels a cast |
| `cooldown` | The ability's own cooldown in seconds |
| `range` | Metres to the edge of the target's ring |
| `target` | `Enemy`, `Ally` (lands on you if no friendly target), or `Myself` |
| `effects` | A list of `(to: <who>, effect: <what>)`; `to` defaults to the target |
| `combo` | `Some((after: "<ability id>", amount: 320))`: stronger right after that ability |
| `vfx` | Visual effect name (looks only) |

Effects: `Damage(amount: N)`, `Heal(amount: N)`,
`Shield(amount: N, status: "<status id>")`, `ApplyStatus(status: "<id>")`, `Taunt`.

Who (`to`): `Target`, `Myself`, `EnemiesAround(centre: Target|Me, radius: N)`,
`AlliesAround(centre: Target|Me, radius: N)` (allies always include you).

## How numbers work

The amounts are what the ability does at **100% power**. Each class has a
Power percentage (in `assets/data/classes/`): at 110% power, `Damage(amount: 200)`
deals 220. Buffs multiply on top (e.g. +20% damage dealt), and a critical hit
does 50% more (see `crit_chance` / `crit_multiplier` in `config/combat.ron`).
In-game tooltips show the final numbers for your current class and buffs.
