//! Ability definitions. An ability is timing (GCD, cast, cooldown, range)
//! plus a list of **effects** — damage, healing, shields, statuses, taunts —
//! each with a rule for who it lands on. New abilities are new data, not
//! new code.

use serde::{Deserialize, Serialize};

use crate::data::Problems;

/// Who an ability is aimed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum TargetKind {
    /// Needs a hostile target.
    Enemy,
    /// A friendly target; with no friendly target selected it lands on you.
    Ally,
    /// Only the user; no target needed.
    Myself,
}

/// Where an area effect is centred.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum Centre {
    Target,
    Me,
}

/// Who receives one effect.
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize, Default)]
pub enum Recipients {
    /// The ability's target (for `Myself` abilities, the user).
    #[default]
    Target,
    /// The user.
    Myself,
    /// Every enemy of the user within `radius` metres of the centre.
    EnemiesAround { centre: Centre, radius: f32 },
    /// The user and every ally within `radius` metres of the centre.
    AlliesAround { centre: Centre, radius: f32 },
}

/// One thing an ability does.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub enum Effect {
    Damage {
        potency: u32,
    },
    Heal {
        potency: u32,
    },
    /// Absorbs damage; the amount comes from potency like a heal, and the
    /// named status says how long it lasts.
    Shield {
        potency: u32,
        status: String,
    },
    /// Apply a buff or debuff (see `assets/data/statuses/`).
    ApplyStatus {
        status: String,
    },
    /// Make the recipients focus the user (top of their threat list).
    Taunt,
}

/// An effect and who it lands on.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct EffectEntry {
    #[serde(default)]
    pub to: Recipients,
    pub effect: Effect,
}

/// A combo step: stronger when used straight after another ability.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Combo {
    /// The ability that must come just before (as the previous GCD).
    pub after: String,
    /// Damage potency used instead when the combo is fulfilled.
    pub potency: u32,
}

/// One ability, as written in `assets/data/abilities/*.ron`.
#[derive(Debug, Clone, Deserialize)]
pub struct AbilityDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// Uses (and is blocked by) the global cooldown.
    pub on_gcd: bool,
    /// Seconds to cast; 0 means instant. Moving cancels a cast.
    #[serde(default)]
    pub cast_time: f32,
    /// The ability's own cooldown in seconds; 0 means none.
    #[serde(default)]
    pub cooldown: f32,
    /// Metres to the edge of the target's ring. Ignored for `Myself`.
    #[serde(default)]
    pub range: f32,
    pub target: TargetKind,
    pub effects: Vec<EffectEntry>,
    #[serde(default)]
    pub combo: Option<Combo>,
    /// Visual effect name (client only).
    #[serde(default)]
    pub vfx: String,
}

impl AbilityDef {
    pub fn is_instant(&self) -> bool {
        self.cast_time <= 0.0
    }

    /// Every status this ability refers to (for cross-file checks).
    pub fn statuses(&self) -> impl Iterator<Item = &str> {
        self.effects.iter().filter_map(|e| match &e.effect {
            Effect::ApplyStatus { status } | Effect::Shield { status, .. } => Some(status.as_str()),
            _ => None,
        })
    }

    /// Damage potency of the first damage effect, if any.
    pub fn damage_potency(&self) -> Option<u32> {
        self.effects.iter().find_map(|e| match e.effect {
            Effect::Damage { potency } => Some(potency),
            _ => None,
        })
    }

    /// Problems with this ability, each prefixed with its id.
    pub fn problems(&self) -> Vec<String> {
        let mut p = Problems::default();
        if self.id.trim().is_empty() {
            p.push("an ability has an empty `id`");
        }
        p.non_negative("cast_time", self.cast_time);
        p.non_negative("cooldown", self.cooldown);
        p.non_negative("range", self.range);
        if self.effects.is_empty() {
            p.push("`effects` is empty");
        }
        for (i, entry) in self.effects.iter().enumerate() {
            if let Recipients::EnemiesAround { radius, .. }
            | Recipients::AlliesAround { radius, .. } = entry.to
            {
                p.positive(&format!("effects[{i}] radius"), radius);
            }
            if self.target == TargetKind::Myself
                && matches!(
                    entry.to,
                    Recipients::EnemiesAround {
                        centre: Centre::Target,
                        ..
                    }
                )
            {
                p.push(format!(
                    "effects[{i}] is centred on a target, but the ability targets yourself"
                ));
            }
        }
        if self.combo.is_some() && self.damage_potency().is_none() {
            p.push("has a `combo` but no Damage effect for it to boost");
        }
        p.0.into_iter()
            .map(|m| format!("ability `{}`: {m}", self.id))
            .collect()
    }

