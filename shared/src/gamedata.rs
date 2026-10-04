//! Everything loaded from `assets/data/` at startup, checked together so
//! that references between files (e.g. a hotbar naming an ability) are
//! valid.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use bevy::prelude::Resource;
use serde::Deserialize;

use crate::combat::AbilityDef;
use crate::components::HOTBAR_SLOTS;
use crate::config::GameConfig;
use crate::data::{DataError, Problems, Validate, load_ron};
use crate::level::Level;

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
/// Milestone 3 moves health and hotbars into class data.
#[derive(Debug, Clone, Deserialize)]
pub struct PlayerConfig {
    pub max_health: u32,
    pub hit_radius: f32,
    /// Ability ids for hotbar slots 1, 2, 3… (at most 10).
    pub hotbar: Vec<String>,
}

impl Validate for PlayerConfig {
    fn validate(&self) -> Vec<String> {
        let mut p = Problems::default();
        if self.max_health == 0 {
            p.push("`max_health` must be greater than 0");
        }
        p.positive("hit_radius", self.hit_radius);
        if self.hotbar.len() > HOTBAR_SLOTS {
            p.push(format!("`hotbar` has more than {HOTBAR_SLOTS} slots"));
        }
        p.0
    }
}

/// All game data except zones (which are loaded when entered).
#[derive(Debug, Clone, Resource)]
pub struct GameData {
    pub config: GameConfig,
    pub player: PlayerConfig,
    pub abilities: HashMap<String, AbilityDef>,
    pub enemies: HashMap<String, EnemyDef>,
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

        let mut enemies = HashMap::new();
        for (path, enemy) in load_dir::<EnemyDef>(&data.join("enemies"))? {
            let id = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            enemies.insert(id, enemy);
        }

        let player_path = data.join("config").join("player.ron");
        for id in &player.hotbar {
            if !abilities.contains_key(id) {
                return Err(invalid(
                    &player_path,
                    format!("hotbar names unknown ability `{id}`"),
                ));
            }
        }

        Ok(Self {
            config,
            player,
            abilities,
            enemies,
        })
    }

    /// Load a zone and check that everything it refers to exists.
    pub fn load_level(&self, assets_dir: &Path, zone: &str) -> Result<Level, DataError> {
        let level = Level::load(assets_dir, zone)?;
        let problems: Vec<String> = level
            .spawns
            .iter()
            .filter(|s| !self.enemies.contains_key(&s.enemy))
            .map(|s| format!("spawn names unknown enemy `{}`", s.enemy))
            .collect();
        if problems.is_empty() {
            Ok(level)
        } else {
            Err(DataError::Invalid {
                path: Level::path(assets_dir, zone),
                problems,
            })
        }
    }
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
        assert!(!data.player.hotbar.is_empty());
        let level = data.load_level(&assets, "sandbox").unwrap();
        assert!(!level.spawns.is_empty());
    }
}
