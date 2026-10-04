//! Boss fights ("encounters"), written as data: phases that change at
//! health thresholds, each with a timeline of attacks that repeats, plus an
//! enrage timer. This module also keeps track of where a fight is up to.

use bevy::math::Vec3;
use serde::Deserialize;

use crate::data::{Problems, Validate};

/// Something the boss does.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub enum EncounterAction {
    /// Use an ability on whoever it is angriest at.
    Use(String),
    /// Call in an extra enemy.
    Spawn { enemy: String, at: Vec3 },
    /// Show a line of text to everyone in the fight.
    Say(String),
}

/// "At this many seconds into the phase, do this."
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct TimelineEntry {
    pub at: f32,
    pub action: EncounterAction,
}

#[derive(Debug, Clone, Deserialize)]
pub struct PhaseDef {
    pub name: String,
    /// The phase lasts until the boss's health drops to this percent.
    /// The last phase uses 0.
    pub until_health: f32,
    /// Done once when the phase begins.
    #[serde(default)]
    pub on_start: Vec<EncounterAction>,
    pub timeline: Vec<TimelineEntry>,
    /// After this many seconds the timeline starts again from the top.
    pub loop_after: f32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Enrage {
    /// Seconds after the fight starts.
    pub after: f32,
    pub ability: String,
    /// Shown as a warning when the enrage begins.
    #[serde(default)]
    pub message: Option<String>,
}

/// One boss fight (`assets/data/encounters/<id>.ron`).
#[derive(Debug, Clone, Deserialize)]
pub struct EncounterDef {
    pub name: String,
    /// The boss (file name in `assets/data/enemies/`).
    pub boss: String,
    pub boss_position: Vec3,
    #[serde(default)]
    pub boss_yaw: f32,
    /// Extra boss health for each player beyond the first, in percent.
    #[serde(default)]
    pub extra_health_per_player: f32,
    #[serde(default)]
    pub pull_message: Option<String>,
    #[serde(default)]
    pub victory_message: Option<String>,
    pub phases: Vec<PhaseDef>,
    #[serde(default)]
    pub enrage: Option<Enrage>,
}

impl EncounterDef {
    /// Every ability the fight uses (for cross-file checks).
    pub fn abilities(&self) -> impl Iterator<Item = &str> {
        let actions = self.phases.iter().flat_map(|p| {
            p.on_start
                .iter()
                .chain(p.timeline.iter().map(|t| &t.action))
        });
        actions
            .filter_map(|a| match a {
                EncounterAction::Use(id) => Some(id.as_str()),
                _ => None,
            })
            .chain(self.enrage.iter().map(|e| e.ability.as_str()))
    }

    /// Every enemy the fight spawns, the boss included.
    pub fn enemies(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.boss.as_str()).chain(self.phases.iter().flat_map(|p| {
            p.on_start
                .iter()
                .chain(p.timeline.iter().map(|t| &t.action))
                .filter_map(|a| match a {
                    EncounterAction::Spawn { enemy, .. } => Some(enemy.as_str()),
                    _ => None,
                })
        }))
    }

    /// Which phase the boss should be in at this health (percent).
    pub fn phase_for_health(&self, health_percent: f32) -> usize {
        self.phases
            .iter()
            .position(|p| health_percent > p.until_health)
            .unwrap_or(self.phases.len().saturating_sub(1))
    }

    /// Boss health for a party of this size.
    pub fn scaled_health(&self, base: u32, players: usize) -> u32 {
        let extra = players.saturating_sub(1) as f32 * self.extra_health_per_player / 100.0;
        (base as f32 * (1.0 + extra)).round() as u32
    }
}

impl Validate for EncounterDef {
    fn validate(&self) -> Vec<String> {
        let mut p = Problems::default();
        if self.phases.is_empty() {
            p.push("`phases` is empty");
        }
        p.non_negative("extra_health_per_player", self.extra_health_per_player);
        let mut previous = 100.0;
        for (i, phase) in self.phases.iter().enumerate() {
            if !(0.0..previous).contains(&phase.until_health) {
                p.push(format!(
                    "phases[{i}] `until_health` must be lower than the phase before (and 0 or more)"
                ));
            }
            previous = phase.until_health;
            p.positive(&format!("phases[{i}].loop_after"), phase.loop_after);
            let mut last = 0.0;
            for (j, entry) in phase.timeline.iter().enumerate() {
                if entry.at < last {
                    p.push(format!(
                        "phases[{i}].timeline[{j}] is earlier than the entry before it"
                    ));
                }
                if entry.at >= phase.loop_after {
                    p.push(format!(
                        "phases[{i}].timeline[{j}] is at or after `loop_after`, so it would never happen"
                    ));
                }
                last = entry.at;
            }
        }
        if self.phases.last().is_some_and(|ph| ph.until_health != 0.0) {
            p.push("the last phase must have `until_health: 0`");
        }
        if let Some(enrage) = &self.enrage {
            p.positive("enrage.after", enrage.after);
        }
        p.0
    }
}

