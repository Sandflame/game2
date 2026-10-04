//! Levels and experience. Every class levels separately, from 1 to the
//! level cap, and a class's level adds health and power. Numbers come from
//! `assets/data/progression.ron`.

use std::collections::HashMap;

use bevy::prelude::Component;
use serde::Deserialize;

use crate::data::{Problems, Validate};

/// `assets/data/progression.ron`.
#[derive(Debug, Clone, Deserialize)]
pub struct ProgressionDef {
    pub max_level: u32,
    /// Experience needed to go from each level to the next: the first
    /// number is level 1 → 2. There is one fewer than `max_level`.
    pub xp_to_next: Vec<u32>,
    /// Extra health per level, as a percentage of the class's level-1 health.
    pub health_per_level: f32,
    /// Extra Power per level, in percentage points (100% → 102% → ...).
    pub power_per_level: f32,
    /// Damage reduction from gear can't go above this (percent).
    pub max_guard: f32,
    /// Most items a character can carry (worn ones included).
    pub bag_size: usize,
    /// Your secondary class's level adds this much Power per level
    /// (percentage points) to your main class.
    #[serde(default)]
    pub secondary_power_per_level: f32,
    /// ...and this much health per level, as a percentage of your main
    /// class's level-1 health.
    #[serde(default)]
    pub secondary_health_per_level: f32,
}

impl Validate for ProgressionDef {
    fn validate(&self) -> Vec<String> {
        let mut p = Problems::default();
        if self.max_level < 2 {
            p.push("`max_level` must be at least 2");
        }
        if self.xp_to_next.len() + 1 != self.max_level as usize {
            p.push(format!(
                "`xp_to_next` needs {} numbers (one per level up to {}), found {}",
                self.max_level.saturating_sub(1),
                self.max_level,
                self.xp_to_next.len()
            ));
        }
        if self.xp_to_next.contains(&0) {
            p.push("every number in `xp_to_next` must be greater than 0");
        }
        p.non_negative("health_per_level", self.health_per_level);
        p.non_negative("power_per_level", self.power_per_level);
        p.non_negative("max_guard", self.max_guard);
        if self.max_guard >= 100.0 {
            p.push("`max_guard` must be below 100");
        }
        p.non_negative("secondary_power_per_level", self.secondary_power_per_level);
        p.non_negative(
            "secondary_health_per_level",
            self.secondary_health_per_level,
        );
        if self.bag_size == 0 {
            p.push("`bag_size` must be greater than 0");
        }
        p.0
    }
}

impl ProgressionDef {
    /// Experience needed to leave `level`, or `None` at the cap.
    pub fn xp_needed(&self, level: u32) -> Option<u32> {
        if level >= self.max_level || level == 0 {
            return None;
        }
        self.xp_to_next.get(level as usize - 1).copied()
    }

    /// Health multiplier at a level (1.0 at level 1).
    pub fn health_scale(&self, level: u32) -> f32 {
        1.0 + level.saturating_sub(1) as f32 * self.health_per_level / 100.0
    }

    /// Power added at a level (0 at level 1).
    pub fn power_bonus(&self, level: u32) -> f32 {
        level.saturating_sub(1) as f32 * self.power_per_level
    }

    /// Bonus from a secondary class at `level`: (extra health as a fraction
    /// of base health, extra Power points).
    pub fn secondary_bonus(&self, level: u32) -> (f32, f32) {
        let level = level as f32;
        (
            level * self.secondary_health_per_level / 100.0,
            level * self.secondary_power_per_level,
        )
    }
}

/// One class's level and experience towards the next level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClassProgress {
    pub level: u32,
    pub xp: u32,
}

impl Default for ClassProgress {
    fn default() -> Self {
        Self { level: 1, xp: 0 }
    }
}

/// Every class's level for one character (classes never played are level 1).
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct ClassLevels(pub HashMap<String, ClassProgress>);

impl ClassLevels {
    pub fn get(&self, class: &str) -> ClassProgress {
        self.0.get(class).copied().unwrap_or_default()
    }

    /// Add experience to a class. Returns the new level if it went up.
    /// At the level cap experience is not kept.
    pub fn add_xp(&mut self, class: &str, amount: u32, rules: &ProgressionDef) -> Option<u32> {
        let progress = self.0.entry(class.to_owned()).or_default();
        let before = progress.level;
        progress.xp = progress.xp.saturating_add(amount);
        while let Some(needed) = rules.xp_needed(progress.level) {
            if progress.xp < needed {
                break;
            }
            progress.xp -= needed;
            progress.level += 1;
        }
        if progress.level >= rules.max_level {
            progress.level = rules.max_level;
            progress.xp = 0;
        }
        (progress.level > before).then_some(progress.level)
    }
}

/// The level a character fights at: their class level, lowered to a
/// zone's level sync if it has one (so friends of any level can play
/// together). Being under the sync level does not raise you.
pub fn effective_level(level: u32, sync: Option<u32>) -> u32 {
    sync.map_or(level, |sync| level.min(sync))
}

#[cfg(test)]
pub(crate) fn test_rules() -> ProgressionDef {
    ProgressionDef {
        max_level: 4,
        xp_to_next: vec![100, 200, 300],
        health_per_level: 10.0,
        power_per_level: 2.0,
        max_guard: 30.0,
        bag_size: 10,
        secondary_power_per_level: 0.5,
        secondary_health_per_level: 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn experience_levels_up_and_carries_over() {
        let rules = test_rules();
        let mut levels = ClassLevels::default();
        assert_eq!(levels.add_xp("mage", 50, &rules), None);
        assert_eq!(levels.get("mage"), ClassProgress { level: 1, xp: 50 });
        // 50 + 300 = 350: level 2 (100), level 3 (200), 50 left over.
        assert_eq!(levels.add_xp("mage", 300, &rules), Some(3));
        assert_eq!(levels.get("mage"), ClassProgress { level: 3, xp: 50 });
        // Other classes are untouched.
        assert_eq!(levels.get("knight").level, 1);
    }

    #[test]
    fn experience_stops_at_the_cap() {
        let rules = test_rules();
        let mut levels = ClassLevels::default();
        assert_eq!(levels.add_xp("mage", 10_000, &rules), Some(4));
        assert_eq!(levels.get("mage"), ClassProgress { level: 4, xp: 0 });
        assert_eq!(levels.add_xp("mage", 500, &rules), None);
        assert_eq!(rules.xp_needed(4), None);
    }

    #[test]
    fn levels_add_health_and_power() {
        let rules = test_rules();
        assert_eq!(rules.health_scale(1), 1.0);
        assert!((rules.health_scale(3) - 1.2).abs() < 1e-6);
        assert_eq!(rules.power_bonus(1), 0.0);
        assert_eq!(rules.power_bonus(4), 6.0);
    }

    #[test]
    fn level_sync_only_lowers() {
        assert_eq!(effective_level(20, Some(5)), 5);
        assert_eq!(effective_level(3, Some(5)), 3);
        assert_eq!(effective_level(12, None), 12);
    }

    #[test]
    fn bad_curves_are_reported() {
        let mut rules = test_rules();
        rules.xp_to_next.pop();
        assert_eq!(rules.validate().len(), 1);
    }
}
