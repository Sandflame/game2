//! Quests and dialogue, written as data.
//!
//! A quest (`assets/data/quests/<id>.ron`) is offered by one person (its
//! `giver`), may need another quest done first, and is a list of steps done
//! in order: talk to someone, defeat some enemies, go somewhere, or win a
//! boss fight. Talking plays a dialogue (`assets/data/dialogue/*.ron`), a
//! list of lines that players can click through or skip; progress is
//! recorded either way.
//!
//! Everything here is plain functions on a [`QuestLog`], so the rules are
//! easy to test; the server wraps them in systems.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use bevy::prelude::Component;
use serde::Deserialize;

use crate::data::{Problems, Validate};
use crate::items::LootEntry;
use crate::level::base_zone;

/// One line of a dialogue.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct Line {
    /// Who says it (shown above the line).
    pub who: String,
    pub says: String,
}

/// A conversation: lines shown one after another.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(transparent)]
pub struct DialogueDef(pub Vec<Line>);

/// A file of dialogues, by id (`assets/data/dialogue/*.ron`).
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct DialogueFile(pub HashMap<String, DialogueDef>);

impl Validate for DialogueFile {
    fn validate(&self) -> Vec<String> {
        let mut p = Problems::default();
        for (id, dialogue) in &self.0 {
            if dialogue.0.is_empty() {
                p.push(format!("dialogue `{id}` has no lines"));
            }
            for (i, line) in dialogue.0.iter().enumerate() {
                if line.says.trim().is_empty() {
                    p.push(format!("dialogue `{id}` line {i} is empty"));
                }
            }
        }
        p.0
    }
}

/// What a step asks for.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub enum Goal {
    /// Talk to this person (their `id` in zone data).
    Talk(String),
    /// Defeat this many of an enemy type (file name in `enemies/`).
    Defeat { enemy: String, count: u32 },
    /// Go to this zone (file name in `zones/`).
    Reach(String),
    /// Win this boss fight (file name in `encounters/`).
    Win(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct StepDef {
    pub goal: Goal,
    /// Shown in the quest tracker, e.g. "Talk to Root Keeper Fen".
    pub text: String,
    /// Played when a `Talk` step is done.
    #[serde(default)]
    pub dialogue: Option<String>,
}

/// One quest (`assets/data/quests/<id>.ron`).
#[derive(Debug, Clone, Deserialize)]
pub struct QuestDef {
    pub name: String,
    /// A sentence or two for the quest log.
    pub summary: String,
    /// The person who offers it (their `id` in zone data).
    pub giver: String,
    /// Played when the quest is offered (and accepted).
    pub offer: String,
    /// Quests that must be finished before this one is offered.
    #[serde(default)]
    pub after: Vec<String>,
    pub steps: Vec<StepDef>,
    /// Experience for finishing it (for the class being played).
    #[serde(default)]
    pub xp: u32,
    /// Items for finishing it (each `chance` in percent; 100 = always).
    #[serde(default)]
    pub items: Vec<LootEntry>,
}

impl Validate for QuestDef {
    fn validate(&self) -> Vec<String> {
        let mut p = Problems::default();
        if self.steps.is_empty() {
            p.push("`steps` is empty");
        }
        for (i, step) in self.steps.iter().enumerate() {
            if let Goal::Defeat { count: 0, .. } = step.goal {
                p.push(format!("steps[{i}] asks to defeat 0 enemies"));
            }
            if step.dialogue.is_some() && !matches!(step.goal, Goal::Talk(_)) {
                p.push(format!(
                    "steps[{i}] has a `dialogue` but only `Talk` steps play one"
                ));
            }
        }
        for (i, entry) in self.items.iter().enumerate() {
            if !(0.0..=100.0).contains(&entry.chance) {
                p.push(format!("items[{i}].chance must be between 0 and 100"));
            }
        }
        p.0
    }
}

impl QuestDef {
    /// Every dialogue the quest plays (for cross-file checks).
    pub fn dialogues(&self) -> impl Iterator<Item = &str> {
        std::iter::once(self.offer.as_str())
            .chain(self.steps.iter().filter_map(|s| s.dialogue.as_deref()))
    }
}

/// A quest in progress: which step, and how far through it (for `Defeat`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Active {
    pub step: usize,
    pub count: u32,
}

/// A player's quests.
#[derive(Component, Debug, Clone, Default, PartialEq)]
pub struct QuestLog {
    /// Quests being done, by id.
    pub active: BTreeMap<String, Active>,
    /// Finished quests.
    pub done: BTreeSet<String>,
}

/// Something that happened to a player that quests may care about.
#[derive(Debug, Clone, PartialEq)]
pub enum Deed {
    Defeated(String),
    Reached(String),
    Won(String),
}

/// What talking to someone did.
#[derive(Debug, Clone, PartialEq)]
pub enum Talked {
    /// Finished a `Talk` step of this quest (maybe the whole quest).
    Step {
        quest: String,
        dialogue: Option<String>,
        finished: bool,
    },
    /// Took on a new quest.
    Accepted { quest: String, dialogue: String },
}

