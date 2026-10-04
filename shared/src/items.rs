//! Gear: item data (`assets/data/items/*.ron`), what a character carries
//! and wears, how gear and levels add up to their stats, and boss loot.
//!
//! Armour and rings are shared by every class; each class wears its own
//! weapon, so switching flame switches weapon too.

use std::collections::HashMap;

use bevy::prelude::Component;
use serde::{Deserialize, Serialize};

use crate::classes::{ClassDef, Stats};
use crate::combat::Reject;
use crate::data::{Problems, Validate};
use crate::formulas::Rng;
use crate::progression::ProgressionDef;

/// Where an item is worn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum Slot {
    Weapon,
    Head,
    Body,
    Hands,
    Feet,
    Ring,
}

impl Slot {
    pub const ALL: [Slot; 6] = [
        Slot::Weapon,
        Slot::Head,
        Slot::Body,
        Slot::Hands,
        Slot::Feet,
        Slot::Ring,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Slot::Weapon => "Weapon",
            Slot::Head => "Head",
            Slot::Body => "Body",
            Slot::Hands => "Hands",
            Slot::Feet => "Feet",
            Slot::Ring => "Ring",
        }
    }

    /// The name used when saving.
    pub fn key(self) -> &'static str {
        match self {
            Slot::Weapon => "weapon",
            Slot::Head => "head",
            Slot::Body => "body",
            Slot::Hands => "hands",
            Slot::Feet => "feet",
            Slot::Ring => "ring",
        }
    }

    pub fn from_key(key: &str) -> Option<Slot> {
        Slot::ALL.into_iter().find(|s| s.key() == key)
    }
}

/// One kind of item.
#[derive(Debug, Clone, Deserialize)]
pub struct ItemDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub slot: Slot,
    /// The level your class needs to wear it. Also its strength: in a
    /// level-synced duty, gear above the sync level is scaled down.
    pub level: u32,
    /// Weapons belong to one class (a file name in `assets/data/classes/`).
    #[serde(default)]
    pub class: Option<String>,
    /// Extra maximum health.
    #[serde(default)]
    pub health: f32,
    /// Extra Power, in percentage points.
    #[serde(default)]
    pub power: f32,
    /// Extra chance of a critical hit, in percentage points.
    #[serde(default)]
    pub crit: f32,
    /// Less damage taken, in percent.
    #[serde(default)]
    pub guard: f32,
}

impl ItemDef {
    pub fn problems(&self) -> Vec<String> {
        let mut p = Problems::default();
        if self.level == 0 {
            p.push("`level` must be at least 1");
        }
        match (self.slot, &self.class) {
            (Slot::Weapon, None) => p.push("weapons need a `class`"),
            (Slot::Weapon, Some(_)) | (_, None) => {}
            (_, Some(_)) => p.push("only weapons have a `class`"),
        }
        for (name, value) in [
            ("health", self.health),
            ("power", self.power),
            ("crit", self.crit),
            ("guard", self.guard),
        ] {
            p.non_negative(name, value);
        }
        p.0
    }

    /// A short list of what it gives, e.g. "+400 health, +2% power".
    pub fn summary(&self) -> String {
        let mut parts = Vec::new();
        if self.health > 0.0 {
            parts.push(format!("+{:.0} health", self.health));
        }
        if self.power > 0.0 {
            parts.push(format!("+{:.0}% power", self.power));
        }
        if self.crit > 0.0 {
            parts.push(format!("+{:.0}% crit", self.crit));
        }
        if self.guard > 0.0 {
            parts.push(format!("-{:.0}% damage taken", self.guard));
        }
        parts.join(", ")
    }
}

/// A file of items: `{ "item_id": (...), ... }`.
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct ItemFile(pub HashMap<String, ItemDef>);

impl Validate for ItemFile {
    fn validate(&self) -> Vec<String> {
        let mut list: Vec<_> = self.0.iter().collect();
        list.sort_by_key(|(id, _)| *id);
        list.into_iter()
            .flat_map(|(id, item)| {
                item.problems()
                    .into_iter()
                    .map(move |problem| format!("`{id}`: {problem}"))
            })
            .collect()
    }
}

/// An item a character owns (the id tells two copies apart).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnedItem {
    pub id: u64,
    pub item: String,
}

/// Everything a character carries, worn gear included.
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Bag {
    pub items: Vec<OwnedItem>,
    next_id: u64,
}

impl Bag {
    /// Add an item if there is room. Returns its id.
    pub fn add(&mut self, item: &str, capacity: usize) -> Result<u64, Reject> {
        if self.items.len() >= capacity {
            return Err(Reject::BagFull);
        }
        self.next_id += 1;
        self.items.push(OwnedItem {
            id: self.next_id,
            item: item.to_owned(),
        });
        Ok(self.next_id)
    }

    pub fn get(&self, id: u64) -> Option<&OwnedItem> {
        self.items.iter().find(|i| i.id == id)
    }

