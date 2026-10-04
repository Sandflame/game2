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
mod requests;

use bevy::prelude::*;
use shared::protocol::Link;

pub use characters::{EnemyKind, PlayerIndex, PlayerInput};

/// Ordered steps of one authority tick.
#[derive(SystemSet, Debug, Clone, PartialEq, Eq, Hash)]
pub enum AuthoritySystems {
    /// Read requests from players.
    Receive,
    /// Move characters.
    Move,
    /// Finish casts, run queued actions, apply damage.
    Act,
    /// Housekeeping such as training dummies resetting.
    Maintain,
}

pub struct AuthorityPlugin;

impl Plugin for AuthorityPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Link>()
            .init_resource::<PlayerIndex>()
            .init_resource::<actions::PendingHits>()
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
            .add_systems(Startup, characters::spawn_enemies)
            .add_systems(
                FixedUpdate,
                (
                    requests::receive_requests.in_set(AuthoritySystems::Receive),
                    characters::move_characters.in_set(AuthoritySystems::Move),
                    (actions::process_actions, actions::apply_hits)
                        .chain()
                        .in_set(AuthoritySystems::Act),
                    characters::reset_dummies.in_set(AuthoritySystems::Maintain),
                ),
            );
    }
}
