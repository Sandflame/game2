# Art direction (decided 2026-10-05)

How characters, races and gear should look. Sample pictures are in
`docs/art/`; they were rendered by `cargo run -p client --example art_samples`
(source: `client/examples/art_samples.rs`).

## What the user wants (their words, summarised)

- Characters that look like **real MMO characters**, not capsules. Regular
  sized, like FFXIV — not chibi. Aesthetic goals named: Blade & Soul,
  FFXIV, Black Desert, Dungeon Fighter Online, Lost Ark.
- **Gear and weapons are the most important part.** Players should feel
  epic; getting cool-looking gear should be exciting. References: FFXIV's
  Ultima gear and other rare late-game gear, and Roblox's *Dungeon Quest*
  (simple shapes that still look impressive).
- **Rejected:** VRoid / VRM anime models ("vtuber/vrchat look, faces that are
  obviously decals"). Don't propose them again.
- **Liked:** the KayKit packs, but less chibi.

## The decision

1. **Bodies: KayKit Adventurers (CC0)**, made longer and slimmer in code
   (`docs/art/proportions.png`, right-hand knight). The user approved this
   after two rounds:
   - Round 1 was "too oval / rotund": fixed by narrowing hips, waist and chest,
     and longer legs.
   - The head must stay round: the head bone undoes the squash and stretch of
     the bones below it (see `stretch_for` in the sample).
   - The numbers (sample `LEVELS[3]`): model scale 0.7, head 0.5, legs ×3.1,
     arms ×1.75, spine ×1.35, chest ×1.12, slim 0.84. Apply them after
     animation each frame, as fixed bone scales. They go in a data file when
     this is built for real; no magic numbers.
   - Honest limit: stretching can't reach true FFXIV bodies, because KayKit's
     clothes keep their chunky shapes. Better bodies can be swapped in later
     (Blender is the user's side project). Everything below is built so a body
     swap doesn't throw any of it away.
2. **The game's toon shading on everything**, plus outlines on animated
   (skinned) meshes: `assets/shaders/outline_skinned.wgsl` is the game's
   outline shader with skinning support. Merge it into `outline.wgsl` when
   this is built for real.
3. **Races are cosmetic parts on top of the same body** (`docs/art/races-*.png`):
   - **Humans:** no extra parts.
   - **Elves** (elegant, blood/high elf style): long pointed ears.
   - **Drakes** (like FFXIV's Au Ra): horns that grow from the head and sweep
     back along it; scales on the cheeks; a thick scaly dragon tail with a row
     of spines that curls up at the end.
   - **Demons** (like succubi / TERA's Castanic): horns that rise and curve
     forward; a long, thin S-shaped tail ending in a heart-shaped spade.
   - **Felari** (cat folk, like FFXIV's Miqo'te; the name was chosen on
     2026-10-05 so it is our own): cat ears on top of the head (fur outside,
     pink inside) and a long tail that curls upward, both in the hair colour,
     with a pale tail tip.
   - Horns must touch the head (rooted in it, never floating). Tails start at
     the hips. Capes cover tails, so tailed races need capes that make room
     (the sample hides the demon's cape for now).
   - A very minor racial bonus might come later. For now races are cosmetic
     only.
4. **Five gear tiers get bigger, brighter and more alive** (`docs/art/gear-*.png`).
   This is where most of the art effort goes:
   - **Common:** the pack's own weapons and armour.
   - **Uncommon** (added 2026-10-05): a touch of green. Lightly tinted armour,
     small plain shoulder caps, a sturdier steel sword with a green grip and
     pommel. No glow yet.
   - **Rare:** recoloured armour, shoulder plates with gold trim, a sword with
     a gold guard and a glowing gem.
   - **Epic:** dark armour with glowing trim, spiked shoulders, a large
     greatsword with a glowing channel down the blade, sparks around the feet.
   - **Legendary:** gold armour, wings of floating light-blades, a spinning
     halo, a greatsword whose edges float apart from a glowing core with a
     spinning ring of light, motes circling the feet.
   - What makes gear exciting: silhouette size, glow (emissive + bloom),
     things that float, spin and sparkle, and colour per tier. Next steps:
     particle trails, swing effects, glow that pulses.
   - Armour on KayKit bodies is painted into four body parts (head, body,
     arms, legs). Variety comes from mixing parts across the pack's
     characters, recolouring them, and adding separate pieces on top
     (shoulders, wings, halos, capes, belts). Truly new armour shapes need new
     models later.
   - Weapons attach to the `handslot.l` / `handslot.r` bones. The pack's
     weapons are in `assets/models/kaykit/*.gltf`. The rare, epic and
     legendary weapons in the sample are built in code (`blade()` and `tube()`
     mesh helpers) and are a bit thinner than KayKit's chunky style; make
     them chunkier to match.

## Files

- `assets/models/kaykit/` — KayKit Character Pack: Adventurers 1.0 by Kay
  Lousberg (CC0, `LICENSE.txt`; credit appreciated, see `CREDITS.md`).
  Characters: Knight, Mage, Rogue, Rogue_Hooded, Barbarian (`.glb`, each with
  76 animations: idle, walk, run, 1H/2H/dual-wield attacks, spellcasting,
  block, dodge, hit, death, sit…). Weapons and shields: `.gltf` + `.bin` +
  texture.
- Skeleton bones: `root, hips, spine, chest, head, upperarm/lowerarm/wrist/
  hand/handslot .l/.r, upperleg/lowerleg/foot/toes .l/.r` (+ IK helpers).
  Body meshes: `<Name>_Head/_Body/_ArmLeft/_ArmRight/_LegLeft/_LegRight/_Cape`
  (+ `_Hat`/`_Helmet`).
- The sample hides unwanted held items by name, swaps materials to toon,
  adds outline hulls, applies the bone stretch, and attaches race parts and
  gear as children of bones.

## Options that were looked at (for the record)

- **VRoid / VRM anime:** rejected by the user (see above).
- **MakeHuman / MPFB2** (free, realistic slider bodies), **Character Creator 4**
  (paid), **Synty POLYGON** packs, commissions: possible upgrades later.
- **Blender:** the user's side project. Claude can drive Blender with Python
  scripts (or through the community "blender-mcp" add-on). Good for props,
  weapons, armour pieces, race parts, rigging and export; weak at attractive
  faces and bodies from scratch.
- quaternius.com, poly.pizza and hub.vroid.com are blocked from the cloud
  environment. raw.githubusercontent.com and `git clone` from GitHub work.
