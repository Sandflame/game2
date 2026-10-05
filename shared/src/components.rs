//! Logic components that describe game entities. The authority (rules
//! half) writes them; the client only reads them. Later they will be sent
//! over the network unchanged.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::movement::MoveState;

/// Identifies a player (one per connected person).
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PlayerId(pub u64);

/// Display name shown on nameplates and frames.
#[derive(Component, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CharacterName(pub String);

/// Where a character is and how it is moving.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct Motion(pub MoveState);

/// Which zone a character is in (file name in `assets/data/zones/`).
/// Characters only see and affect others in the same zone.
#[derive(Component, Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Zone(pub String);

/// A doorway that appeared during play (e.g. the way out after a boss
/// falls). Leads back to wherever each player came in from.
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExitPortal {
    pub position: Vec3,
    pub radius: f32,
    pub label: String,
}

/// Which person this is (their `id` in zone data); quests name people by it.
#[derive(Component, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NpcId(pub String);

/// Which side a character is on.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Faction {
    Player,
    Enemy,
    /// Townsfolk and other people you talk to: nobody fights them.
    Neutral,
}

impl Faction {
    /// Can a character of this faction harm one of `other`?
    /// (A rule function rather than a hard-coded check, so duels and PvP
    /// can change it later.)
    pub fn can_harm(self, other: Faction) -> bool {
        self != other && self != Faction::Neutral && other != Faction::Neutral
    }
}

/// Radius of the character's target ring, in metres. Ability ranges are
/// measured to this edge.
#[derive(Component, Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct HitRadius(pub f32);

/// Which placeholder look the client should build (e.g. "player",
/// "training_dummy"). Logic never depends on it.
#[derive(Component, Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct VisualKey(pub String);

/// The abilities on a player's hotbar, slot by slot (None = empty slot).
#[derive(Component, Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Hotbar(pub Vec<Option<String>>);

/// The number of hotbar slots (keys 1–0).
pub const HOTBAR_SLOTS: usize = 12;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn players_and_enemies_can_harm_each_other_only() {
        assert!(Faction::Player.can_harm(Faction::Enemy));
        assert!(Faction::Enemy.can_harm(Faction::Player));
        assert!(!Faction::Player.can_harm(Faction::Player));
    }
}

/// Counts the times the authority moved a character itself (portals,
/// lunges, rides, revives…). A client that moves its own character over a
/// network takes the authority's position when this changes.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MoveEpoch(pub u32);

/// The authority's clock (seconds of game time), so clients can tell how
/// long is left on cooldowns, casts and markers that use its times.
#[derive(Component, Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
pub struct WorldClock(pub f64);
