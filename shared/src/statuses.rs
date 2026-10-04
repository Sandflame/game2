//! Buffs and debuffs ("statuses"): timed effects such as damage over time,
//! healing over time, damage up/down, and shields that absorb damage.

use bevy::prelude::{Component, Entity};
use serde::{Deserialize, Serialize};

use crate::data::{Problems, Validate};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum StatusKind {
    Buff,
    Debuff,
}

/// Something a status does every tick (see `tick_interval` in combat.ron).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum Tick {
    Damage { amount: u32 },
    Heal { amount: u32 },
}

/// Multipliers a status applies while active (1.0 = no change).
#[derive(Debug, Clone, Copy, PartialEq, Deserialize, Serialize)]
#[serde(default)]
pub struct Modifiers {
    pub damage_dealt: f32,
    pub damage_taken: f32,
    pub healing_done: f32,
    pub healing_received: f32,
    /// Movement speed (0.7 = slowed by 30%).
    pub move_speed: f32,
}

impl Default for Modifiers {
    fn default() -> Self {
        Self {
            damage_dealt: 1.0,
            damage_taken: 1.0,
            healing_done: 1.0,
            healing_received: 1.0,
            move_speed: 1.0,
        }
    }
}

impl Modifiers {
    /// Two sets of modifiers applied together (they multiply).
    pub fn combine(self, other: Modifiers) -> Modifiers {
        Modifiers {
            damage_dealt: self.damage_dealt * other.damage_dealt,
            damage_taken: self.damage_taken * other.damage_taken,
            healing_done: self.healing_done * other.healing_done,
            healing_received: self.healing_received * other.healing_received,
            move_speed: self.move_speed * other.move_speed,
        }
    }

    /// These modifiers applied `times` times over (for stacking statuses).
    pub fn repeated(self, times: u32) -> Modifiers {
        let n = times as i32;
        Modifiers {
            damage_dealt: self.damage_dealt.powi(n),
            damage_taken: self.damage_taken.powi(n),
            healing_done: self.healing_done.powi(n),
            healing_received: self.healing_received.powi(n),
            move_speed: self.move_speed.powi(n),
        }
    }
}

/// One status, as written in `assets/data/statuses/*.ron`.
#[derive(Debug, Clone, Deserialize)]
pub struct StatusDef {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub kind: StatusKind,
    /// Seconds.
    pub duration: f32,
    #[serde(default)]
    pub tick: Option<Tick>,
    #[serde(default)]
    pub modifiers: Modifiers,
    /// Applying it again adds a stack, up to this many (1 = it just
    /// refreshes). Modifiers apply once per stack.
    #[serde(default = "one_stack")]
    pub max_stacks: u32,
    /// Visual effect name (client only).
    #[serde(default)]
    pub vfx: String,
}

fn one_stack() -> u32 {
    1
}

impl StatusDef {
    pub fn problems(&self) -> Vec<String> {
        let mut p = Problems::default();
        if self.id.trim().is_empty() {
            p.push("a status has an empty `id`");
        }
        p.positive("duration", self.duration);
        let m = self.modifiers;
        for (name, value) in [
            ("damage_dealt", m.damage_dealt),
            ("damage_taken", m.damage_taken),
            ("healing_done", m.healing_done),
            ("healing_received", m.healing_received),
            ("move_speed", m.move_speed),
        ] {
            p.non_negative(&format!("modifiers.{name}"), value);
        }
        if self.max_stacks == 0 {
            p.push("`max_stacks` must be 1 or more");
        }
        p.0.into_iter()
            .map(|m| format!("status `{}`: {m}", self.id))
            .collect()
    }
}

/// A list of statuses in one file.
#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub struct StatusFile(pub Vec<StatusDef>);

impl Validate for StatusFile {
    fn validate(&self) -> Vec<String> {
        self.0.iter().flat_map(StatusDef::problems).collect()
    }
}

/// What an active status does each tick, worked out when it was applied
/// (so later buffs on the caster don't change a running damage-over-time).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TickAmount {
    Damage(u32),
    Heal(u32),
}

