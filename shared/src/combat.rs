//! Combat timing rules: the global cooldown, personal cooldowns, cast
//! times, the animation lock, the input queue and combos; plus health and
//! range checks. (Abilities themselves are in `abilities.rs`.)
//!
//! All times are seconds on the authority's game clock (`f64`).

use std::collections::HashMap;

use bevy::math::Vec3;
use bevy::prelude::{Component, Entity};
use serde::{Deserialize, Serialize};

use crate::abilities::AbilityDef;
use crate::data::{Problems, Validate};

/// Tunable combat settings (`assets/data/config/combat.ron`).
#[derive(Debug, Clone, Deserialize)]
pub struct CombatConfig {
    /// Global cooldown shared by all GCD abilities.
    pub gcd: f32,
    /// After an instant ability, no other ability can start for this long.
    pub animation_lock: f32,
    /// Short lock after a cast finishes.
    pub cast_lock: f32,
    /// Pressing an ability this close to it being ready queues it.
    pub queue_window: f32,
    /// A combo continues if its next step comes within this many seconds.
    pub combo_window: f32,
    /// Chance (0–1) that a hit or heal is critical.
    pub crit_chance: f32,
    /// Critical hits and heals are multiplied by this.
    pub crit_multiplier: f32,
    /// Seconds between damage-over-time / healing-over-time ticks.
    pub tick_interval: f32,
    /// You leave combat this long after the last hostile action.
    pub combat_timeout: f32,
    /// Seconds to change the flame in your lantern (switch class).
    pub flame_change_time: f32,
    /// Out of combat, you heal this fraction of your health per second.
    pub out_of_combat_regen: f32,
    /// Seconds before a defeated player gets back up (until raising arrives).
    pub revive_after: f32,
    /// Tab targeting only considers enemies within this distance.
    pub tab_target_range: f32,
    /// Ground markers are this much smaller (metres) when checking who is
    /// hit, so standing right on the edge is forgiven.
    pub marker_grace: f32,
}

impl Validate for CombatConfig {
    fn validate(&self) -> Vec<String> {
        let mut p = Problems::default();
        p.positive("gcd", self.gcd);
        p.non_negative("animation_lock", self.animation_lock);
        p.non_negative("cast_lock", self.cast_lock);
        p.non_negative("queue_window", self.queue_window);
        p.non_negative("combo_window", self.combo_window);
        if !(0.0..=1.0).contains(&self.crit_chance) {
            p.push(format!(
                "`crit_chance` must be between 0 and 1 (got {})",
                self.crit_chance
            ));
        }
        p.positive("crit_multiplier", self.crit_multiplier);
        p.positive("tick_interval", self.tick_interval);
        p.non_negative("combat_timeout", self.combat_timeout);
        p.non_negative("flame_change_time", self.flame_change_time);
        p.non_negative("out_of_combat_regen", self.out_of_combat_regen);
        p.non_negative("revive_after", self.revive_after);
        p.positive("tab_target_range", self.tab_target_range);
        p.non_negative("marker_grace", self.marker_grace);
        p.0
    }
}

/// Why an ability request was refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reject {
    NotReady,
    Casting,
    NoTarget,
    InvalidTarget,
    TargetDead,
    OutOfRange,
    UnknownAbility,
    NotOnHotbar,
    Dead,
    InCombat,
    UnknownClass,
    AlreadyThatClass,
    Busy,
    NothingHere,
    NoSuchItem,
    LevelTooLow,
    WrongClass,
    BagFull,
    NotLendable,
    SameClass,
    NotAtBoard,
    NoSuchPlace,
}

impl Reject {
    /// Short message shown to the player.
    pub fn message(self) -> &'static str {
        match self {
            Reject::NotReady => "Not ready yet.",
            Reject::Casting => "Already casting.",
            Reject::NoTarget => "No target selected.",
            Reject::InvalidTarget => "Invalid target.",
            Reject::TargetDead => "Target is already defeated.",
            Reject::OutOfRange => "Target is out of range.",
            Reject::UnknownAbility => "Unknown ability.",
            Reject::NotOnHotbar => "That ability isn't on your hotbar.",
            Reject::Dead => "You can't do that while defeated.",
            Reject::InCombat => "You can't do that during combat.",
            Reject::UnknownClass => "Unknown class.",
            Reject::AlreadyThatClass => "That flame is already burning.",
            Reject::Busy => "You're busy.",
            Reject::NothingHere => "There's nothing here to use.",
            Reject::NoSuchItem => "You don't have that item.",
            Reject::LevelTooLow => "Your level is too low to wear that.",
            Reject::WrongClass => "That weapon is for another class.",
            Reject::BagFull => "Your bag is full.",
            Reject::NotLendable => "That ability can't be borrowed.",
            Reject::SameClass => "Your secondary flame must differ from your main one.",
            Reject::NotAtBoard => "You need to be at a dungeon board.",
            Reject::NoSuchPlace => "There's no such place.",
        }
    }
}

