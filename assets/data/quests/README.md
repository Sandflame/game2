# Quests

One quest per file; the id is the file name.

- `giver`: the person who offers it (their `id` in zone data). Talk to them
  when the quest is available and it is offered and taken on at once.
- `offer`: the dialogue played then (`assets/data/dialogue/`).
- `after`: quests that must be finished first.
- `steps`, done in order. Each has a `goal`, the `text` shown in the quest
  tracker, and (for `Talk` steps) an optional `dialogue`:
  - `Talk("<person id>")` — talk to them.
  - `Defeat(enemy: "<enemy>", count: 3)` — defeat enemies of that type.
  - `Reach("<zone>")` — go to that zone.
  - `Win("<encounter>")` — win that boss fight.
- `xp`: experience for finishing (for the class being played).
- `items`: `[(item: "<id>", chance: 100)]` for finishing.

People who have a quest to offer show a gold `!` over their head; people a
quest wants you to talk to show a gold `?`.
