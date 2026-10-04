//! Classes and specializations. A character can be any class: the class
//! is the flame burning in their lantern.

use bevy::prelude::Component;
use serde::{Deserialize, Serialize};

use std::collections::HashMap;

use crate::combat::Reject;
use crate::components::HOTBAR_SLOTS;
use crate::data::{Problems, Validate};

/// Abilities every class has in its core kit.
pub const CORE_ABILITIES: usize = 5;
/// Abilities each specialization adds.
pub const SPEC_ABILITIES: usize = 3;
/// Abilities borrowed from a secondary class (hotbar slots 9 and 0).
pub const SECONDARY_ABILITIES: usize = 2;

/// What a class leans towards (soft roles: no content requires any).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub enum Role {
    Durable,
    Sustain,
    Damage,
}

impl Role {
    pub const ALL: [Role; 3] = [Role::Durable, Role::Sustain, Role::Damage];
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
    /// How much this specialization counts towards each role for party
    /// synergy, e.g. `{Durable: 0.7, Sustain: 0.3}`. Empty: fully the
    /// class's own role.
    #[serde(default)]
    pub roles: HashMap<Role, f32>,
}

/// An ability other classes may borrow, once this class reaches `level`.
#[derive(Debug, Clone, Deserialize)]
pub struct Lendable {
    pub ability: String,
    pub level: u32,
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
    /// Abilities other classes can borrow when this is their secondary class.
    #[serde(default)]
    pub lendable: Vec<Lendable>,
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

    /// What a specialization counts as for party synergy.
    pub fn role_weights(&self, spec: &str) -> Vec<(Role, f32)> {
        match self.spec(spec).filter(|s| !s.roles.is_empty()) {
            Some(s) => {
                let mut weights: Vec<_> = s.roles.iter().map(|(r, w)| (*r, *w)).collect();
                weights.sort_by_key(|(r, _)| *r as u8);
                weights
            }
            None => vec![(self.role, 1.0)],
        }
    }

    /// The level this class needs before others can borrow `ability`.
    pub fn lend_level(&self, ability: &str) -> Option<u32> {
        self.lendable
            .iter()
            .find(|l| l.ability == ability)
            .map(|l| l.level)
    }

    /// Hotbar layout: 5 core abilities, 3 from the specialization, then
    /// empty slots (secondary class and lantern abilities are added by
    /// `GameData::hotbar`).
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
        for spec in &self.specializations {
            for (role, weight) in &spec.roles {
                if !(0.0..=1.0).contains(weight) {
                    p.push(format!(
                        "specialization `{}`: role weight for {role:?} must be between 0 and 1",
                        spec.id
                    ));
                }
            }
        }
        for lend in &self.lendable {
            if lend.level == 0 {
                p.push(format!(
                    "lendable `{}` needs a level of 1 or more",
                    lend.ability
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

/// A class's choice of secondary class: which class it borrows from and
/// the abilities it put in its two secondary slots.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SecondaryChoice {
    pub class: String,
    pub abilities: [Option<String>; SECONDARY_ABILITIES],
}

/// Each class's secondary choice for one character (keyed by main class).
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct Secondaries(pub HashMap<String, SecondaryChoice>);

/// Can a character playing `main` borrow these abilities from `choice.class`,
/// with that class at `secondary_level`?
pub fn check_secondary(
    main: &str,
    choice: &SecondaryChoice,
    secondary: &ClassDef,
    secondary_level: u32,
) -> Result<(), Reject> {
    if choice.class == main {
        return Err(Reject::SameClass);
    }
    let picked: Vec<&String> = choice.abilities.iter().flatten().collect();
    if picked.len() == 2 && picked[0] == picked[1] {
        return Err(Reject::NotLendable);
    }
    for ability in picked {
        match secondary.lend_level(ability) {
            None => return Err(Reject::NotLendable),
            Some(level) if level > secondary_level => return Err(Reject::LevelTooLow),
            Some(_) => {}
        }
    }
    Ok(())
}

/// A character's current class and specialization.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct CurrentClass {
    pub class: String,
    pub spec: String,
}

/// The specialization each class last chose (classes not listed use their
/// `default_spec`).
#[derive(Component, Debug, Clone, Default, PartialEq, Eq)]
pub struct ChosenSpecs(pub std::collections::HashMap<String, String>);

impl ChosenSpecs {
    /// The specialization a class plays as: its chosen one if that still
    /// exists, otherwise its default.
    pub fn spec_of(&self, class_id: &str, class: &ClassDef) -> String {
        self.0
            .get(class_id)
            .filter(|spec| class.specializations.iter().any(|s| &s.id == *spec))
            .cloned()
            .unwrap_or_else(|| class.default_spec.clone())
    }
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
                roles: HashMap::new(),
            }],
            default_spec: "main".into(),
            lendable: vec![
                Lendable {
                    ability: "core1".into(),
                    level: 1,
                },
                Lendable {
                    ability: "spec1".into(),
                    level: 10,
                },
            ],
        }
    }

    fn choice(abilities: [Option<&str>; 2]) -> SecondaryChoice {
        SecondaryChoice {
            class: "test".into(),
            abilities: abilities.map(|a| a.map(str::to_owned)),
        }
    }

    #[test]
    fn secondary_abilities_must_be_lendable_and_unlocked() {
        let lender = class();
        assert_eq!(
            check_secondary("other", &choice([Some("core1"), None]), &lender, 1),
            Ok(())
        );
        assert_eq!(
            check_secondary("other", &choice([Some("core2"), None]), &lender, 30),
            Err(Reject::NotLendable)
        );
        assert_eq!(
            check_secondary("other", &choice([Some("spec1"), None]), &lender, 9),
            Err(Reject::LevelTooLow)
        );
        assert_eq!(
            check_secondary(
                "other",
                &choice([Some("core1"), Some("core1")]),
                &lender,
                30
            ),
            Err(Reject::NotLendable)
        );
        assert_eq!(
            check_secondary("test", &choice([None, None]), &lender, 30),
            Err(Reject::SameClass)
        );
    }

    #[test]
    fn role_weights_default_to_the_class_role() {
        let mut c = class();
        assert_eq!(c.role_weights("main"), vec![(Role::Damage, 1.0)]);
        c.specializations[0].roles = HashMap::from([(Role::Durable, 0.7), (Role::Sustain, 0.3)]);
        assert_eq!(
            c.role_weights("main"),
            vec![(Role::Durable, 0.7), (Role::Sustain, 0.3)]
        );
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
