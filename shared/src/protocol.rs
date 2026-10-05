//! Messages between the client (screen half) and the authority (rules
//! half). Today they travel through an in-process [`Link`]; in Milestone 11
//! the same messages go over the network.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::combat::Reject;
use crate::components::PlayerId;
use crate::items::Slot;
use crate::movement::MoveInput;

/// One of an account's characters, for the character list.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CharacterSummary {
    pub name: String,
    /// Current class id.
    pub class: String,
    /// The current class's level.
    pub level: u32,
    /// The zone it is in.
    pub zone: String,
    pub appearance: Option<crate::appearance::Appearance>,
}

/// Something a player asks the authority to do.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ClientRequest {
    /// Make an account (and log in to it).
    Register { account: String, password: String },
    /// Log in to an account.
    Login { account: String, password: String },
    /// Log out (back to the login screen).
    Logout,
    /// Make a new character on the logged-in account.
    CreateCharacter {
        name: String,
        class: String,
        appearance: crate::appearance::Appearance,
    },
    /// Delete one of the logged-in account's characters.
    DeleteCharacter { name: String },
    /// Enter the world with this character name (one of the logged-in
    /// account's; without a save file, any name).
    Join { name: String },
    /// Latest movement keys. Sent every frame when the authority moves
    /// the character itself (playing on this computer).
    Move(MoveInput),
    /// Over a network the client moves its own character at once and
    /// reports where it got to; the authority checks it could have.
    /// `epoch` is the last [`MoveEpoch`](crate::components::MoveEpoch) the
    /// client saw: reports from before the authority last moved the
    /// character itself (a portal, a lunge…) are ignored.
    Moved {
        input: MoveInput,
        state: crate::movement::MoveState,
        epoch: u32,
    },
    /// Leave the world (the connection closed): the character is saved
    /// and removed.
    Leave,
    /// Use the ability in a hotbar slot (0-based) on a target.
    UseAbility { slot: usize, target: Option<Entity> },
    /// Change the flame in your lantern to switch class.
    ChangeClass { class: String },
    /// Use whatever is here (for now: a portal you are standing in).
    Interact,
    /// Put on an item from your bag (by its id in the bag).
    Equip { item: u64 },
    /// Take off what is worn in a slot (for weapons: your current class's).
    Unequip { slot: Slot },
    /// Throw an item away.
    Discard { item: u64 },
    /// Choose the secondary class (and its two borrowed abilities) for your
    /// current class. `None` removes the secondary class.
    SetSecondary {
        choice: Option<crate::classes::SecondaryChoice>,
    },
    /// At a dungeon board: go to this dungeon or trial (zone file name).
    EnterFromBoard { zone: String },
    /// Switch the current class to this specialization (out of combat).
    ChangeSpec { spec: String },
}

/// Something the authority tells clients about.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ServerEvent {
    /// Sent when a client connects over the network: its player id.
    Welcome { player: PlayerId },
    /// Logged in to this account.
    LoggedIn { player: PlayerId, account: String },
    /// An account or character request was refused (shown as it is).
    AccountError { player: PlayerId, message: String },
    /// The logged-in account's characters.
    Characters {
        player: PlayerId,
        characters: Vec<CharacterSummary>,
    },
    /// Your character entered the world.
    Joined { player: PlayerId, entity: Entity },
    /// An ability started (instant or the start of a cast).
    AbilityUsed {
        user: Entity,
        ability: String,
        target: Option<Entity>,
    },
    /// An ability's effects landed (the end of a cast, or an instant).
    /// The client uses this to time visual effects.
    AbilityLanded {
        user: Entity,
        ability: String,
        target: Entity,
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
    /// Someone switched specialization.
    SpecChanged { user: Entity, spec: String },
    /// A character was defeated.
    Defeated { entity: Entity },
    /// A defeated character got back up.
    Revived { entity: Entity },
    /// A character moved to another zone.
    ZoneChanged { entity: Entity, zone: String },
    /// A line of text for everyone in a zone (boss speech, warnings).
    Announce { zone: String, text: String },
    /// A boss fight began.
    EncounterStarted { zone: String, name: String },
    /// A boss was defeated, `seconds` after the fight began.
    EncounterWon {
        zone: String,
        /// Encounter id (file name in `assets/data/encounters/`).
        encounter: String,
        name: String,
        seconds: f32,
    },
    /// Everyone fell; the fight resets.
    EncounterWiped { zone: String, name: String },
    /// A character's current class gained experience.
    XpGained {
        entity: Entity,
        class: String,
        amount: u32,
    },
    /// A character's class reached a new level.
    LevelUp {
        entity: Entity,
        class: String,
        level: u32,
    },
    /// A character got an item (e.g. boss loot).
    ItemReceived { entity: Entity, item: String },
    /// Someone (or something, like a closed gate) says a line to a player.
    Speech {
        player: PlayerId,
        speaker: String,
        text: String,
    },
    /// The player is at a dungeon board: show the list.
    OpenBoard { player: PlayerId },
    /// Play a conversation (`assets/data/dialogue/`) for this player.
    Dialogue { player: PlayerId, dialogue: String },
    /// The player took on a quest.
    QuestAccepted { player: PlayerId, quest: String },
    /// A quest moved on a step (or counted another enemy).
    QuestProgressed { player: PlayerId, quest: String },
    /// The player finished a quest.
    QuestCompleted { player: PlayerId, quest: String },
}