/// Timing comparisons allow this much slack, so that tiny rounding errors
/// (for example from converting data-file numbers) never make an ability
/// that is ready by the rules count as "not ready".
pub const TIME_TOLERANCE: f64 = 1e-4;

/// A cast in progress.
#[derive(Debug, Clone, PartialEq)]
pub struct Cast {
    pub ability: String,
    pub target: Option<Entity>,
    pub started: f64,
    pub ends: f64,
    /// Whether this cast continues a combo (decided when it started).
    pub combo: bool,
}

/// A request waiting for the GCD or a cooldown to come back.
#[derive(Debug, Clone, PartialEq)]
pub struct Queued {
    pub ability: String,
    pub target: Option<Entity>,
}

/// What happened when an ability started.
#[derive(Debug, Clone, PartialEq)]
pub enum Started {
    /// An instant ability: resolve its effects now.
    Instant { combo: bool },
    /// A cast began; effects resolve when it finishes.
    Casting,
}

/// A character's action timers: GCD, cooldowns, cast, lock and queue.
#[derive(Component, Debug, Clone, Default)]
pub struct ActionState {
    pub gcd_start: f64,
    pub gcd_end: f64,
    /// No ability can start before this time.
    pub lock_until: f64,
    /// Ability id → (start, end) of its personal cooldown.
    pub cooldowns: HashMap<String, (f64, f64)>,
    pub cast: Option<Cast>,
    pub queued: Option<Queued>,
    /// The last GCD ability started and when (for combos).
    pub last_gcd: Option<(String, f64)>,
}

impl ActionState {
    /// Earliest time this ability could start, ignoring any cast in progress.
    pub fn ready_at(&self, ability: &AbilityDef) -> f64 {
        let mut ready = self.lock_until;
        if ability.on_gcd {
            ready = ready.max(self.gcd_end);
        }
        if let Some((_, end)) = self.cooldowns.get(&ability.id) {
            ready = ready.max(*end);
        }
        ready
    }

    /// Can this ability start right now (timing only — target checks are
    /// done separately because they need the world)?
    pub fn check(&self, ability: &AbilityDef, now: f64) -> Result<(), Reject> {
        if self.cast.is_some() {
            return Err(Reject::Casting);
        }
        if now + TIME_TOLERANCE < self.ready_at(ability) {
            return Err(Reject::NotReady);
        }
        Ok(())
    }

    /// Handle a request that failed only on timing: queue it if it will be
    /// ready within the queue window, otherwise reject. A newer queued
    /// request replaces an older one.
    pub fn try_queue(
        &mut self,
        ability: &AbilityDef,
        target: Option<Entity>,
        now: f64,
        config: &CombatConfig,
    ) -> Result<(), Reject> {
        let ready = match &self.cast {
            Some(cast) => self
                .ready_at(ability)
                .max(cast.ends + f64::from(config.cast_lock)),
            None => self.ready_at(ability),
        };
        if ready - now <= f64::from(config.queue_window) + TIME_TOLERANCE {
            self.queued = Some(Queued {
                ability: ability.id.clone(),
                target,
            });
            Ok(())
        } else if self.cast.is_some() {
            Err(Reject::Casting)
        } else {
            Err(Reject::NotReady)
        }
    }

