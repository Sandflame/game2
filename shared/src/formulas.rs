//! Damage and healing maths, critical hits, and a small random number
//! generator (so results are reproducible in tests).

use crate::combat::CombatConfig;
use crate::statuses::Modifiers;

/// Power is a percentage: 100 means abilities do exactly the amounts
/// written in the data files, 110 means 10% more.
pub const BASE_POWER: f32 = 100.0;

/// An ability's listed amount adjusted for the user's power, before buffs and crits.
pub fn base_amount(amount: u32, power: f32) -> f32 {
    amount as f32 * (power / BASE_POWER)
}

/// Damage a hit deals before the target's defences.
pub fn outgoing_damage(
    amount: u32,
    power: f32,
    dealer: Modifiers,
    crit: bool,
    config: &CombatConfig,
) -> u32 {
    let crit_factor = if crit { config.crit_multiplier } else { 1.0 };
    (base_amount(amount, power) * dealer.damage_dealt * crit_factor).round() as u32
}

/// Damage after the target's damage-taken modifiers and their gear's
/// guard (percent less damage taken).
pub fn incoming_damage(amount: u32, target: Modifiers, guard: f32) -> u32 {
    let guard = (1.0 - guard / 100.0).clamp(0.0, 1.0);
    (amount as f32 * target.damage_taken * guard).round() as u32
}

/// Healing (or shield) amount.
pub fn healing(
    amount: u32,
    power: f32,
    healer: Modifiers,
    target: Modifiers,
    crit: bool,
    config: &CombatConfig,
) -> u32 {
    let crit_factor = if crit { config.crit_multiplier } else { 1.0 };
    (base_amount(amount, power) * healer.healing_done * target.healing_received * crit_factor)
        .round() as u32
}

/// A tiny, fast random number generator (SplitMix64). Not for security,
/// just for crits — and it gives the same sequence for the same seed.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0.0..1.0`.
    pub fn unit(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    /// True with the given probability.
    pub fn chance(&mut self, probability: f32) -> bool {
        self.unit() < probability
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::combat::test_config;

    #[test]
    fn power_is_a_percentage() {
        let c = test_config();
        assert_eq!(
            outgoing_damage(200, 100.0, Modifiers::default(), false, &c),
            200
        );
        assert_eq!(
            outgoing_damage(200, 150.0, Modifiers::default(), false, &c),
            300
        );
    }

    #[test]
    fn buffs_and_crits_multiply() {
        let c = test_config();
        let buffed = Modifiers {
            damage_dealt: 1.2,
            ..Default::default()
        };
        assert_eq!(outgoing_damage(100, 100.0, buffed, false, &c), 120);
        assert_eq!(outgoing_damage(100, 100.0, buffed, true, &c), 180);
    }

    #[test]
    fn mitigation_reduces_incoming_damage() {
        let guarded = Modifiers {
            damage_taken: 0.7,
            ..Default::default()
        };
        assert_eq!(incoming_damage(1000, guarded, 0.0), 700);
        // 20% guard from gear on top.
        assert_eq!(incoming_damage(1000, guarded, 20.0), 560);
    }

    #[test]
    fn healing_uses_both_sides_modifiers() {
        let c = test_config();
        let healer = Modifiers {
            healing_done: 1.1,
            ..Default::default()
        };
        let target = Modifiers {
            healing_received: 1.2,
            ..Default::default()
        };
        assert_eq!(healing(500, 100.0, healer, target, false, &c), 660);
    }

    #[test]
    fn rng_is_reproducible_and_roughly_fair() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        assert_eq!(a.next_u64(), b.next_u64());
        let mut r = Rng::new(42);
        let hits = (0..10_000).filter(|_| r.chance(0.25)).count();
        assert!((2_200..2_800).contains(&hits), "{hits}");
        assert!((0..1000).all(|_| (0.0..1.0).contains(&r.unit())));
    }
}