    fn remove(&mut self, id: u64) -> Option<OwnedItem> {
        let index = self.items.iter().position(|i| i.id == id)?;
        Some(self.items.remove(index))
    }
}

/// What a character is wearing: armour shared by all classes, and one
/// weapon per class. Values are ids of items in their [`Bag`].
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Equipment {
    pub armour: HashMap<Slot, u64>,
    pub weapons: HashMap<String, u64>,
}

impl Equipment {
    /// The item in a slot for this class.
    pub fn in_slot(&self, slot: Slot, class: &str) -> Option<u64> {
        match slot {
            Slot::Weapon => self.weapons.get(class).copied(),
            other => self.armour.get(&other).copied(),
        }
    }

    pub fn is_worn(&self, id: u64) -> bool {
        self.armour.values().any(|&i| i == id) || self.weapons.values().any(|&i| i == id)
    }

    /// Ids of what this class has on: armour plus its own weapon.
    pub fn worn_by(&self, class: &str) -> impl Iterator<Item = u64> + '_ {
        self.armour
            .values()
            .copied()
            .chain(self.weapons.get(class).copied())
    }
}

/// Put on an item from the bag. `level` is the class's real level (level
/// sync doesn't stop you wearing your own gear).
pub fn equip(
    bag: &Bag,
    equipment: &mut Equipment,
    items: &HashMap<String, ItemDef>,
    id: u64,
    class: &str,
    level: u32,
) -> Result<(), Reject> {
    let owned = bag.get(id).ok_or(Reject::NoSuchItem)?;
    let def = items.get(&owned.item).ok_or(Reject::NoSuchItem)?;
    if def.level > level {
        return Err(Reject::LevelTooLow);
    }
    match (&def.class, def.slot) {
        (Some(for_class), Slot::Weapon) => {
            if for_class != class {
                return Err(Reject::WrongClass);
            }
            equipment.weapons.insert(class.to_owned(), id);
        }
        (_, slot) => {
            equipment.armour.insert(slot, id);
        }
    }
    Ok(())
}

/// Take off whatever is in a slot (for weapons: this class's weapon).
pub fn unequip(equipment: &mut Equipment, slot: Slot, class: &str) {
    match slot {
        Slot::Weapon => equipment.weapons.remove(class),
        other => equipment.armour.remove(&other),
    };
}

/// Throw an item away (taking it off first if it is worn).
pub fn discard(bag: &mut Bag, equipment: &mut Equipment, id: u64) -> Result<(), Reject> {
    bag.remove(id).ok_or(Reject::NoSuchItem)?;
    equipment.armour.retain(|_, &mut worn| worn != id);
    equipment.weapons.retain(|_, &mut worn| worn != id);
    Ok(())
}

/// A character's stats from their class, level and worn gear.
/// `level` is the level they fight at (after level sync); gear above that
/// level counts for proportionally less.
pub fn character_stats(
    class: &ClassDef,
    level: u32,
    worn: &[&ItemDef],
    rules: &ProgressionDef,
    base_crit: f32,
) -> Stats {
    let (mut health, mut power, mut crit, mut guard) = (0.0, 0.0, 0.0, 0.0);
    for item in worn {
        let scale = if item.level > level {
            level as f32 / item.level as f32
        } else {
            1.0
        };
        health += item.health * scale;
        power += item.power * scale;
        crit += item.crit * scale;
        guard += item.guard * scale;
    }
    Stats {
        max_health: (class.max_health as f32 * rules.health_scale(level) + health).round() as u32,
        power: class.power + rules.power_bonus(level) + power,
        threat_multiplier: class.threat_multiplier,
        crit_chance: (base_crit + crit / 100.0).clamp(0.0, 1.0),
        guard: guard.min(rules.max_guard),
    }
}

/// One possible drop: `chance` in percent.
#[derive(Debug, Clone, Deserialize)]
pub struct LootEntry {
    pub item: String,
    pub chance: f32,
}

