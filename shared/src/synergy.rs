//! Party synergy: a party that is missing a role (durable, sustain or
//! damage) gets a smaller bonus that makes up for it, so any mix of classes
//! can play, while a mixed party still does best. Numbers live in
//! `assets/data/synergy.ron`; the bonuses are ordinary statuses.
//!
//! Until parties arrive with multiplayer (M11), "the party" is everyone in
//! the same zone — on your own you are a party of one.

use std::collections::HashMap;

use serde::Deserialize;

use crate::classes::Role;
use crate::data::{Problems, Validate};

/// `assets/data/synergy.ron`.
#[derive(Debug, Clone, Deserialize)]
pub struct SynergyDef {
    /// A role counts as covered when the party's weights for it add up to
    /// at least this much.
    pub covered_at: f32,
    /// The status everyone in the party gets while a role is not covered.
    pub missing: HashMap<Role, String>,
}

impl Validate for SynergyDef {
    fn validate(&self) -> Vec<String> {
        let mut p = Problems::default();
        p.positive("covered_at", self.covered_at);
        p.0
    }
}

impl SynergyDef {
    /// Every status synergy can hand out (durable, sustain, damage order).
    pub fn statuses(&self) -> impl Iterator<Item = &str> {
        Role::ALL
            .into_iter()
            .filter_map(|role| self.missing.get(&role).map(String::as_str))
    }

    /// The bonus statuses for a party whose members count as these role
    /// weights (one list per member), in a fixed order.
    pub fn bonuses<'a>(&self, members: impl IntoIterator<Item = &'a [(Role, f32)]>) -> Vec<&str> {
        let coverage = coverage(members);
        Role::ALL
            .into_iter()
            .filter(|role| coverage.get(role).copied().unwrap_or(0.0) < self.covered_at)
            .filter_map(|role| self.missing.get(&role).map(String::as_str))
            .collect()
    }
}

/// How much of each role a party covers.
pub fn coverage<'a>(members: impl IntoIterator<Item = &'a [(Role, f32)]>) -> HashMap<Role, f32> {
    let mut total = HashMap::new();
    for member in members {
        for (role, weight) in member {
            *total.entry(*role).or_insert(0.0) += weight;
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rules() -> SynergyDef {
        SynergyDef {
            covered_at: 0.5,
            missing: HashMap::from([
                (Role::Durable, "no_durable".to_owned()),
                (Role::Sustain, "no_sustain".to_owned()),
                (Role::Damage, "no_damage".to_owned()),
            ]),
        }
    }

    #[test]
    fn a_lone_damage_dealer_gets_the_other_two_bonuses() {
        let me = [(Role::Damage, 1.0)];
        assert_eq!(rules().bonuses([&me[..]]), vec!["no_durable", "no_sustain"]);
    }

    #[test]
    fn a_full_party_gets_nothing() {
        let tank = [(Role::Durable, 1.0)];
        let healer = [(Role::Sustain, 1.0)];
        let damage = [(Role::Damage, 1.0)];
        assert!(
            rules()
                .bonuses([&tank[..], &healer[..], &damage[..]])
                .is_empty()
        );
    }

    #[test]
    fn partial_roles_add_up() {
        // Two hybrids that are each 0.3 sustain cover it together.
        let hybrid = [(Role::Durable, 0.7), (Role::Sustain, 0.3)];
        assert_eq!(
            rules().bonuses([&hybrid[..], &hybrid[..]]),
            vec!["no_damage"]
        );
        assert_eq!(
            rules().bonuses([&hybrid[..]]),
            vec!["no_sustain", "no_damage"]
        );
    }
}
