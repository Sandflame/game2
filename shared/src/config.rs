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

/// Playing over a network (`config/network.ron`).
#[derive(Debug, Clone, Deserialize)]
pub struct NetworkConfig {
    /// The port the server listens on (UDP).
    pub port: u16,
    /// How often the server sends the world's state to players (seconds).
    pub send_every: f32,
    /// A player who sends nothing for this long is disconnected (seconds).
    pub timeout: f32,
    /// Moves a player reports may be this much faster than their speed
    /// allows (1.5 = 50% more), to forgive uneven network timing…
    pub move_tolerance: f32,
    /// …plus this many metres.
    pub move_slack: f32,
    /// How far behind the newest news other characters are shown, so their
    /// movement is smooth (seconds).
    pub smoothing: f32,
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            port: 5888,
            send_every: 0.05,
            timeout: 10.0,
            move_tolerance: 1.5,
            move_slack: 0.75,
            smoothing: 0.1,
        }
    }
}

impl Validate for NetworkConfig {
    fn validate(&self) -> Vec<String> {
        let mut p = Problems::default();
        if self.port == 0 {
            p.push("`port` must not be 0");
        }
        p.positive("send_every", self.send_every);
        p.positive("timeout", self.timeout);
        if self.move_tolerance < 1.0 {
            p.push("`move_tolerance` must be at least 1");
        }
        p.non_negative("move_slack", self.move_slack);
        p.non_negative("smoothing", self.smoothing);
        p.0
    }
}

/// All global config files, loaded together at startup.
#[derive(Debug, Clone, Resource)]
pub struct GameConfig {
    pub simulation: SimulationConfig,
    pub movement: MovementConfig,
    pub combat: CombatConfig,
    pub network: NetworkConfig,
}

impl GameConfig {
    /// Load every config file from `<assets>/data/config/`.
    pub fn load(assets_dir: &Path) -> Result<Self, DataError> {
        let dir = assets_dir.join("data").join("config");
        Ok(Self {
            simulation: load_ron(&dir.join("simulation.ron"))?,
            movement: load_ron(&dir.join("movement.ron"))?,
            combat: load_ron(&dir.join("combat.ron"))?,
            network: load_ron(&dir.join("network.ron"))?,
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