/// A change to a quest from something the player did.
#[derive(Debug, Clone, PartialEq)]
pub enum Progressed {
    /// Moved on a step (or counted one more enemy).
    Step { quest: String },
    /// Finished the quest.
    Finished { quest: String },
}

impl QuestLog {
    /// Quests this person can offer right now, by id (sorted).
    pub fn offers<'a>(&self, quests: &'a HashMap<String, QuestDef>, npc: &str) -> Vec<&'a str> {
        let mut offered: Vec<&str> = quests
            .iter()
            .filter(|(id, def)| {
                def.giver == npc
                    && !self.active.contains_key(*id)
                    && !self.done.contains(*id)
                    && def.after.iter().all(|q| self.done.contains(q))
            })
            .map(|(id, _)| id.as_str())
            .collect();
        offered.sort();
        offered
    }

    /// What to show over a person's head: `?` if a quest wants you to talk
    /// to them, else `!` if they have a quest to offer.
    pub fn marker(&self, quests: &HashMap<String, QuestDef>, npc: &str) -> Option<char> {
        if self.wants_talk(quests, npc) {
            Some('?')
        } else if !self.offers(quests, npc).is_empty() {
            Some('!')
        } else {
            None
        }
    }

    /// Does any active quest want the player to talk to this person now?
    pub fn wants_talk(&self, quests: &HashMap<String, QuestDef>, npc: &str) -> bool {
        self.active.iter().any(|(id, active)| {
            current_goal(quests, id, active).is_some_and(|g| *g == Goal::Talk(npc.to_owned()))
        })
    }

    /// The player talked to `npc`: finish a step that asked for it, or
    /// else take on the first quest they offer. `None` if neither.
    pub fn talk(&mut self, quests: &HashMap<String, QuestDef>, npc: &str) -> Option<Talked> {
        let wanted = self
            .active
            .iter()
            .find(|(id, active)| {
                current_goal(quests, id, active).is_some_and(|g| *g == Goal::Talk(npc.to_owned()))
            })
            .map(|(id, _)| id.clone());
        if let Some(quest) = wanted {
            let def = quests.get(&quest)?;
            let step = self.active.get(&quest)?.step;
            let dialogue = def.steps.get(step).and_then(|s| s.dialogue.clone());
            let finished = self.next_step(quests, &quest);
            return Some(Talked::Step {
                quest,
                dialogue,
                finished,
            });
        }
        let quest = self.offers(quests, npc).first()?.to_string();
        let def = quests.get(&quest)?;
        self.active.insert(quest.clone(), Active::default());
        Some(Talked::Accepted {
            quest,
            dialogue: def.offer.clone(),
        })
    }

    /// Record something the player did; returns what changed.
    pub fn record(&mut self, quests: &HashMap<String, QuestDef>, deed: &Deed) -> Vec<Progressed> {
        let ids: Vec<String> = self.active.keys().cloned().collect();
        let mut changes = Vec::new();
        for quest in ids {
            let Some(active) = self.active.get_mut(&quest) else {
                continue;
            };
            let Some(goal) = current_goal(quests, &quest, active) else {
                continue;
            };
            let counts = match (goal, deed) {
                (Goal::Defeat { enemy, count }, Deed::Defeated(kind)) if enemy == kind => {
                    active.count += 1;
                    active.count >= *count
                }
                (Goal::Reach(zone), Deed::Reached(here)) if zone == base_zone(here) => true,
                (Goal::Win(fight), Deed::Won(won)) if fight == won => true,
                _ => continue,
            };
            if !counts {
                changes.push(Progressed::Step { quest });
                continue;
            }
            if self.next_step(quests, &quest) {
                changes.push(Progressed::Finished { quest });
            } else {
                changes.push(Progressed::Step { quest });
            }
        }
        changes
    }

    /// Move a quest on to its next step; returns true when that finishes it.
    fn next_step(&mut self, quests: &HashMap<String, QuestDef>, quest: &str) -> bool {
        let steps = quests.get(quest).map_or(0, |d| d.steps.len());
        let Some(active) = self.active.get_mut(quest) else {
            return false;
        };
        active.step += 1;
        active.count = 0;
        if active.step >= steps {
            self.active.remove(quest);
            self.done.insert(quest.to_owned());
            return true;
        }
        false
    }

    /// Drop quests (and steps) that no longer exist in the data files.
    pub fn tidy(&mut self, quests: &HashMap<String, QuestDef>) {
        self.active.retain(|id, active| {
            quests
                .get(id)
                .is_some_and(|def| active.step < def.steps.len())
        });
        self.done.retain(|id| quests.contains_key(id));
    }
}

/// The goal of the step a quest is on.
pub fn current_goal<'a>(
    quests: &'a HashMap<String, QuestDef>,
    quest: &str,
    active: &Active,
) -> Option<&'a Goal> {
    quests
        .get(quest)?
        .steps
        .get(active.step)
        .map(|step| &step.goal)
}