    /// Start an ability. Call only after [`check`](Self::check) passed.
    pub fn begin(
        &mut self,
        ability: &AbilityDef,
        target: Option<Entity>,
        now: f64,
        config: &CombatConfig,
    ) -> Started {
        let combo = self.combo_ready(ability, now, config);
        if ability.on_gcd {
            self.gcd_start = now;
            self.gcd_end = now + f64::from(config.gcd);
            self.last_gcd = Some((ability.id.clone(), now));
        }
        if ability.cooldown > 0.0 {
            self.cooldowns
                .insert(ability.id.clone(), (now, now + f64::from(ability.cooldown)));
        }
        if ability.is_instant() {
            self.lock_until = now + f64::from(config.animation_lock);
            Started::Instant { combo }
        } else {
            let ends = now + f64::from(ability.cast_time);
            self.lock_until = ends + f64::from(config.cast_lock);
            self.cast = Some(Cast {
                ability: ability.id.clone(),
                target,
                started: now,
                ends,
                combo,
            });
            Started::Casting
        }
    }

    /// Would using `ability` now continue a combo?
    pub fn combo_ready(&self, ability: &AbilityDef, now: f64, config: &CombatConfig) -> bool {
        let (Some(combo), Some((last, at))) = (&ability.combo, &self.last_gcd) else {
            return false;
        };
        *last == combo.after && now - at <= f64::from(config.combo_window)
    }

    /// Forget all timers (used when changing class).
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// If the current cast has finished, end it and return it.
    pub fn finish_cast(&mut self, now: f64) -> Option<Cast> {
        if self
            .cast
            .as_ref()
            .is_some_and(|c| now + TIME_TOLERANCE >= c.ends)
        {
            self.cast.take()
        } else {
            None
        }
    }

    /// Cancel the current cast (for example because the caster moved).
    /// The GCD and the ability's cooldown are given back, as in FFXIV.
    pub fn interrupt(&mut self, now: f64) -> Option<Cast> {
        let cast = self.cast.take()?;
        if (self.gcd_start - cast.started).abs() < 1e-9 {
            self.gcd_end = now;
        }
        if self
            .cooldowns
            .get(&cast.ability)
            .is_some_and(|(start, _)| (*start - cast.started).abs() < 1e-9)
        {
            self.cooldowns.remove(&cast.ability);
        }
        self.lock_until = now;
        self.queued = None;
        Some(cast)
    }

    /// Remaining fraction (1 → 0) of the GCD, or `None` when it's ready.
    pub fn gcd_remaining_fraction(&self, now: f64) -> Option<f32> {
        remaining_fraction(self.gcd_start, self.gcd_end, now)
    }

    /// Remaining fraction (1 → 0) of an ability's own cooldown.
    pub fn cooldown_remaining_fraction(&self, ability: &str, now: f64) -> Option<f32> {
        let (start, end) = self.cooldowns.get(ability)?;
        remaining_fraction(*start, *end, now)
    }

    /// Seconds until an ability's own cooldown ends.
    pub fn cooldown_remaining(&self, ability: &str, now: f64) -> Option<f32> {
        let (_, end) = self.cooldowns.get(ability)?;
        (*end > now).then_some((*end - now) as f32)
    }

    /// Progress of the current cast from 0 to 1.
    pub fn cast_progress(&self, now: f64) -> Option<f32> {
        let cast = self.cast.as_ref()?;
        let length = (cast.ends - cast.started).max(1e-6);
        Some((((now - cast.started) / length) as f32).clamp(0.0, 1.0))
    }
}

fn remaining_fraction(start: f64, end: f64, now: f64) -> Option<f32> {
    if now >= end || end <= start {
        return None;
    }
    Some((((end - now) / (end - start)) as f32).clamp(0.0, 1.0))
}

/// Hit points.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct Health {
    pub current: u32,
    pub max: u32,
}

impl Health {
    pub fn full(max: u32) -> Self {
        Self { current: max, max }
    }

    /// Add up to `amount` (never above max) and return how much was added.
    pub fn heal(&mut self, amount: u32) -> u32 {
        let added = amount.min(self.max - self.current);
        self.current += added;
        added
    }

    /// Change the maximum, keeping the same fraction of health.
    pub fn set_max(&mut self, max: u32) {
        let fraction = self.fraction();
        self.max = max;
        self.current = ((max as f32 * fraction).round() as u32).min(max);
    }

    /// Remove up to `amount` and return how much was actually removed.
    pub fn damage(&mut self, amount: u32) -> u32 {
        let dealt = amount.min(self.current);
        self.current -= dealt;
        dealt
    }

    pub fn is_dead(&self) -> bool {
        self.current == 0
    }