/// A status currently on a character.
#[derive(Debug, Clone, PartialEq)]
pub struct ActiveStatus {
    pub id: String,
    pub source: Entity,
    pub applied: f64,
    pub expires: f64,
    pub next_tick: f64,
    pub tick: Option<TickAmount>,
    /// Damage this status can still absorb (shields only).
    pub absorb: u32,
    pub is_shield: bool,
    /// How many times it is stacked (1 for most statuses).
    pub stacks: u32,
}

impl ActiveStatus {
    pub fn remaining(&self, now: f64) -> f32 {
        (self.expires - now).max(0.0) as f32
    }
}

/// A tick that came due.
#[derive(Debug, Clone, PartialEq)]
pub struct DueTick {
    pub status: String,
    pub source: Entity,
    pub amount: TickAmount,
}

/// All statuses on one character.
#[derive(Component, Debug, Clone, Default)]
pub struct Statuses(pub Vec<ActiveStatus>);

impl Statuses {
    /// Add a status, or refresh it if the same source already applied it.
    pub fn apply(&mut self, status: ActiveStatus) {
        self.apply_stacking(status, 1);
    }

    /// Add a status, or refresh it and add a stack (up to `max_stacks`) if
    /// the same source already applied it.
    pub fn apply_stacking(&mut self, mut status: ActiveStatus, max_stacks: u32) {
        match self
            .0
            .iter_mut()
            .find(|s| s.id == status.id && s.source == status.source)
        {
            Some(existing) => {
                status.stacks = (existing.stacks + 1).min(max_stacks.max(1));
                *existing = status;
            }
            None => self.0.push(status),
        }
    }

    /// How many stacks of a status this source has here (0 if none).
    pub fn stacks_from(&self, id: &str, source: Entity) -> u32 {
        self.0
            .iter()
            .find(|s| s.id == id && s.source == source)
            .map_or(0, |s| s.stacks)
    }

    /// Remove a status this source applied.
    pub fn remove_from(&mut self, id: &str, source: Entity) {
        self.0.retain(|s| !(s.id == id && s.source == source));
    }

