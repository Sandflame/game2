//! Classes and specializations. A character can be any class: the class
//! is the flame burning in their lantern.

use bevy::prelude::Component;
use serde::{Deserialize, Serialize};

use crate::components::HOTBAR_SLOTS;
use crate::data::{Problems, Validate};

/// Abilities every class has in its core kit.
pub const CORE_ABILITIES: usize = 5;
/// Abilities each specialization adds.
pub const SPEC_ABILITIES: usize = 3;

/// What a class leans towards (soft roles: no content requires any).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum Role {
    Durable,
    Sustain,
    Damage,
}

impl Role {
    pub fn label(self) -> &'static str {
        match self {
            Role::Durable => "Durable",
            Role::Sustain => "Sustain",
            Role::Damage => "Damage",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SpecDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub abilities: Vec<String>,
}

/// One class (`assets/data/classes/<id>.ron`; the id is the file name).
#[derive(Debug, Clone, Deserialize)]
pub struct ClassDef {
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// The lantern flame's look (client only, e.g. "crimson").
    pub flame: String,
    pub role: Role,
    pub max_health: u32,
    /// Scales damage and healing (100 = normal).
    pub power: f32,
    /// Threat generated per point of damage or healing.
    pub threat_multiplier: f32,
    /// The five core abilities (hotbar slots 1–5).
    pub core: Vec<String>,
    pub specializations: Vec<SpecDef>,
    pub default_spec: String,
}

impl ClassDef {
    pub fn spec(&self, id: &str) -> Option<&SpecDef> {
        self.specializations.iter().find(|s| s.id == id)
    }

    /// Every ability this class can put on its hotbar.
    pub fn all_abilities(&self) -> impl Iterator<Item = &str> {
        self.core
            .iter()
            .chain(self.specializations.iter().flat_map(|s| s.abilities.iter()))
            .map(String::as_str)
    }

    /// Hotbar layout: 5 core abilities, 3 from the specialization, and
    /// 2 empty slots (for a secondary class, Milestone 6).
    pub fn hotbar(&self, spec: &str) -> Vec<Option<String>> {
        let mut bar: Vec<Option<String>> = self.core.iter().cloned().map(Some).collect();
        if let Some(spec) = self.spec(spec) {
            bar.extend(spec.abilities.iter().cloned().map(Some));
        }
        bar.resize(HOTBAR_SLOTS, None);
        bar
    }
}

impl Validate for ClassDef {
    fn validate(&self) -> Vec<String> {
        let mut p = Problems::default();
        if self.max_health == 0 {
            p.push("`max_health` must be greater than 0");
        }
        p.positive("power", self.power);
        p.non_negative("threat_multiplier", self.threat_multiplier);
        if self.core.len() != CORE_ABILITIES {
            p.push(format!(
                "`core` must list exactly {CORE_ABILITIES} abilities"
            ));
        }
        for spec in &self.specializations {
            if spec.abilities.len() != SPEC_ABILITIES {
                p.push(format!(
                    "specialization `{}` must list exactly {SPEC_ABILITIES} abilities",
                    spec.id
                ));
            }
        }
        if self.spec(&self.default_spec).is_none() {
            p.push(format!(
                "`default_spec` `{}` is not one of its specializations",
                self.default_spec
            ));
        }
        p.0
    }
}

/// A character's current class and specialization.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct CurrentClass {
    pub class: String,
    pub spec: String,
}

/// A character's fighting numbers, from their class, level and gear
/// (see `items::character_stats`).
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct Stats {
    pub max_health: u32,
    pub power: f32,
    pub threat_multiplier: f32,
    /// Chance of a critical hit (0–1).
    pub crit_chance: f32,
    /// Less damage taken, in percent.
    pub guard: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn class() -> ClassDef {
        ClassDef {
            name: "Test".into(),
            description: String::new(),
            flame: "white".into(),
            role: Role::Damage,
            max_health: 1000,
            power: 100.0,
            threat_multiplier: 1.0,
            core: (1..=5).map(|i| format!("core{i}")).collect(),
            specializations: vec![SpecDef {
                id: "main".into(),
                name: "Main".into(),
                description: String::new(),
                abilities: (1..=3).map(|i| format!("spec{i}")).collect(),
            }],
            default_spec: "main".into(),
        }
    }

    #[test]
    fn hotbar_is_core_then_spec_then_empty() {
        let bar = class().hotbar("main");
        assert_eq!(bar.len(), HOTBAR_SLOTS);
        assert_eq!(bar[0].as_deref(), Some("core1"));
        assert_eq!(bar[5].as_deref(), Some("spec1"));
        assert_eq!(bar[8], None);
    }

    #[test]
    fn valid_class_has_no_problems() {
        assert!(class().validate().is_empty());
    }

    #[test]
    fn wrong_ability_counts_are_reported() {
        let mut c = class();
        c.core.pop();
        c.specializations[0].abilities.push("extra".into());
        c.default_spec = "missing".into();
        assert_eq!(c.validate().len(), 3, "{:?}", c.validate());
    }
}