    pub fn fraction(&self) -> f32 {
        if self.max == 0 {
            0.0
        } else {
            self.current as f32 / self.max as f32
        }
    }
}

/// Is the target within `range`? Distance is measured on the ground, from
/// the user's centre to the edge of the target's hit circle (like FFXIV's
/// target rings), so big monsters can be hit from further away.
pub fn in_range(user: Vec3, target: Vec3, target_radius: f32, range: f32) -> bool {
    horizontal_distance(user, target) - target_radius <= range
}

pub fn horizontal_distance(a: Vec3, b: Vec3) -> f32 {
    let d = b - a;
    (d.x * d.x + d.z * d.z).sqrt()
}

/// Combat settings for tests: no crits, simple numbers.
#[cfg(test)]
pub fn test_config() -> CombatConfig {
    CombatConfig {
        gcd: 1.5,
        animation_lock: 0.6,
        cast_lock: 0.1,
        queue_window: 0.5,
        combo_window: 15.0,
        crit_chance: 0.0,
        crit_multiplier: 1.5,
        tick_interval: 3.0,
        combat_timeout: 6.0,
        flame_change_time: 2.0,
        out_of_combat_regen: 0.1,
        revive_after: 5.0,
        tab_target_range: 40.0,
        marker_grace: 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abilities::Combo;
    use crate::abilities::test_support::damage_ability;

    fn config() -> CombatConfig {
        test_config()
    }

    fn ability(id: &str, on_gcd: bool, cast_time: f32, cooldown: f32) -> AbilityDef {
        damage_ability(id, on_gcd, cast_time, cooldown)
    }

    fn strike() -> AbilityDef {
        ability("strike", true, 0.0, 0.0)
    }
    fn bolt() -> AbilityDef {
        ability("bolt", true, 2.0, 0.0)
    }
    fn burst() -> AbilityDef {
        ability("burst", false, 0.0, 15.0)
    }

    #[test]
    fn instant_gcd_starts_gcd_and_lock() {
        let mut s = ActionState::default();
        assert_eq!(s.check(&strike(), 10.0), Ok(()));
        assert_eq!(
            s.begin(&strike(), None, 10.0, &config()),
            Started::Instant { combo: false }
        );
        assert_eq!(s.gcd_end, 11.5);
        assert!((s.lock_until - 10.6).abs() < 1e-6);
        assert_eq!(s.check(&strike(), 11.0), Err(Reject::NotReady));
        assert_eq!(s.check(&strike(), 11.5), Ok(()));
    }

    #[test]
    fn off_gcd_ignores_gcd_but_respects_lock() {
        let mut s = ActionState::default();
        s.begin(&strike(), None, 0.0, &config());
        assert_eq!(
            s.check(&burst(), 0.3),
            Err(Reject::NotReady),
            "animation lock"
        );
        assert_eq!(s.check(&burst(), 0.6), Ok(()), "weaved inside the GCD");
    }

    #[test]
    fn cooldowns_are_per_ability() {
        let mut s = ActionState::default();
        s.begin(&burst(), None, 0.0, &config());
        assert_eq!(s.check(&burst(), 10.0), Err(Reject::NotReady));
        assert_eq!(s.check(&strike(), 1.0), Ok(()));
        assert_eq!(s.check(&burst(), 15.0), Ok(()));
        assert_eq!(s.cooldown_remaining("burst", 5.0), Some(10.0));
        assert_eq!(s.cooldown_remaining("burst", 20.0), None);
    }

    #[test]
    fn casts_block_everything_until_finished() {
        let mut s = ActionState::default();
        assert_eq!(s.begin(&bolt(), None, 0.0, &config()), Started::Casting);
        assert_eq!(s.check(&burst(), 1.0), Err(Reject::Casting));
        assert_eq!(s.finish_cast(1.9), None);
        let cast = s.finish_cast(2.0).unwrap();
        assert_eq!(cast.ability, "bolt");
        assert_eq!(s.check(&burst(), 2.05), Err(Reject::NotReady), "cast lock");
        assert_eq!(s.check(&burst(), 2.1), Ok(()));
    }

    #[test]
    fn cast_progress_goes_from_zero_to_one() {
        let mut s = ActionState::default();
        s.begin(&bolt(), None, 0.0, &config());
        assert_eq!(s.cast_progress(0.0), Some(0.0));
        assert_eq!(s.cast_progress(1.0), Some(0.5));
        assert_eq!(s.cast_progress(5.0), Some(1.0));
    }

    #[test]
    fn interrupting_a_cast_refunds_the_gcd() {
        let mut s = ActionState::default();
        s.begin(&bolt(), None, 0.0, &config());
        let cast = s.interrupt(0.5).unwrap();
        assert_eq!(cast.ability, "bolt");
        assert_eq!(s.check(&strike(), 0.5), Ok(()));
        assert_eq!(s.interrupt(0.6), None, "nothing left to interrupt");
    }

    #[test]
    fn interrupting_refunds_cast_cooldown() {
        let mut s = ActionState::default();
        let slow = ability("slow", false, 1.0, 30.0);
        s.begin(&slow, None, 0.0, &config());
        s.interrupt(0.5);
        assert_eq!(s.check(&slow, 0.5), Ok(()));
    }

    #[test]
    fn presses_near_ready_are_queued() {
        let mut s = ActionState::default();
        s.begin(&strike(), None, 0.0, &config());
        // 0.4 s before the GCD is back: inside the 0.5 s window.
        assert_eq!(s.try_queue(&strike(), None, 1.1, &config()), Ok(()));
        assert_eq!(s.queued.as_ref().unwrap().ability, "strike");
    }

    #[test]
    fn early_presses_are_rejected() {
        let mut s = ActionState::default();
        s.begin(&strike(), None, 0.0, &config());
        assert_eq!(
            s.try_queue(&strike(), None, 0.5, &config()),
            Err(Reject::NotReady)
        );
        assert!(s.queued.is_none());
    }

    #[test]
    fn can_queue_during_the_end_of_a_cast() {
        let mut s = ActionState::default();
        s.begin(&bolt(), None, 0.0, &config());
        assert_eq!(
            s.try_queue(&strike(), None, 0.5, &config()),
            Err(Reject::Casting)
        );
        assert_eq!(s.try_queue(&strike(), None, 1.8, &config()), Ok(()));
    }

    #[test]
    fn gcd_fraction_counts_down() {
        let mut s = ActionState::default();
        assert_eq!(s.gcd_remaining_fraction(0.0), None);
        s.begin(&strike(), None, 0.0, &config());
        assert_eq!(s.gcd_remaining_fraction(0.0), Some(1.0));
        assert_eq!(s.gcd_remaining_fraction(0.75), Some(0.5));
        assert_eq!(s.gcd_remaining_fraction(1.5), None);
    }

    #[test]
    fn health_never_goes_below_zero() {
        let mut h = Health::full(100);
        assert_eq!(h.damage(30), 30);
        assert_eq!(h.damage(500), 70);
        assert!(h.is_dead());
        assert_eq!(h.fraction(), 0.0);
    }

    #[test]
    fn healing_never_exceeds_max() {
        let mut h = Health {
            current: 900,
            max: 1000,
        };
        assert_eq!(h.heal(500), 100);
        assert_eq!(h.current, 1000);
    }

    #[test]
    fn changing_max_health_keeps_the_fraction() {
        let mut h = Health {
            current: 500,
            max: 1000,
        };
        h.set_max(2000);
        assert_eq!(
            h,
            Health {
                current: 1000,
                max: 2000
            }
        );
    }

    #[test]
    fn combos_need_the_right_previous_gcd_in_time() {
        let mut s = ActionState::default();
        let mut second = ability("second", true, 0.0, 0.0);
        second.combo = Some(Combo {
            after: "strike".into(),
            amount: 300,
        });
        assert!(!s.combo_ready(&second, 0.0, &config()));
        s.begin(&strike(), None, 0.0, &config());
        assert!(s.combo_ready(&second, 2.0, &config()));
        assert!(!s.combo_ready(&second, 20.0, &config()), "too late");
        s.begin(&bolt(), None, 2.0, &config());
        assert!(
            !s.combo_ready(&second, 4.0, &config()),
            "another GCD broke the combo"
        );
    }

    #[test]
    fn range_is_measured_to_target_edge() {
        let user = Vec3::ZERO;
        let target = Vec3::new(0.0, 5.0, 4.0);
        assert!(
            in_range(user, target, 1.0, 3.0),
            "4 m away, 1 m radius, 3 m range"
        );
        assert!(!in_range(user, target, 0.5, 3.0));
    }
}