/// Where a fight has got to.
#[derive(Debug, Clone, PartialEq)]
pub struct Progress {
    pub phase: usize,
    /// When the current pass through the phase's timeline began.
    pub cycle_start: f64,
    /// The next timeline entry to do.
    pub next: usize,
    /// When the fight began (for the enrage timer and the clear time).
    pub started: f64,
    pub enraged: bool,
}

impl Progress {
    pub fn new(now: f64) -> Self {
        Self {
            phase: 0,
            cycle_start: now,
            next: 0,
            started: now,
            enraged: false,
        }
    }

    /// Start a new phase now.
    pub fn enter_phase(&mut self, phase: usize, now: f64) {
        self.phase = phase;
        self.cycle_start = now;
        self.next = 0;
    }

    /// The timeline entry that is due now, if any. Call [`advance`](Self::advance)
    /// once it has been done; if the boss is busy, leave it and it stays due.
    pub fn due<'a>(&mut self, def: &'a EncounterDef, now: f64) -> Option<&'a EncounterAction> {
        let phase = def.phases.get(self.phase)?;
        if self.next >= phase.timeline.len() {
            // Finished this pass: start again once the loop time is up.
            if now < self.cycle_start + f64::from(phase.loop_after) {
                return None;
            }
            self.cycle_start += f64::from(phase.loop_after);
            self.next = 0;
        }
        let entry = phase.timeline.get(self.next)?;
        (now >= self.cycle_start + f64::from(entry.at)).then_some(&entry.action)
    }

    pub fn advance(&mut self) {
        self.next += 1;
    }

    /// Is it time to enrage (and hasn't it happened yet)?
    pub fn enrage_due(&self, def: &EncounterDef, now: f64) -> bool {
        def.enrage
            .as_ref()
            .is_some_and(|e| !self.enraged && now - self.started >= f64::from(e.after))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::parse_ron;
    use std::path::Path;

    fn def() -> EncounterDef {
        parse_ron(
            r#"(
                name: "Test", boss: "boss", boss_position: (0.0, 0.0, 0.0),
                extra_health_per_player: 50,
                phases: [
                    (name: "One", until_health: 60, loop_after: 10.0, timeline: [
                        (at: 2.0, action: Use("slam")),
                        (at: 6.0, action: Say("Grr")),
                    ]),
                    (name: "Two", until_health: 0, loop_after: 5.0,
                     on_start: [Spawn(enemy: "add", at: (1.0, 0.0, 1.0))],
                     timeline: [(at: 1.0, action: Use("burst"))]),
                ],
                enrage: Some((after: 300.0, ability: "wrath")),
            )"#,
            Path::new("test.ron"),
        )
        .unwrap()
    }

    #[test]
    fn timeline_runs_in_order_and_loops() {
        let def = def();
        let mut p = Progress::new(0.0);
        assert_eq!(p.due(&def, 1.0), None);
        assert_eq!(p.due(&def, 2.0), Some(&EncounterAction::Use("slam".into())));
        p.advance();
        assert_eq!(p.due(&def, 3.0), None);
        assert_eq!(p.due(&def, 6.5), Some(&EncounterAction::Say("Grr".into())));
        p.advance();
        assert_eq!(p.due(&def, 9.0), None, "waits for the loop");
        assert_eq!(
            p.due(&def, 12.0),
            Some(&EncounterAction::Use("slam".into())),
            "looped"
        );
    }

    #[test]
    fn a_busy_boss_does_the_action_late_instead_of_skipping_it() {
        let def = def();
        let mut p = Progress::new(0.0);
        assert!(p.due(&def, 2.0).is_some());
        // Not advanced (boss was busy): still due a moment later.
        assert!(p.due(&def, 2.5).is_some());
    }

    #[test]
    fn phases_follow_health() {
        let def = def();
        assert_eq!(def.phase_for_health(100.0), 0);
        assert_eq!(def.phase_for_health(61.0), 0);
        assert_eq!(def.phase_for_health(60.0), 1);
        assert_eq!(def.phase_for_health(0.0), 1);
    }

    #[test]
    fn health_scales_with_party_size() {
        let def = def();
        assert_eq!(def.scaled_health(10_000, 1), 10_000);
        assert_eq!(def.scaled_health(10_000, 4), 25_000);
    }

    #[test]
    fn enrage_comes_once() {
        let def = def();
        let mut p = Progress::new(10.0);
        assert!(!p.enrage_due(&def, 200.0));
        assert!(p.enrage_due(&def, 310.0));
        p.enraged = true;
        assert!(!p.enrage_due(&def, 400.0));
    }

    #[test]
    fn lists_abilities_and_enemies() {
        let def = def();
        let abilities: Vec<_> = def.abilities().collect();
        assert_eq!(abilities, vec!["slam", "burst", "wrath"]);
        let enemies: Vec<_> = def.enemies().collect();
        assert_eq!(enemies, vec!["boss", "add"]);
    }

    #[test]
    fn validation_catches_mistakes() {
        let mut bad = def();
        bad.phases[1].until_health = 5.0;
        bad.phases[0].timeline[0].at = 20.0;
        let problems = bad.validate();
        assert_eq!(problems.len(), 3, "{problems:?}");
    }
}
