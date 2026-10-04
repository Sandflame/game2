//! Everything loaded from `assets/data/` at startup, checked together so
//! that references between files (e.g. a hotbar naming an ability) are
//! valid.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::prelude::Resource;
use serde::Deserialize;

use crate::abilities::AbilityDef;
use crate::classes::{ClassDef, SecondaryChoice};
use crate::config::GameConfig;
use crate::data::{DataError, Problems, Validate, load_ron};
use crate::encounters::EncounterDef;
use crate::items::{ItemDef, ItemFile};
use crate::level::Level;
use crate::progression::ProgressionDef;
use crate::statuses::{StatusDef, StatusFile};
use crate::synergy::SynergyDef;

/// An enemy type (`assets/data/enemies/<id>.ron`; the id is the file name).
#[derive(Debug, Clone, Deserialize)]
pub struct EnemyDef {
    pub name: String,
    pub max_health: u32,
    pub hit_radius: f32,
    /// Placeholder look (client only).
    pub visual: String,
    /// Training dummies: heal to full after this many seconds without being hit.
    #[serde(default)]
    pub reset_after: Option<f32>,
    /// Scales the enemy's damage (100 = normal).
    #[serde(default = "normal_power")]
    pub power: f32,
    /// Abilities the enemy uses on whoever it is most angry at.
    #[serde(default)]
    pub actions: Vec<EnemyAction>,
    /// Experience for each player who fought it when it is defeated.
    #[serde(default)]
    pub xp: u32,
}

fn normal_power() -> f32 {
    crate::formulas::BASE_POWER
}

/// "Use this ability every so many seconds."
#[derive(Debug, Clone, Deserialize)]
pub struct EnemyAction {
    pub ability: String,
    pub every: f32,
}

impl Validate for EnemyDef {
    fn validate(&self) -> Vec<String> {
        let mut p = Problems::default();
        if self.max_health == 0 {
            p.push("`max_health` must be greater than 0");
        }
        p.positive("hit_radius", self.hit_radius);
        if let Some(reset) = self.reset_after {
            p.positive("reset_after", reset);
        }
        p.positive("power", self.power);
        for (i, action) in self.actions.iter().enumerate() {
            p.positive(&format!("actions[{i}].every"), action.every);
        }
        p.0
    }
}

/// A list of abilities in one file (`assets/data/abilities/*.ron`).
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct AbilityFile(pub Vec<AbilityDef>);

impl Validate for AbilityFile {
    fn validate(&self) -> Vec<String> {
        self.0.iter().flat_map(AbilityDef::problems).collect()
    }
}

/// Player character settings (`assets/data/config/player.ron`).
#[derive(Debug, Clone, Deserialize)]
pub struct PlayerConfig {
    pub hit_radius: f32,
    /// The class (file name in `assets/data/classes/`) new characters start as.
    pub start_class: String,
    /// The zone (file name in `assets/data/zones/`) new characters start in.
    pub start_zone: String,
    /// Abilities every class has, after the class's own and its secondary
    /// class's (hotbar slots - and =).
    #[serde(default)]
    pub shared_abilities: Vec<String>,
    /// Items new characters start with; they put on what they can.
    #[serde(default)]
    pub start_items: Vec<String>,
}

impl Validate for PlayerConfig {
    fn validate(&self) -> Vec<String> {
        let mut p = Problems::default();
        p.positive("hit_radius", self.hit_radius);
        let class_slots = crate::classes::CORE_ABILITIES
            + crate::classes::SPEC_ABILITIES
            + crate::classes::SECONDARY_ABILITIES;
        if class_slots + self.shared_abilities.len() > crate::components::HOTBAR_SLOTS {
            p.push("too many `shared_abilities` to fit on the hotbar");
        }
        p.0
    }
}

/// Every zone, by id (file name in `assets/data/zones/`).
#[derive(Debug, Clone, Resource, Default)]
pub struct Zones(pub HashMap<String, Level>);

impl Zones {
    pub fn get(&self, zone: &str) -> Option<&Level> {
        self.0.get(zone)
    }
}

/// All game data except zones (which are loaded when entered).
#[derive(Debug, Clone, Resource)]
pub struct GameData {
    pub config: GameConfig,
    pub player: PlayerConfig,
    pub abilities: HashMap<String, AbilityDef>,
    pub statuses: HashMap<String, StatusDef>,
    pub classes: HashMap<String, ClassDef>,
    pub enemies: HashMap<String, EnemyDef>,
    pub encounters: HashMap<String, EncounterDef>,
    pub progression: ProgressionDef,
    pub items: HashMap<String, ItemDef>,
    pub synergy: SynergyDef,
}

