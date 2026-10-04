# Items

Each file is a list of items: `"item_id": (...)`. Ids must be unique
across all files.

- `slot`: Weapon, Head, Body, Hands, Feet or Ring.
- `level`: the class level needed to wear it. In a level-synced dungeon or trial,
  items above the sync level count for proportionally less.
- `class`: weapons only — the class that uses it (a file name in
  `assets/data/classes/`). Each class wears its own weapon; armour and
  rings are shared by every class.
- Stats (all optional, 0 if left out):
  - `health`: extra maximum health
  - `power`: extra Power, in percentage points
  - `crit`: extra chance of a critical hit, in percentage points
  - `guard`: less damage taken, in percent (capped in `progression.ron`)
