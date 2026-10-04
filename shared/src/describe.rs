//! Plain-language descriptions of abilities and statuses for tooltips,
//! using the numbers a player will actually see (their power and current
//! buffs included), e.g. "Deals 220 damage to the target."

use crate::abilities::{AbilityDef, Centre, Effect, Recipients};
use crate::formulas::{healing, outgoing_damage};
use crate::gamedata::GameData;
use crate::statuses::{Modifiers, StatusDef, Tick};

/// Describes things from one character's point of view.
pub struct Describer<'a> {
    pub data: &'a GameData,
    /// The character's power (100 = the listed amounts).
    pub power: f32,
    /// The character's current buffs and debuffs.
    pub modifiers: Modifiers,
}

impl Describer<'_> {
    fn damage(&self, amount: u32) -> u32 {
        outgoing_damage(
            amount,
            self.power,
            self.modifiers,
            false,
            &self.data.config.combat,
        )
    }

    fn heal(&self, amount: u32) -> u32 {
        healing(
            amount,
            self.power,
            self.modifiers,
            Modifiers::default(),
            false,
            &self.data.config.combat,
        )
    }

    fn ability_name(&self, id: &str) -> String {
        self.data
            .abilities
            .get(id)
            .map_or_else(|| id.to_owned(), |a| a.name.clone())
    }

    /// One line per effect, plus the combo bonus if there is one.
    pub fn ability(&self, ability: &AbilityDef) -> String {
        let mut lines = Vec::new();
        for entry in &ability.effects {
            let who = match entry.to {
                Recipients::Target => match ability.target {
                    crate::abilities::TargetKind::Enemy => " to the target".to_owned(),
                    crate::abilities::TargetKind::Ally => " to the target (or you)".to_owned(),
                    crate::abilities::TargetKind::Myself => " to yourself".to_owned(),
                },
                Recipients::Myself => " to yourself".to_owned(),
                Recipients::EnemiesAround { centre, radius } => {
                    format!(" to enemies within {radius:.0}m of {}", centre_name(centre))
                }
                Recipients::AlliesAround { centre, radius } => {
                    format!(
                        " to you and allies within {radius:.0}m of {}",
                        centre_name(centre)
                    )
                }
            };
            let line = match &entry.effect {
                Effect::Damage { amount } => format!("Deals {} damage{who}.", self.damage(*amount)),
                Effect::Heal { amount } => format!("Heals {}{who}.", self.heal(*amount)),
                Effect::Shield { amount, status } => {
                    let seconds = self.data.statuses.get(status).map_or(0.0, |s| s.duration);
                    format!(
                        "Gives a shield that absorbs {} damage{who} for {seconds:.0}s.",
                        self.heal(*amount)
                    )
                }
                Effect::ApplyStatus { status } => match self.data.statuses.get(status) {
                    Some(def) => format!("Applies {}{who}: {}", def.name, self.status(def)),
                    None => format!("Applies {status}{who}."),
                },
                Effect::Taunt => format!("Makes enemies attack you{who}."),
            };
            lines.push(line);
        }
        if let Some(combo) = &ability.combo {
            lines.push(format!(
                "Right after {}: deals {} damage instead.",
                self.ability_name(&combo.after),
                self.damage(combo.amount)
            ));
        }
        lines.join("\n")
    }

    /// What a status does, e.g. "55 damage every 3s for 18s."
    pub fn status(&self, def: &StatusDef) -> String {
        let interval = self.data.config.combat.tick_interval;
        let mut parts = Vec::new();
        match def.tick {
            Some(Tick::Damage { amount }) => {
                parts.push(format!(
                    "{} damage every {interval:.0}s",
                    self.damage(amount)
                ));
            }
            Some(Tick::Heal { amount }) => {
                parts.push(format!("heals {} every {interval:.0}s", self.heal(amount)));
            }
            None => {}
        }
        let m = def.modifiers;
        for (value, label) in [
            (m.damage_dealt, "damage dealt"),
            (m.damage_taken, "damage taken"),
            (m.healing_done, "healing done"),
            (m.healing_received, "healing received"),
        ] {
            if let Some(change) = percent_change(value) {
                parts.push(format!("{change} {label}"));
            }
        }
        if parts.is_empty() {
            format!("lasts {:.0}s.", def.duration)
        } else {
            format!("{} for {:.0}s.", parts.join(", "), def.duration)
        }
    }
}

/// "+20%" for 1.2, "-30%" for 0.7, nothing for 1.0.
pub fn percent_change(multiplier: f32) -> Option<String> {
    let percent = ((multiplier - 1.0) * 100.0).round() as i32;
    match percent {
        0 => None,
        p if p > 0 => Some(format!("+{p}%")),
        p => Some(format!("{p}%")),
    }
}

fn centre_name(centre: Centre) -> &'static str {
    match centre {
        Centre::Target => "the target",
        Centre::Me => "you",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::find_assets_dir;

    fn data() -> GameData {
        GameData::load(&find_assets_dir().unwrap()).unwrap()
    }

    #[test]
    fn percent_changes_read_naturally() {
        assert_eq!(percent_change(1.2).as_deref(), Some("+20%"));
        assert_eq!(percent_change(0.7).as_deref(), Some("-30%"));
        assert_eq!(percent_change(1.0), None);
    }

    #[test]
    fn descriptions_include_power_and_buffs() {
        let data = data();
        let mut ability = data.abilities.values().next().unwrap().clone();
        ability.effects = vec![crate::abilities::EffectEntry {
            to: Recipients::Target,
            effect: Effect::Damage { amount: 200 },
        }];
        ability.target = crate::abilities::TargetKind::Enemy;
        ability.combo = None;
        let plain = Describer {
            data: &data,
            power: 100.0,
            modifiers: Modifiers::default(),
        };
        assert_eq!(plain.ability(&ability), "Deals 200 damage to the target.");
        let strong = Describer {
            data: &data,
            power: 110.0,
            modifiers: Modifiers {
                damage_dealt: 1.5,
                ..Default::default()
            },
        };
        assert_eq!(strong.ability(&ability), "Deals 330 damage to the target.");
    }

    #[test]
    fn every_shipped_ability_has_a_description() {
        let data = data();
        let describer = Describer {
            data: &data,
            power: 100.0,
            modifiers: Modifiers::default(),
        };
        for ability in data.abilities.values() {
            let text = describer.ability(ability);
            assert!(!text.is_empty(), "{}", ability.id);
            assert!(!text.contains("amount"), "{}: {text}", ability.id);
        }
    }
}
