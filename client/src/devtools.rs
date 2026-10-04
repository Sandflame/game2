//! Developer helpers, switched on by environment variables. Used to check
//! the game on machines with no screen (for example automated test runs).
//!
//! - `LANTERNFLAME_SCREENSHOT=shot.png` saves a screenshot and quits.
//! - `LANTERNFLAME_DEMO=1` also plays a short scripted fight (targets the
//!   nearest dummy, uses abilities) and takes screenshots during it,
//!   saved as `shot-1.png`, `shot-2.png`, …

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use shared::components::{Faction, Motion};
use shared::protocol::{ClientRequest, Link};

use crate::characters::LocalPlayer;
use crate::session::{LocalPlayerId, send};
use crate::targeting::CurrentTarget;

const SCREENSHOT_ENV: &str = "LANTERNFLAME_SCREENSHOT";
const DEMO_ENV: &str = "LANTERNFLAME_DEMO";

/// Seconds after start-up for a plain screenshot (lets shaders compile).
const PLAIN_SHOT_AT: f32 = 2.0;
/// Scripted demo: (seconds after start, hotbar slot) presses…
const DEMO_PRESSES: [(f32, usize); 2] = [(1.2, 2), (2.0, 1)];
/// …and when to take screenshots (mid-cast, then just after it lands).
const DEMO_SHOTS_AT: [f32; 2] = [3.0, 4.25];
const DEMO_TARGET_AT: f32 = 1.0;

pub struct DevToolsPlugin;

impl Plugin for DevToolsPlugin {
    fn build(&self, app: &mut App) {
        let Ok(path) = std::env::var(SCREENSHOT_ENV) else {
            return;
        };
        let demo = std::env::var(DEMO_ENV).is_ok();
        let shots: Vec<(f32, PathBuf)> = if demo {
            DEMO_SHOTS_AT
                .iter()
                .enumerate()
                .map(|(i, at)| (*at, numbered(&path, i + 1)))
                .collect()
        } else {
            vec![(PLAIN_SHOT_AT, PathBuf::from(path))]
        };
        app.insert_resource(Script {
            demo,
            shots,
            done: 0,
        })
        .add_systems(Update, run_script);
    }
}

fn numbered(path: &str, n: usize) -> PathBuf {
    let path = PathBuf::from(path);
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!("{stem}-{n}.png"))
}

#[derive(Resource)]
struct Script {
    demo: bool,
    shots: Vec<(f32, PathBuf)>,
    done: usize,
}

/// Uses the game's own clock so slow software rendering doesn't change the timing.
fn run_script(
    mut commands: Commands,
    fixed: Res<Time<Fixed>>,
    mut script: ResMut<Script>,
    mut previous: Local<f32>,
    mut link: ResMut<Link>,
    me: Res<LocalPlayerId>,
    mut target: ResMut<CurrentTarget>,
    player: Option<Single<&Motion, With<LocalPlayer>>>,
    enemies: Query<(Entity, &Motion, &Faction)>,
    mut exit: MessageWriter<AppExit>,
) {
    let now = fixed.elapsed_secs();
    let crossed = |at: f32| *previous < at && now >= at;

    if script.demo {
        if crossed(DEMO_TARGET_AT)
            && let Some(player) = player
        {
            target.0 = enemies
                .iter()
                .filter(|(_, _, f)| **f == Faction::Enemy)
                .min_by(|a, b| {
                    let da = a.1.0.position.distance(player.0.position);
                    let db = b.1.0.position.distance(player.0.position);
                    da.total_cmp(&db)
                })
                .map(|(e, ..)| e);
        }
        for (at, slot) in DEMO_PRESSES {
            if crossed(at) {
                send(
                    &mut link,
                    *me,
                    ClientRequest::UseAbility {
                        slot,
                        target: target.0,
                    },
                );
            }
        }
    }

    let shots: Vec<PathBuf> = script
        .shots
        .iter()
        .filter(|(at, _)| crossed(*at))
        .map(|(_, path)| path.clone())
        .collect();
    for path in shots {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(path));
        script.done += 1;
    }
    let last = script.shots.iter().map(|(at, _)| *at).fold(0.0, f32::max);
    if script.done == script.shots.len() && now >= last + 0.5 {
        exit.write(AppExit::Success);
    }
    *previous = now;
}