impl GameData {
    pub fn load(assets_dir: &Path) -> Result<Self, DataError> {
        let data = assets_dir.join("data");
        let config = GameConfig::load(assets_dir)?;
        let player: PlayerConfig = load_ron(&data.join("config").join("player.ron"))?;

        let mut abilities = HashMap::new();
        for (path, file) in load_dir::<AbilityFile>(&data.join("abilities"))? {
            for ability in file.0 {
                if abilities.contains_key(&ability.id) {
                    return Err(invalid(
                        &path,
                        format!("ability id `{}` is used twice", ability.id),
                    ));
                }
                abilities.insert(ability.id.clone(), ability);
            }
        }

        let mut statuses = HashMap::new();
        for (path, file) in load_dir::<StatusFile>(&data.join("statuses"))? {
            for status in file.0 {
                if statuses.contains_key(&status.id) {
                    return Err(invalid(
                        &path,
                        format!("status id `{}` is used twice", status.id),
                    ));
                }
                statuses.insert(status.id.clone(), status);
            }
        }

        let classes: HashMap<String, ClassDef> = load_dir::<ClassDef>(&data.join("classes"))?
            .into_iter()
            .map(|(path, class)| (file_id(&path), class))
            .collect();
        let enemies: HashMap<String, EnemyDef> = load_dir::<EnemyDef>(&data.join("enemies"))?
            .into_iter()
            .map(|(path, enemy)| (file_id(&path), enemy))
            .collect();

        let encounters: HashMap<String, EncounterDef> =
            load_dir::<EncounterDef>(&data.join("encounters"))?
                .into_iter()
                .map(|(path, encounter)| (file_id(&path), encounter))
                .collect();

        let progression: ProgressionDef = load_ron(&data.join("progression.ron"))?;
        let synergy: SynergyDef = load_ron(&data.join("synergy.ron"))?;
        let mut items = HashMap::new();
        for (path, file) in load_dir::<ItemFile>(&data.join("items"))? {
            for (id, item) in file.0 {
                if items.contains_key(&id) {
                    return Err(invalid(&path, format!("item id `{id}` is used twice")));
                }
                items.insert(id, item);
            }
        }

        let game = Self {
            config,
            player,
            abilities,
            statuses,
            classes,
            enemies,
            encounters,
            progression,
            items,
            synergy,
        };
        game.check_references(&data)?;
        Ok(game)
    }

    /// Make sure every name one file uses for something in another file exists.
    fn check_references(&self, data: &Path) -> Result<(), DataError> {
        let ability = |id: &str| self.abilities.contains_key(id);
        let status = |id: &str| self.statuses.contains_key(id);

        for a in self.abilities.values() {
            let mut problems: Vec<String> = a
                .statuses()
                .filter(|s| !status(s))
                .map(|s| format!("ability `{}` uses unknown status `{s}`", a.id))
                .collect();
            if let Some(combo) = &a.combo
                && !ability(&combo.after)
            {
                problems.push(format!(
                    "ability `{}` combos after unknown ability `{}`",
                    a.id, combo.after
                ));
            }
            if !problems.is_empty() {
                return Err(DataError::Invalid {
                    path: data.join("abilities"),
                    problems,
                });
            }
        }
        for (id, class) in &self.classes {
            let problems: Vec<String> = class
                .all_abilities()
                .chain(class.lendable.iter().map(|l| l.ability.as_str()))
                .filter(|a| !ability(a))
                .map(|a| format!("uses unknown ability `{a}`"))
                .collect();
            if !problems.is_empty() {
                return Err(DataError::Invalid {
                    path: data.join("classes").join(format!("{id}.ron")),
                    problems,
                });
            }
        }
        for (id, enemy) in &self.enemies {
            let problems: Vec<String> = enemy
                .actions
                .iter()
                .filter(|a| !ability(&a.ability))
                .map(|a| format!("uses unknown ability `{}`", a.ability))
                .collect();
            if !problems.is_empty() {
                return Err(DataError::Invalid {
                    path: data.join("enemies").join(format!("{id}.ron")),
                    problems,
                });
            }
        }
        for (id, encounter) in &self.encounters {
            let mut problems: Vec<String> = encounter
                .abilities()
                .filter(|a| !ability(a))
                .map(|a| format!("uses unknown ability `{a}`"))
                .collect();
            problems.extend(
                encounter
                    .enemies()
                    .filter(|e| !self.enemies.contains_key(*e))
                    .map(|e| format!("uses unknown enemy `{e}`")),
            );
            problems.extend(
                encounter
                    .loot
                    .iter()
                    .filter(|l| !self.items.contains_key(&l.item))
                    .map(|l| format!("loot names unknown item `{}`", l.item)),
            );
            if !problems.is_empty() {
                return Err(DataError::Invalid {
                    path: data.join("encounters").join(format!("{id}.ron")),
                    problems,
                });
            }
        }
        let mut item_problems: Vec<String> = self
            .items
            .iter()
            .filter_map(|(id, item)| {
                let class = item.class.as_ref()?;
                (!self.classes.contains_key(class))
                    .then(|| format!("`{id}` is for unknown class `{class}`"))
            })
            .collect();
        if !item_problems.is_empty() {
            item_problems.sort();
            return Err(DataError::Invalid {
                path: data.join("items"),
                problems: item_problems,
            });
        }
        let synergy_problems: Vec<String> = self
            .synergy
            .statuses()
            .filter(|s| !status(s))
            .map(|s| format!("names unknown status `{s}`"))
            .collect();
        if !synergy_problems.is_empty() {
            return Err(DataError::Invalid {
                path: data.join("synergy.ron"),
                problems: synergy_problems,
            });
        }
        let player_path = data.join("config").join("player.ron");
        for item in &self.player.start_items {
            if !self.items.contains_key(item) {
                return Err(invalid(
                    &player_path,
                    format!("`start_items` names unknown item `{item}`"),
                ));
            }
        }
        for shared in &self.player.shared_abilities {
            if !ability(shared) {
                return Err(invalid(
                    &player_path,
                    format!("`shared_abilities` names unknown ability `{shared}`"),
                ));
            }
        }
        if !self.classes.contains_key(&self.player.start_class) {
            return Err(invalid(
                &data.join("config").join("player.ron"),
                format!(
                    "`start_class` names unknown class `{}`",
                    self.player.start_class
                ),
            ));
        }
        Ok(())
    }

