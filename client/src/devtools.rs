//! Developer helpers, switched on by environment variables. Used to check
//! the game on machines with no screen (for example automated test runs).
//!
//! - `LANTERNFLAME_SCREENSHOT=shot.png` saves a screenshot and quits.
//! - `LANTERNFLAME_DEMO=trial` also plays a scripted scene: switch to
//!   Elementalist, walk through the portal, pull the Rootwarden, dodge a
//!   marker. `LANTERNFLAME_DEMO=classes` plays the older lantern/sparring
//!   dummy scene. Screenshots are saved as `shot-1.png`, `shot-2.png`, …

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use server::EnemyKind;
use shared::components::Motion;
use shared::movement::yaw_from_direction;
use shared::protocol::{ClientRequest, Link};

use crate::camera::FollowCamera;
use crate::characters::{LocalPlayer, ScriptedMove};
use crate::hud::lantern::LanternPanel;
use crate::session::{LocalPlayerId, send};
use crate::targeting::CurrentTarget;

const SCREENSHOT_ENV: &str = "LANTERNFLAME_SCREENSHOT";
const DEMO_ENV: &str = "LANTERNFLAME_DEMO";

/// Seconds after the last step before quitting, so the last screenshot is saved.
const EXIT_AFTER_LAST: f32 = 1.5;

/// Seconds after start-up for a plain screenshot (lets shaders compile).
const PLAIN_SHOT_AT: f32 = 2.0;

/// One step of the scripted demo, at a time in seconds.
#[derive(Clone, Copy)]
enum Step {
    OpenLantern,
    CloseLantern,
    ChangeClass(&'static str),
    /// Target the nearest enemy of this kind and turn the camera to it.
    Target(&'static str),
    Press(usize),
    /// Start walking in this world direction (x, z), or stop with `None`.
    Walk(Option<(f32, f32)>),
    Interact,
    /// Camera pitch (radians) and distance (metres).
    Camera(f32, f32),
    Shot,
}

/// Walk from the meadow start (0, 8) to the trial portal (18, 18).
const TO_PORTAL: (f32, f32) = (0.874, 0.486);

const TRIAL_DEMO: &[(f32, Step)] = &[
    (0.3, Step::ChangeClass("elementalist")),
    (2.6, Step::Walk(Some(TO_PORTAL))),
    (5.9, Step::Shot), // the portal in the meadow, with rising motes
    (6.0, Step::Walk(None)),
    (6.3, Step::Interact),
    (6.6, Step::Target("rootwarden")),
    (6.7, Step::Camera(0.25, 9.0)),
    (7.6, Step::Shot), // the arena, its root wall and the boss, with the zone name
    (7.7, Step::Walk(Some((0.0, -1.0)))),
    (8.5, Step::Walk(None)),
    (8.6, Step::Camera(0.5, 13.0)),
    (8.7, Step::Target("rootwarden")),
    (8.8, Step::Press(1)), // Kindle: pulls the boss
    (9.6, Step::Press(0)), // Firebolt
    (13.2, Step::Shot),    // Root Slam's marker under us; the boss winds up
    (13.3, Step::Walk(Some((1.0, 0.0)))),
    (14.5, Step::Walk(None)),
    (15.05, Step::Shot), // Root Slam going off: dust over its area
    (15.3, Step::Target("rootwarden")),
    (15.4, Step::Press(0)), // Firebolt
    (20.0, Step::Shot),     // Crushing Bough's cone
];

const CLASSES_DEMO: &[(f32, Step)] = &[
    (1.0, Step::OpenLantern),
    (1.6, Step::Shot), // the lantern panel
    (1.8, Step::ChangeClass("elementalist")),
    (1.9, Step::CloseLantern),
    (3.0, Step::Shot),  // lantern held up while the flame changes
    (4.25, Step::Shot), // the new flame catches: a burst of its colour
    (4.0, Step::Target("sparring_dummy")),
    (4.1, Step::Press(1)), // Kindle: burn
    (4.8, Step::Press(3)), // Ember Shield
    (5.6, Step::Press(0)), // Firebolt (1.5 s cast)
    (7.4, Step::Shot),     // lantern put away; fight in progress
];

pub struct DevToolsPlugin;

impl Plugin for DevToolsPlugin {
    fn build(&self, app: &mut App) {
        let Ok(path) = std::env::var(SCREENSHOT_ENV) else {
            return;
        };
        let steps: Vec<(f32, Step)> = match std::env::var(DEMO_ENV).as_deref() {
            Ok("classes") => CLASSES_DEMO.to_vec(),
            Ok(_) => TRIAL_DEMO.to_vec(),
            Err(_) => vec![(PLAIN_SHOT_AT, Step::Shot)],
        };
        let numbered = steps
            .iter()
            .filter(|(_, s)| matches!(s, Step::Shot))
            .count()
            > 1;
        app.insert_resource(Script {
            steps,
            path: PathBuf::from(path),
            numbered,
            shots_taken: 0,
        })
        .add_systems(Update, run_script);
    }
}

#[derive(Resource)]
struct Script {
    steps: Vec<(f32, Step)>,
    path: PathBuf,
    numbered: bool,
    shots_taken: usize,
}

impl Script {
    fn next_path(&mut self) -> PathBuf {
        self.shots_taken += 1;
        if !self.numbered {
            return self.path.clone();
        }
        let stem = self
            .path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        self.path
            .with_file_name(format!("{stem}-{}.png", self.shots_taken))
    }
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
    mut lantern: ResMut<LanternPanel>,
    player: Option<Single<&Motion, With<LocalPlayer>>>,
    enemies: Query<(Entity, &Motion, &EnemyKind)>,
    mut camera: Single<&mut FollowCamera>,
    mut scripted: ResMut<ScriptedMove>,
    mut exit: MessageWriter<AppExit>,
) {
    let now = fixed.elapsed_secs();
    let due: Vec<Step> = script
        .steps
        .iter()
        .filter(|(at, _)| *previous < *at && now >= *at)
        .map(|(_, step)| *step)
        .collect();
    for step in due {
        match step {
            Step::OpenLantern => lantern.open = true,
            Step::CloseLantern => lantern.open = false,
            Step::ChangeClass(class) => send(
                &mut link,
                *me,
                ClientRequest::ChangeClass {
                    class: class.into(),
                },
            ),
            Step::Walk(direction) => {
                scripted.0 = direction.map(|(x, z)| Vec2::new(x, z));
            }
            Step::Interact => send(&mut link, *me, ClientRequest::Interact),
            Step::Camera(pitch, distance) => {
                camera.pitch = pitch;
                camera.target_distance = distance;
            }
            Step::Target(kind) => {
                let found = enemies.iter().find(|(_, _, k)| k.0 == kind);
                if let (Some((entity, motion, _)), Some(player)) = (found, player.as_ref()) {
                    target.0 = Some(entity);
                    let offset = motion.0.position - player.0.position;
                    camera.yaw = yaw_from_direction(Vec2::new(offset.x, offset.z));
                }
            }
            Step::Press(slot) => send(
                &mut link,
                *me,
                ClientRequest::UseAbility {
                    slot,
                    target: target.0,
                },
            ),
            Step::Shot => {
                let path = script.next_path();
                commands
                    .spawn(Screenshot::primary_window())
                    .observe(save_to_disk(path));
            }
        }
    }
    let last = script.steps.iter().map(|(at, _)| *at).fold(0.0, f32::max);
    if now >= last + EXIT_AFTER_LAST {
        exit.write(AppExit::Success);
    }
    *previous = now;
}