    /// Combined modifiers of every active status.
    pub fn modifiers<'a>(&self, def: impl Fn(&str) -> Option<&'a StatusDef>) -> Modifiers {
        self.0
            .iter()
            .filter_map(|s| def(&s.id).map(|d| d.modifiers.repeated(s.stacks.max(1))))
            .fold(Modifiers::default(), Modifiers::combine)
    }

    /// Let shields soak up damage, soonest-expiring first. Returns
    /// (absorbed, damage left over). Used-up shields are removed.
    pub fn absorb(&mut self, amount: u32) -> (u32, u32) {
        let mut left = amount;
        let mut shields: Vec<&mut ActiveStatus> =
            self.0.iter_mut().filter(|s| s.is_shield).collect();
        shields.sort_by(|a, b| a.expires.total_cmp(&b.expires));
        for shield in shields {
            let soaked = shield.absorb.min(left);
            shield.absorb -= soaked;
            left -= soaked;
            if left == 0 {
                break;
            }
        }
        self.0.retain(|s| !s.is_shield || s.absorb > 0);
        (amount - left, left)
    }

    /// Total shield left.
    pub fn total_absorb(&self) -> u32 {
        self.0
            .iter()
            .filter(|s| s.is_shield)
            .map(|s| s.absorb)
            .sum()
    }

    /// Remove statuses whose time is up; returns their ids.
    pub fn expire(&mut self, now: f64) -> Vec<String> {
        let mut gone = Vec::new();
        self.0.retain(|s| {
            let keep = s.expires > now;
            if !keep {
                gone.push(s.id.clone());
            }
            keep
        });
        gone
    }

    /// Ticks that are due, moving each status's next tick along.
    pub fn due_ticks(&mut self, now: f64, interval: f64) -> Vec<DueTick> {
        let mut due = Vec::new();
        for status in &mut self.0 {
            let Some(amount) = status.tick else {
                continue;
            };
            while status.next_tick <= now && status.next_tick <= status.expires {
                due.push(DueTick {
                    status: status.id.clone(),
                    source: status.source,
                    amount,
                });
                status.next_tick += interval;
            }
        }
        due
    }

    pub fn has(&self, id: &str) -> bool {
        self.0.iter().any(|s| s.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(n: u32) -> Entity {
        Entity::from_raw_u32(n).unwrap()
    }

    fn active(id: &str, source: u32, expires: f64) -> ActiveStatus {
        ActiveStatus {
            id: id.into(),
            source: entity(source),
            applied: 0.0,
            expires,
            next_tick: 3.0,
            tick: None,
            absorb: 0,
            is_shield: false,
            stacks: 1,
        }
    }

    fn shield(id: &str, absorb: u32, expires: f64) -> ActiveStatus {
        ActiveStatus {
            absorb,
            is_shield: true,
            ..active(id, 1, expires)
        }
    }

    fn def(id: &str, modifiers: Modifiers) -> StatusDef {
        StatusDef {
            id: id.into(),
            name: id.into(),
            description: String::new(),
            kind: StatusKind::Buff,
            duration: 10.0,
            tick: None,
            modifiers,
            max_stacks: 1,
            vfx: String::new(),
        }
    }

    #[test]
    fn reapplying_refreshes_instead_of_stacking() {
        let mut s = Statuses::default();
        s.apply(active("burn", 1, 10.0));
        s.apply(active("burn", 1, 20.0));
        assert_eq!(s.0.len(), 1);
        assert_eq!(s.0[0].expires, 20.0);
        // A different caster's burn is separate.
        s.apply(active("burn", 2, 20.0));
        assert_eq!(s.0.len(), 2);
    }

    #[test]
    fn stacks_build_up_and_multiply() {
        let slow = def(
            "chill",
            Modifiers {
                move_speed: 0.9,
                ..Default::default()
            },
        );
        let mut s = Statuses::default();
        for _ in 0..5 {
            s.apply_stacking(active("chill", 1, 10.0), 3);
        }
        assert_eq!(s.stacks_from("chill", entity(1)), 3, "capped");
        assert_eq!(s.stacks_from("chill", entity(2)), 0);
        let m = s.modifiers(|id| (id == "chill").then_some(&slow));
        assert!((m.move_speed - 0.729).abs() < 1e-4, "{}", m.move_speed);
        s.remove_from("chill", entity(1));
        assert!(s.0.is_empty());
    }

    #[test]
    fn modifiers_multiply() {
        let up = def(
            "up",
            Modifiers {
                damage_dealt: 1.2,
                ..Default::default()
            },
        );
        let guard = def(
            "guard",
            Modifiers {
                damage_taken: 0.5,
                ..Default::default()
            },
        );
        let mut s = Statuses::default();
        s.apply(active("up", 1, 10.0));
        s.apply(active("guard", 1, 10.0));
        let m = s.modifiers(|id| [&up, &guard].into_iter().find(|d| d.id == id));
        assert!((m.damage_dealt - 1.2).abs() < 1e-6);
        assert!((m.damage_taken - 0.5).abs() < 1e-6);
        assert_eq!(m.healing_done, 1.0);
    }

    #[test]
    fn shields_absorb_and_break() {
        let mut s = Statuses::default();
        s.apply(shield("late", 500, 20.0));
        s.apply(shield("early", 300, 10.0));
        assert_eq!(s.total_absorb(), 800);
        assert_eq!(s.absorb(400), (400, 0));
        assert!(!s.has("early"), "used up first because it expires sooner");
        assert_eq!(s.total_absorb(), 400);
        assert_eq!(s.absorb(1000), (400, 600));
        assert!(s.0.is_empty());
    }

    #[test]
    fn statuses_expire() {
        let mut s = Statuses::default();
        s.apply(active("a", 1, 5.0));
        s.apply(active("b", 1, 15.0));
        assert_eq!(s.expire(10.0), vec!["a".to_owned()]);
        assert!(s.has("b"));
    }

    #[test]
    fn ticks_come_due_on_schedule() {
        let mut s = Statuses::default();
        s.apply(ActiveStatus {
            tick: Some(TickAmount::Damage(50)),
            ..active("burn", 1, 9.5)
        });
        assert!(s.due_ticks(2.9, 3.0).is_empty());
        assert_eq!(s.due_ticks(3.0, 3.0).len(), 1);
        // A long frame catches up on missed ticks, but none after expiry.
        assert_eq!(s.due_ticks(20.0, 3.0).len(), 2, "ticks at 6 and 9");
    }
}
