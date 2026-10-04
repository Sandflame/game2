//! The authority: Lanternflame's rules half. It owns the truth about the
//! game world and changes it in response to [`ClientRequest`]s, reporting
//! what happened as [`ServerEvent`]s. It never draws anything.
//!
//! Requires the [`GameData`] and [`Level`] resources to be inserted before
//! the app starts, and runs its systems in `FixedUpdate`.
//!
//! [`ClientRequest`]: shared::protocol::ClientRequest
//! [`ServerEvent`]: shared::protocol::ServerEvent
//! [`GameData`]: shared::gamedata::GameData
//! [`Level`]: shared::level::Level

mod actions;
mod characters;
mod classes;
pub mod database;
mod effects;
mod encounters;
mod enemies;
pub mod instances;
pub mod progression;
mod quests;
mod requests;
pub mod travel;

use bevy::prelude::*;
use shared::formulas::Rng;
use shared::protocol::Link;

pub use characters::{CombatClock, Defeated, PlayerIndex, PlayerInput};
pub use classes::FlameChange;
pub use database::Database;
pub use encounters::{Encounter, FightState};
pub use enemies::{EnemyHome, EnemyKind, ResetWhenIdle, Returning};
pub use instances::Instances;
pub use travel::{CameFrom, Npc, Riding};

/// Ordered steps of one authority tick.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum AuthoritySystems {
    /// Read requests from players.
    Receive,
    /// Move characters.
    Move,
    /// Enemies decide, casts finish, effects land, statuses tick.
    Act,
    /// Housekeeping: class changes, regeneration, revives, resets.
    Maintain,
}

/// Random numbers for critical hits.
#[derive(Resource)]
pub struct CombatRng(pub Rng);

impl Default for CombatRng {
    fn default() -> Self {
        // Seeded from the clock so each session differs.
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0x5EED, |d| d.as_nanos() as u64);
        Self(Rng::new(seed))
    }
}

pub struct AuthorityPlugin;

impl Plugin for AuthorityPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Link>()
            .init_resource::<PlayerIndex>()
            .init_resource::<CombatRng>()
            .init_resource::<effects::PendingEffects>()
            .init_resource::<encounters::EncounterChanges>()
            .init_resource::<requests::PendingInteractions>()
            .init_resource::<requests::PendingJoins>()
            .init_resource::<progression::PendingRewards>()
            .init_resource::<progression::PendingGear>()
            .init_resource::<progression::LastAutosave>()
            .init_resource::<Instances>()
            .init_resource::<quests::PendingDeeds>()
            .configure_sets(
                FixedUpdate,
                (
                    AuthoritySystems::Receive,
                    AuthoritySystems::Move,
                    AuthoritySystems::Act,
                    AuthoritySystems::Maintain,
                )
                    .chain(),
            )
            .add_systems(Startup, instances::fill_zones)
            .add_systems(
                FixedUpdate,
                (
                    (
                        requests::receive_requests,
                        requests::finish_joins,
                        requests::handle_interactions,
                        progression::handle_gear,
                    )
                        .chain()
                        .in_set(AuthoritySystems::Receive),
                    (
                        travel::advance_rides,
                        characters::move_characters,
                        enemies::move_enemies,
                    )
                        .chain()
                        .in_set(AuthoritySystems::Move),
                    (
                        encounters::run_encounters,
                        encounters::apply_encounter_changes,
                        enemies::notice_players,
                        enemies::enemy_brains,
                        enemies::face_targets,
                        actions::process_actions,
                        effects::spawn_telegraphs,
                        effects::follow_telegraphs,
                        effects::resolve_effects,
                        effects::tick_statuses,
                        characters::handle_defeats,
                    )
                        .chain()
                        .in_set(AuthoritySystems::Act),
                    (
                        classes::finish_flame_changes,
                        quests::note_arrivals,
                        progression::kill_rewards,
                        quests::record_deeds,
                        progression::grant_rewards,
                        progression::refresh_stats,
                        progression::apply_synergy,
                        characters::regenerate,
                        characters::revive,
                        characters::recover_wipes,
                        enemies::reset_idle_enemies,
                        characters::forget_absent,
                        instances::close_empty_instances,
                        progression::save_players,
                    )
                        .chain()
                        .in_set(AuthoritySystems::Maintain),
                ),
            )
            .add_systems(Last, progression::save_on_exit);
    }
}
