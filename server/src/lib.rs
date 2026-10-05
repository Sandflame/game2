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

pub mod accounts;
mod actions;
mod characters;
mod classes;
pub mod database;
mod effects;
mod encounters;
mod enemies;
pub mod instances;
pub mod net;
pub mod progression;
mod quests;
mod requests;
pub mod travel;

use bevy::prelude::*;
use shared::formulas::Rng;
use shared::protocol::Link;

pub use characters::{CombatClock, Defeated, PlayerIndex, PlayerInput, Settled};
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

/// Whether the rules run in this program. The `server` program and a
/// game played on this computer run them; a game connected to a server
/// switches them off (the server runs them instead).
#[derive(Resource, Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthorityActive(pub bool);

impl Default for AuthorityActive {
    fn default() -> Self {
        Self(true)
    }
}

fn active(state: Res<AuthorityActive>) -> bool {
    state.0
}

/// The zones' enemies and people are put in place once, the first time the
/// rules run.
#[derive(Resource, Default)]
struct ZonesFilled(bool);

fn zones_unfilled(filled: Res<ZonesFilled>) -> bool {
    !filled.0
}

fn mark_filled(mut filled: ResMut<ZonesFilled>) {
    filled.0 = true;
}

/// Keep the shared clock entity up to date (clients use it to time
/// cooldowns, casts and markers).
fn tick_world_clock(
    mut commands: Commands,
    time: Res<Time>,
    mut clocks: Query<&mut shared::components::WorldClock>,
) {
    let now = time.elapsed_secs_f64();
    match clocks.single_mut() {
        Ok(mut clock) => clock.0 = now,
        Err(_) => {
            commands.spawn(shared::components::WorldClock(now));
        }
    }
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
            .init_resource::<accounts::Sessions>()
            .init_resource::<accounts::AccountRequests>()
            .init_resource::<progression::PendingRewards>()
            .init_resource::<progression::PendingGear>()
            .init_resource::<progression::LastAutosave>()
            .init_resource::<Instances>()
            .init_resource::<classes::PendingSpecs>()
            .init_resource::<quests::PendingDeeds>()
            .init_resource::<requests::PendingLeaves>()
            .init_resource::<AuthorityActive>()
            .init_resource::<ZonesFilled>()
            .configure_sets(
                FixedUpdate,
                (
                    AuthoritySystems::Receive,
                    AuthoritySystems::Move,
                    AuthoritySystems::Act,
                    AuthoritySystems::Maintain,
                )
                    .chain()
                    .run_if(active),
            )
            .add_systems(
                FixedUpdate,
                (instances::fill_zones, mark_filled)
                    .chain()
                    .run_if(active.and_then(zones_unfilled))
                    .before(AuthoritySystems::Receive),
            )
            .add_systems(
                FixedUpdate,
                (
                    (
                        requests::receive_requests,
                        accounts::handle_account_requests,
                        accounts::finish_account_jobs,
                        requests::finish_joins,
                        requests::handle_interactions,
                        progression::handle_gear,
                        classes::handle_spec_changes,
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
                        effects::apply_lunges,
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
                        progression::handle_leaves,
                        instances::close_empty_instances,
                        progression::save_players,
                        characters::count_forced_moves,
                        tick_world_clock,
                    )
                        .chain()
                        .in_set(AuthoritySystems::Maintain),
                ),
            )
            .add_systems(Last, progression::save_on_exit);
    }
}