/// The tracker's text for a quest's current step, e.g. "Defeat thornwolves (1/3)".
pub fn step_text(quests: &HashMap<String, QuestDef>, quest: &str, active: &Active) -> String {
    let Some(step) = quests.get(quest).and_then(|d| d.steps.get(active.step)) else {
        return String::new();
    };
    match step.goal {
        Goal::Defeat { count, .. } => format!("{} ({}/{count})", step.text, active.count),
        _ => step.text.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::parse_ron;
    use std::path::Path;

    fn quests() -> HashMap<String, QuestDef> {
        let first: QuestDef = parse_ron(
            r#"(
                name: "Errand", summary: "Go and see Fen.", giver: "ilsa", offer: "ilsa_offer",
                steps: [(goal: Talk("fen"), text: "Talk to Fen", dialogue: Some("fen_hello"))],
                xp: 100,
            )"#,
            Path::new("errand.ron"),
        )
        .unwrap();
        let second: QuestDef = parse_ron(
            r#"(
                name: "Wolves", summary: "Thin the pack.", giver: "fen", offer: "fen_offer",
                after: ["errand"],
                steps: [
                    (goal: Reach("forest"), text: "Go to the forest"),
                    (goal: Defeat(enemy: "wolf", count: 2), text: "Defeat wolves"),
                    (goal: Win("boss"), text: "Defeat the boss"),
                    (goal: Talk("fen"), text: "Report back"),
                ],
            )"#,
            Path::new("wolves.ron"),
        )
        .unwrap();
        HashMap::from([("errand".into(), first), ("wolves".into(), second)])
    }

    #[test]
    fn people_offer_quests_in_order() {
        let quests = quests();
        let mut log = QuestLog::default();
        assert_eq!(log.offers(&quests, "ilsa"), vec!["errand"]);
        assert_eq!(log.marker(&quests, "ilsa"), Some('!'));
        assert_eq!(log.marker(&quests, "fen"), None);
        assert!(
            log.offers(&quests, "fen").is_empty(),
            "needs the errand first"
        );
        assert_eq!(
            log.talk(&quests, "ilsa"),
            Some(Talked::Accepted {
                quest: "errand".into(),
                dialogue: "ilsa_offer".into()
            })
        );
        assert!(log.offers(&quests, "ilsa").is_empty(), "already taken");
        assert!(log.wants_talk(&quests, "fen"));
        assert_eq!(log.marker(&quests, "fen"), Some('?'));
        assert_eq!(
            log.talk(&quests, "fen"),
            Some(Talked::Step {
                quest: "errand".into(),
                dialogue: Some("fen_hello".into()),
                finished: true
            })
        );
        assert!(log.done.contains("errand"));
        assert_eq!(log.offers(&quests, "fen"), vec!["wolves"]);
        assert_eq!(log.talk(&quests, "nobody"), None);
    }

    #[test]
    fn steps_follow_what_the_player_does() {
        let quests = quests();
        let mut log = QuestLog::default();
        log.done.insert("errand".into());
        log.talk(&quests, "fen");
        // Out of order: defeating wolves before reaching the forest doesn't count.
        assert!(
            log.record(&quests, &Deed::Defeated("wolf".into()))
                .is_empty()
        );
        assert_eq!(
            log.record(&quests, &Deed::Reached("forest#2".into())),
            vec![Progressed::Step {
                quest: "wolves".into()
            }]
        );
        log.record(&quests, &Deed::Defeated("wolf".into()));
        let active = log.active["wolves"];
        assert_eq!(step_text(&quests, "wolves", &active), "Defeat wolves (1/2)");
        log.record(&quests, &Deed::Defeated("bear".into()));
        log.record(&quests, &Deed::Defeated("wolf".into()));
        assert_eq!(log.active["wolves"].step, 2);
        log.record(&quests, &Deed::Won("boss".into()));
        assert!(log.wants_talk(&quests, "fen"));
        assert!(matches!(
            log.talk(&quests, "fen"),
            Some(Talked::Step { finished: true, .. })
        ));
        assert!(log.active.is_empty());
        assert_eq!(log.done.len(), 2);
    }

    #[test]
    fn tidy_drops_unknown_quests() {
        let quests = quests();
        let mut log = QuestLog::default();
        log.active.insert("gone".into(), Active::default());
        log.active
            .insert("errand".into(), Active { step: 9, count: 0 });
        log.done.insert("old".into());
        log.done.insert("errand".into());
        log.tidy(&quests);
        assert!(log.active.is_empty());
        assert_eq!(log.done.len(), 1);
    }

    #[test]
    fn validation_catches_mistakes() {
        let mut def = quests()["wolves"].clone();
        def.steps[1].goal = Goal::Defeat {
            enemy: "wolf".into(),
            count: 0,
        };
        def.steps[0].dialogue = Some("x".into());
        assert_eq!(def.validate().len(), 2);
    }
}
