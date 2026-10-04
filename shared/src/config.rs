//! Global tunable settings loaded from `assets/data/config/`.

use std::path::Path;

use bevy::prelude::Resource;
use serde::Deserialize;

use crate::combat::CombatConfig;
use crate::data::{DataError, Problems, Validate, load_ron};
use crate::movement::MovementConfig;

/// Settings for the fixed-rate game simulation (`config/simulation.ron`).
#[derive(Debug, Clone, Deserialize)]
pub struct SimulationConfig {
    /// How many times per second the game rules run.
    pub tick_hz: f64,
    /// Characters are saved this often (seconds), as well as whenever they
    /// level up, get loot, change gear, class or zone, and when the game closes.
    #[serde(default = "default_autosave")]
    pub autosave_every: f32,
}

fn default_autosave() -> f32 {
    60.0
}

impl Validate for SimulationConfig {
    fn validate(&self) -> Vec<String> {
        let mut p = Problems::default();
        if !(self.tick_hz.is_finite() && (10.0..=240.0).contains(&self.tick_hz)) {
            p.push(format!(
                "`tick_hz` must be between 10 and 240 (got {})",
                self.tick_hz
            ));
        }
        p.positive("autosave_every", self.autosave_every);
        p.0
    }
}

/// All global config files, loaded together at startup.
#[derive(Debug, Clone, Resource)]
pub struct GameConfig {
    pub simulation: SimulationConfig,
    pub movement: MovementConfig,
    pub combat: CombatConfig,
}

impl GameConfig {
    /// Load every config file from `<assets>/data/config/`.
    pub fn load(assets_dir: &Path) -> Result<Self, DataError> {
        let dir = assets_dir.join("data").join("config");
        Ok(Self {
            simulation: load_ron(&dir.join("simulation.ron"))?,
            movement: load_ron(&dir.join("movement.ron"))?,
            combat: load_ron(&dir.join("combat.ron"))?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::find_assets_dir;

    /// The real data files in the repository must always load.
    #[test]
    fn shipped_config_files_are_valid() {
        let assets = find_assets_dir().unwrap();
        GameConfig::load(&assets).unwrap();
    }

    #[test]
    fn rejects_silly_tick_rate() {
        let config = SimulationConfig {
            tick_hz: 0.0,
            autosave_every: 60.0,
        };
        assert_eq!(config.validate().len(), 1);
    }
}