/// Roll a loot table: each entry drops on its own chance. If nothing
/// dropped and `at_least_one` is set, one entry is picked at random.
pub fn roll_loot(table: &[LootEntry], rng: &mut Rng, at_least_one: bool) -> Vec<String> {
    let mut drops: Vec<String> = table
        .iter()
        .filter(|entry| rng.chance(entry.chance / 100.0))
        .map(|entry| entry.item.clone())
        .collect();
    if drops.is_empty() && at_least_one && !table.is_empty() {
        let pick = (rng.next_u64() % table.len() as u64) as usize;
        drops.push(table[pick].item.clone());
    }
    drops
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::classes::{Role, SpecDef};
    use crate::progression::test_rules;

    fn item(slot: Slot, level: u32, class: Option<&str>) -> ItemDef {
        ItemDef {
            name: "Thing".into(),
            description: String::new(),
            slot,
            level,
            class: class.map(str::to_owned),
            health: 100.0,
            power: 4.0,
            crit: 10.0,
            guard: 20.0,
        }
    }

    fn items() -> HashMap<String, ItemDef> {
        HashMap::from([
            ("helm".to_owned(), item(Slot::Head, 1, None)),
            ("big_helm".to_owned(), item(Slot::Head, 10, None)),
            ("sword".to_owned(), item(Slot::Weapon, 1, Some("knight"))),
        ])
    }

    fn class() -> ClassDef {
        ClassDef {
            name: "Knight".into(),
            description: String::new(),
            flame: "white".into(),
            role: Role::Durable,
            max_health: 1000,
            power: 100.0,
            threat_multiplier: 2.0,
            core: Vec::new(),
            specializations: vec![SpecDef {
                id: "main".into(),
                name: "Main".into(),
                description: String::new(),
                abilities: Vec::new(),
            }],
            default_spec: "main".into(),
        }
    }

    #[test]
    fn equipping_checks_level_and_class() {
        let items = items();
        let mut bag = Bag::default();
        let helm = bag.add("helm", 10).unwrap();
        let big = bag.add("big_helm", 10).unwrap();
        let sword = bag.add("sword", 10).unwrap();
        let mut worn = Equipment::default();
        assert_eq!(equip(&bag, &mut worn, &items, helm, "knight", 1), Ok(()));
        assert_eq!(
            equip(&bag, &mut worn, &items, big, "knight", 5),
            Err(Reject::LevelTooLow)
        );
        assert_eq!(
            equip(&bag, &mut worn, &items, sword, "mage", 1),
            Err(Reject::WrongClass)
        );
        assert_eq!(equip(&bag, &mut worn, &items, sword, "knight", 1), Ok(()));
        assert_eq!(
            equip(&bag, &mut worn, &items, 99, "knight", 1),
            Err(Reject::NoSuchItem)
        );
        // Armour is shared; the weapon only counts for its class.
        assert_eq!(worn.worn_by("knight").count(), 2);
        assert_eq!(worn.worn_by("mage").count(), 1);
        unequip(&mut worn, Slot::Weapon, "knight");
        assert_eq!(worn.in_slot(Slot::Weapon, "knight"), None);
    }

    #[test]
    fn discarding_takes_worn_items_off() {
        let items = items();
        let mut bag = Bag::default();
        let helm = bag.add("helm", 10).unwrap();
        let mut worn = Equipment::default();
        equip(&bag, &mut worn, &items, helm, "knight", 1).unwrap();
        discard(&mut bag, &mut worn, helm).unwrap();
        assert!(bag.items.is_empty());
        assert!(!worn.is_worn(helm));
        assert_eq!(discard(&mut bag, &mut worn, helm), Err(Reject::NoSuchItem));
    }

    #[test]
    fn bags_fill_up() {
        let mut bag = Bag::default();
        bag.add("helm", 1).unwrap();
        assert_eq!(bag.add("helm", 1), Err(Reject::BagFull));
    }

    #[test]
    fn stats_add_up_from_class_level_and_gear() {
        let rules = test_rules();
        let items = items();
        let helm = &items["helm"];
        let stats = character_stats(&class(), 3, &[helm], &rules, 0.05);
        // 1000 × 1.2 (two levels at 10%) + 100 from the helm.
        assert_eq!(stats.max_health, 1300);
        // 100 + 2 levels × 2 + 4.
        assert!((stats.power - 108.0).abs() < 1e-4);
        assert!((stats.crit_chance - 0.15).abs() < 1e-6);
        assert_eq!(stats.guard, 20.0);
        assert_eq!(stats.threat_multiplier, 2.0);
    }

    #[test]
    fn guard_is_capped_and_synced_gear_is_scaled() {
        let rules = test_rules();
        let items = items();
        let big = &items["big_helm"];
        // A level 10 helm at level 5 counts half.
        let stats = character_stats(&class(), 5, &[big, big], &rules, 0.0);
        assert_eq!(stats.guard, 20.0);
        let capped = character_stats(&class(), 10, &[big, big], &rules, 0.0);
        assert_eq!(capped.guard, rules.max_guard);
    }

    #[test]
    fn loot_rolls_respect_chances() {
        let table = vec![
            LootEntry {
                item: "always".into(),
                chance: 100.0,
            },
            LootEntry {
                item: "never".into(),
                chance: 0.0,
            },
        ];
        let mut rng = Rng::new(7);
        assert_eq!(roll_loot(&table, &mut rng, true), vec!["always".to_owned()]);
        let rare = vec![LootEntry {
            item: "never".into(),
            chance: 0.0,
        }];
        assert!(roll_loot(&rare, &mut rng, false).is_empty());
        assert_eq!(roll_loot(&rare, &mut rng, true), vec!["never".to_owned()]);
    }

    #[test]
    fn item_data_is_checked() {
        let mut bad = item(Slot::Head, 0, Some("knight"));
        bad.power = -1.0;
        assert_eq!(bad.problems().len(), 3);
        assert_eq!(item(Slot::Weapon, 1, None).problems().len(), 1);
    }
}