    /// A short summary of what the ability does, for tooltips.
    pub fn summary(&self, status_name: impl Fn(&str) -> String) -> String {
        let mut parts = Vec::new();
        for entry in &self.effects {
            let who = match entry.to {
                Recipients::Target => String::new(),
                Recipients::Myself => " to yourself".to_owned(),
                Recipients::EnemiesAround { centre, radius } => {
                    format!(" to enemies within {radius:.0}m of {}", centre_name(centre))
                }
                Recipients::AlliesAround { centre, radius } => {
                    format!(" to allies within {radius:.0}m of {}", centre_name(centre))
                }
            };
            let what = match &entry.effect {
                Effect::Damage { potency } => format!("Deals {potency} potency damage"),
                Effect::Heal { potency } => format!("Heals for {potency} potency"),
                Effect::Shield { potency, .. } => format!("Grants a {potency} potency shield"),
                Effect::ApplyStatus { status } => format!("Applies {}", status_name(status)),
                Effect::Taunt => "Taunts".to_owned(),
            };
            parts.push(format!("{what}{who}."));
        }
        if let Some(combo) = &self.combo {
            parts.push(format!(
                "Combo after {}: {} potency.",
                combo.after, combo.potency
            ));
        }
        parts.join("\n")
    }
}

fn centre_name(centre: Centre) -> &'static str {
    match centre {
        Centre::Target => "the target",
        Centre::Me => "you",
    }
}

#[cfg(test)]
pub mod test_support {
    use super::*;

    /// A simple single-target damage ability for tests.
    pub fn damage_ability(id: &str, on_gcd: bool, cast_time: f32, cooldown: f32) -> AbilityDef {
        AbilityDef {
            id: id.into(),
            name: id.into(),
            description: String::new(),
            on_gcd,
            cast_time,
            cooldown,
            range: 3.0,
            target: TargetKind::Enemy,
            effects: vec![EffectEntry {
                to: Recipients::Target,
                effect: Effect::Damage { potency: 100 },
            }],
            combo: None,
            vfx: String::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::damage_ability;
    use super::*;
    use crate::data::{Validate, parse_ron};
    use std::path::Path;

    struct List(Vec<AbilityDef>);
    impl<'de> Deserialize<'de> for List {
        fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
            Vec::<AbilityDef>::deserialize(d).map(List)
        }
    }
    impl Validate for List {
        fn validate(&self) -> Vec<String> {
            self.0.iter().flat_map(AbilityDef::problems).collect()
        }
    }

    #[test]
    fn parses_effects_from_ron() {
        let text = r#"[(
            id: "flame_wave", name: "Flame Wave", on_gcd: true, cast_time: 2.0, range: 25.0,
            target: Enemy,
            effects: [
                (to: EnemiesAround(centre: Target, radius: 5.0), effect: Damage(potency: 220)),
                (effect: ApplyStatus(status: "burn")),
            ],
        )]"#;
        let list: List = parse_ron(text, Path::new("test.ron")).unwrap();
        let ability = &list.0[0];
        assert_eq!(ability.effects.len(), 2);
        assert_eq!(
            ability.effects[1].to,
            Recipients::Target,
            "`to` defaults to the target"
        );
        assert_eq!(ability.statuses().collect::<Vec<_>>(), vec!["burn"]);
        assert_eq!(ability.damage_potency(), Some(220));
    }

    #[test]
    fn validation_catches_empty_effects_and_bad_radius() {
        let mut a = damage_ability("a", true, 0.0, 0.0);
        a.effects.clear();
        assert_eq!(a.problems().len(), 1);
        let mut b = damage_ability("b", true, 0.0, 0.0);
        b.effects[0].to = Recipients::EnemiesAround {
            centre: Centre::Me,
            radius: 0.0,
        };
        assert_eq!(b.problems().len(), 1);
    }

    #[test]
    fn self_abilities_cannot_centre_on_a_target() {
        let mut a = damage_ability("a", true, 0.0, 0.0);
        a.target = TargetKind::Myself;
        a.effects[0].to = Recipients::EnemiesAround {
            centre: Centre::Target,
            radius: 5.0,
        };
        assert_eq!(a.problems().len(), 1);
    }

    #[test]
    fn summary_describes_effects() {
        let mut a = damage_ability("a", true, 0.0, 0.0);
        a.effects.push(EffectEntry {
            to: Recipients::Myself,
            effect: Effect::ApplyStatus {
                status: "focus".into(),
            },
        });
        let text = a.summary(|id| format!("[{id}]"));
        assert!(text.contains("100 potency damage"), "{text}");
        assert!(text.contains("Applies [focus] to yourself"), "{text}");
    }
}
