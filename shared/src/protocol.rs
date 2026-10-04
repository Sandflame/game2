//! Messages between the client (screen half) and the authority (rules
//! half). Today they travel through an in-process [`Link`]; in Milestone 11
//! the same messages go over the network.

use bevy::prelude::*;

use crate::combat::Reject;
use crate::components::PlayerId;
use crate::movement::MoveInput;

/// Something a player asks the authority to do.
#[derive(Debug, Clone, PartialEq)]
pub enum ClientRequest {
    /// Enter the world with this character name.
    Join { name: String },
    /// Latest movement keys. Sent every frame.
    Move(MoveInput),
    /// Use the ability in a hotbar slot (0-based) on a target.
    UseAbility { slot: usize, target: Option<Entity> },
    /// Change the flame in your lantern to switch class.
    ChangeClass { class: String },
}

/// Something the authority tells clients about.
#[derive(Debug, Clone, PartialEq)]
pub enum ServerEvent {
    /// Your character entered the world.
    Joined { player: PlayerId, entity: Entity },
    /// An ability started (instant or the start of a cast).
    AbilityUsed {
        user: Entity,
        ability: String,
        target: Option<Entity>,
    },
    /// A cast or flame change was cancelled.
    CastInterrupted { user: Entity, ability: String },
    /// Damage landed. `cause` is an ability or status id.
    Damage {
        source: Entity,
        target: Entity,
        amount: u32,
        /// How much of the hit a shield soaked up.
        absorbed: u32,
        crit: bool,
        cause: String,
        /// From a damage-over-time tick rather than a direct hit.
        tick: bool,
    },
    /// Healing landed (`amount` is what was actually restored).
    Heal {
        source: Entity,
        target: Entity,
        amount: u32,
        crit: bool,
        cause: String,
        tick: bool,
    },
    /// A request from this player was refused.
    Rejected { player: PlayerId, reason: Reject },
    /// A request was accepted but will run when the GCD/cooldown is back.
    Queued { player: PlayerId, ability: String },
    /// Someone started changing their lantern flame.
    FlameChangeStarted { user: Entity, class: String },
    /// Someone's class changed.
    ClassChanged { user: Entity, class: String },
    /// A character was defeated.
    Defeated { entity: Entity },
    /// A defeated character got back up.
    Revived { entity: Entity },
}

/// The in-process connection between the client and the authority.
/// Each side only ever pushes to one list and drains the other.
#[derive(Resource, Default, Debug)]
pub struct Link {
    pub to_authority: Vec<(PlayerId, ClientRequest)>,
    pub to_client: Vec<ServerEvent>,
}