    /// Classes in display order (by role, then name).
    pub fn class_list(&self) -> Vec<(&String, &ClassDef)> {
        let mut list: Vec<_> = self.classes.iter().collect();
        list.sort_by_key(|(_, c)| (c.role as u8, c.name.clone()));
        list
    }

    /// Load every zone and check that everything they refer to exists.
    pub fn load_zones(&self, assets_dir: &Path) -> Result<Zones, DataError> {
        let dir = assets_dir.join("data").join("zones");
        let zones: HashMap<String, Level> = load_dir::<Level>(&dir)?
            .into_iter()
            .map(|(path, level)| (file_id(&path), level))
            .collect();
        for (id, level) in &zones {
            let mut problems: Vec<String> = level
                .spawns
                .iter()
                .filter(|s| !self.enemies.contains_key(&s.enemy))
                .map(|s| format!("spawn names unknown enemy `{}`", s.enemy))
                .collect();
            problems.extend(
                level
                    .portals
                    .iter()
                    .filter(|p| !zones.contains_key(&p.to))
                    .map(|p| format!("portal leads to unknown zone `{}`", p.to)),
            );
            if let Some(encounter) = &level.encounter
                && !self.encounters.contains_key(encounter)
            {
                problems.push(format!("names unknown encounter `{encounter}`"));
            }
            if !problems.is_empty() {
                return Err(DataError::Invalid {
                    path: Level::path(assets_dir, id),
                    problems,
                });
            }
        }
        if !zones.contains_key(&self.player.start_zone) {
            return Err(invalid(
                &assets_dir.join("data").join("config").join("player.ron"),
                format!(
                    "`start_zone` names unknown zone `{}`",
                    self.player.start_zone
                ),
            ));
        }
        Ok(Zones(zones))
    }

    /// A class's hotbar: its own abilities (slots 1-8), two borrowed from
    /// its secondary class (9, 0), then the shared lantern abilities (-, =).
    /// Pass only a secondary choice that passed `check_secondary`.
    pub fn hotbar(
        &self,
        class: &ClassDef,
        spec: &str,
        secondary: Option<&SecondaryChoice>,
    ) -> Vec<Option<String>> {
        let mut bar = class.hotbar(spec);
        let own = crate::classes::CORE_ABILITIES + crate::classes::SPEC_ABILITIES;
        if let Some(choice) = secondary {
            for (slot, ability) in bar.iter_mut().skip(own).zip(&choice.abilities) {
                slot.clone_from(ability);
            }
        }
        let shared = own + crate::classes::SECONDARY_ABILITIES;
        for (slot, ability) in bar
            .iter_mut()
            .skip(shared)
            .zip(&self.player.shared_abilities)
        {
            *slot = Some(ability.clone());
        }
        bar
    }
}

fn file_id(path: &Path) -> String {
    path.file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

fn invalid(path: &Path, problem: String) -> DataError {
    DataError::Invalid {
        path: path.to_owned(),
        problems: vec![problem],
    }
}

/// Load every `.ron` file in a folder, sorted by file name.
pub fn load_dir<T: serde::de::DeserializeOwned + Validate>(
    dir: &Path,
) -> Result<Vec<(PathBuf, T)>, DataError> {
    let entries = std::fs::read_dir(dir).map_err(|source| DataError::Io {
        path: dir.to_owned(),
        source,
    })?;
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext == "ron"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .map(|path| load_ron(&path).map(|value| (path, value)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::find_assets_dir;

    /// The real data files in the repository must always load.
    #[test]
    fn shipped_game_data_is_valid() {
        let assets = find_assets_dir().unwrap();
        let data = GameData::load(&assets).unwrap();
        assert!(data.enemies.contains_key("training_dummy"));
        assert_eq!(data.classes.len(), 4);
        let zones = data.load_zones(&assets).unwrap();
        assert!(!zones.get("sandbox").unwrap().spawns.is_empty());
    }
}