impl bevy::ecs::entity::MapEntities for ClientRequest {
    fn map_entities<M: bevy::ecs::entity::EntityMapper>(&mut self, mapper: &mut M) {
        if let ClientRequest::UseAbility {
            target: Some(target),
            ..
        } = self
        {
            *target = mapper.get_mapped(*target);
        }
    }
}

impl bevy::ecs::entity::MapEntities for ServerEvent {
    fn map_entities<M: bevy::ecs::entity::EntityMapper>(&mut self, mapper: &mut M) {
        for entity in self.entities_mut() {
            *entity = mapper.get_mapped(*entity);
        }
    }
}

impl ServerEvent {
    /// Every entity this event mentions.
    pub fn entities_mut(&mut self) -> Vec<&mut Entity> {
        use ServerEvent as E;
        match self {
            E::Joined { entity, .. }
            | E::CastInterrupted { user: entity, .. }
            | E::FlameChangeStarted { user: entity, .. }
            | E::ClassChanged { user: entity, .. }
            | E::SpecChanged { user: entity, .. }
            | E::Defeated { entity }
            | E::Revived { entity }
            | E::ZoneChanged { entity, .. }
            | E::XpGained { entity, .. }
            | E::LevelUp { entity, .. }
            | E::ItemReceived { entity, .. } => vec![entity],
            E::AbilityUsed { user, target, .. } => {
                let mut list = vec![user];
                list.extend(target.as_mut());
                list
            }
            E::AbilityLanded { user, target, .. } => vec![user, target],
            E::Damage { source, target, .. } | E::Heal { source, target, .. } => {
                vec![source, target]
            }
            E::Welcome { .. }
            | E::LoggedIn { .. }
            | E::AccountError { .. }
            | E::Characters { .. }
            | E::Rejected { .. }
            | E::Queued { .. }
            | E::Announce { .. }
            | E::EncounterStarted { .. }
            | E::EncounterWon { .. }
            | E::EncounterWiped { .. }
            | E::Speech { .. }
            | E::OpenBoard { .. }
            | E::Dialogue { .. }
            | E::QuestAccepted { .. }
            | E::QuestProgressed { .. }
            | E::QuestCompleted { .. } => Vec::new(),
        }
    }

    /// The one player this event is for, if it is private.
    pub fn for_player(&self) -> Option<PlayerId> {
        use ServerEvent as E;
        match self {
            E::Welcome { player }
            | E::LoggedIn { player, .. }
            | E::AccountError { player, .. }
            | E::Characters { player, .. }
            | E::Joined { player, .. }
            | E::Rejected { player, .. }
            | E::Queued { player, .. }
            | E::Speech { player, .. }
            | E::OpenBoard { player }
            | E::Dialogue { player, .. }
            | E::QuestAccepted { player, .. }
            | E::QuestProgressed { player, .. }
            | E::QuestCompleted { player, .. } => Some(*player),
            _ => None,
        }
    }

    /// The zone this event happens in, if it names one.
    pub fn zone(&self) -> Option<&str> {
        use ServerEvent as E;
        match self {
            E::Announce { zone, .. }
            | E::EncounterStarted { zone, .. }
            | E::EncounterWon { zone, .. }
            | E::EncounterWiped { zone, .. } => Some(zone),
            _ => None,
        }
    }
}

/// The in-process connection between the client and the authority.
/// Each side only ever pushes to one list and drains the other.
#[derive(Resource, Default, Debug)]
pub struct Link {
    pub to_authority: Vec<(PlayerId, ClientRequest)>,
    pub to_client: Vec<ServerEvent>,
}
