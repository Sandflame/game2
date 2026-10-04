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
| `combo` | `Some((after: "<ability id>", potency: 320))`: stronger right after that ability |
| `vfx` | Visual effect name (looks only) |

Effects: `Damage(potency: N)`, `Heal(potency: N)`,
`Shield(potency: N, status: "<status id>")`, `ApplyStatus(status: "<id>")`, `Taunt`.

Who (`to`): `Target`, `Myself`, `EnemiesAround(centre: Target|Me, radius: N)`,
`AlliesAround(centre: Target|Me, radius: N)` (allies always include you).

Potency 100 is about 100 damage or healing at normal power.
