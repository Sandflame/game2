//! Developer helpers, switched on by environment variables. Used to check
//! the game on machines with no screen (for example automated test runs).
//!
//! - `LANTERNFLAME_SCREENSHOT=shot.png` saves a screenshot and quits.
//! - `LANTERNFLAME_DEMO=trial` also plays a scripted scene: switch to
//!   Elementalist, walk through the portal, pull the Rootwarden, dodge a
//!   marker, then (cheating) see it fall. `classes`, `progress`, `world`
//!   and `dungeon` (the dungeon board, then a quick tour of the Tangled
//!   Burrow, cheating past each fight) play other scenes. Screenshots are
//!   saved as `shot-1.png`, `shot-2.png`, …
//!
//! Demos may cheat (move the player, defeat enemies) by writing logic
//! components directly; normal play never does.

use std::f32::consts::{FRAC_PI_2, PI};
use std::path::PathBuf;

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};
use server::EnemyKind;
use shared::combat::Health;
use shared::components::{Motion, Zone};
use shared::movement::yaw_from_direction;
use shared::protocol::{ClientRequest, Link};

use crate::camera::FollowCamera;
use crate::characters::{LocalPlayer, ScriptedMove};
use crate::gear::ShowRarity;
use crate::hud::character::CharacterPanel;
use crate::hud::dialogue::Conversation;
use crate::hud::journal::QuestJournal;
use crate::hud::lantern::LanternPanel;
use crate::hud::map::WorldMap;
use crate::hud::options::OptionsMenu;
use crate::menus::{Choice, FieldId, MenuAction, MenuActions, TextField};
use crate::session::{LocalPlayerId, send};
use crate::targeting::CurrentTarget;
use shared::classes::SecondaryChoice;
use shared::items::Rarity;

const SCREENSHOT_ENV: &str = "LANTERNFLAME_SCREENSHOT";

/// The zone a scripted demo starts in (new characters normally start in
/// the hub).
pub fn demo_start_zone() -> Option<&'static str> {
    if !demo_mode() {
        return None;
    }
    match std::env::var(DEMO_ENV).as_deref() {
        Ok("classes" | "progress" | "specs" | "models" | "races" | "gear") => Some("sandbox"),
        Ok("world" | "dungeon" | "quests" | "menus") | Err(_) => None,
        Ok(_) => Some("trial_rootwarden"),
    }
}

/// The menus demo's save file: a fresh one in the temporary folder (the
/// other demos have none, so they never save).
pub fn demo_save_file() -> Option<PathBuf> {
    if std::env::var(DEMO_ENV).as_deref() != Ok("menus") {
        return None;
    }
    let path = std::env::temp_dir().join("lanternflame-menus-demo.db");
    for end in ["", "-wal", "-shm"] {
        let mut file = path.clone().into_os_string();
        file.push(end);
        let _ = std::fs::remove_file(file);
    }
    Some(path)
}

/// Is the game running a scripted screenshot (which never saves, except
/// the menus demo)?
pub fn demo_mode() -> bool {
    std::env::var_os(SCREENSHOT_ENV).is_some()
}
const DEMO_ENV: &str = "LANTERNFLAME_DEMO";

/// Seconds after the last step before quitting, so the last screenshot is saved.
const EXIT_AFTER_LAST: f32 = 1.5;

/// Seconds after start-up for a plain screenshot (lets shaders compile).
const PLAIN_SHOT_AT: f32 = 2.0;

/// One step of the scripted demo, at a time in seconds.
#[derive(Clone, Copy)]
enum Step {
    OpenLantern,
    OpenOptions,
    OpenCharacter,
    CloseCharacter,
    /// Borrow an ability from another class (secondary flame).
    Secondary(&'static str, &'static str),
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
    /// Turn the camera to look from this direction (radians; 0 = from behind
    /// a character facing -Z, π = from the front).
    CameraYaw(f32),
    Shot,
    /// Cheat: move the player to (x, z), facing this way (radians).
    Teleport(f32, f32, f32),
    /// Cheat: defeat every enemy of this kind in the player's zone.
    Defeat(&'static str),
    /// Pick a place on the dungeon board.
    Board(&'static str),
    /// Open or close the big map / the quest log.
    Map(bool),
    Journal(bool),
    /// Skip the conversation on screen.
    SkipDialogue,
    /// Switch specialization.
    Spec(&'static str),
    /// Cheat: change the player's look (race, face, skin, hair, feature,
    /// feature colour, height 0-1).
    Look(&'static str, usize, usize, usize, usize, usize, f32),
    /// Cheat: show gear of this rarity whatever is worn.
    Rarity(Rarity),
    /// Type into a box on the menus.
    Type(FieldId, &'static str),
    /// Press a menu button.
    Menu(fn() -> MenuAction),
}

/// Starts in the Training Grounds: the five gear rarities on a
/// Blademaster, from the front, then Legendary from behind and on an
/// Elementalist.
const GEAR_DEMO: &[(f32, Step)] = &[
    (0.5, Step::Teleport(0.0, 2.0, 0.0)),
    (0.6, Step::Camera(0.12, 4.5)),
    (0.7, Step::CameraYaw(PI * 0.85)),
    (3.0, Step::Shot), // Common
    (3.2, Step::Rarity(Rarity::Uncommon)),
    (5.0, Step::Shot),
    (5.2, Step::Rarity(Rarity::Rare)),
    (7.0, Step::Shot),
    (7.2, Step::Rarity(Rarity::Epic)),
    (9.0, Step::Shot),
    (9.2, Step::Rarity(Rarity::Legendary)),
    (11.0, Step::Shot),
    (11.2, Step::CameraYaw(0.0)),
    (13.0, Step::Shot), // wings from behind
    (13.2, Step::ChangeClass("elementalist")),
    (13.3, Step::CameraYaw(PI * 0.85)),
    (18.0, Step::Shot), // a Legendary staff
];

/// Starts at the login screen with an empty save file: make an account,
/// make two characters, pick one and play.
const MENUS_DEMO: &[(f32, Step)] = &[
    (0.3, Step::Type(FieldId::Account, "sandflame")),
    (0.3, Step::Type(FieldId::Password, "lantern")),
    (2.0, Step::Shot), // the login screen
    (2.2, Step::Menu(|| MenuAction::Register)),
    (4.0, Step::Shot), // no characters yet
    (4.2, Step::Menu(|| MenuAction::NewCharacter)),
    (6.5, Step::Shot), // creation: the first choice of everything
    (6.7, Step::Type(FieldId::Name, "ari")),
    (6.7, Step::Menu(|| MenuAction::Race("drake".into()))),
    (6.8, Step::Menu(|| MenuAction::Class("elementalist".into()))),
    (6.9, Step::Menu(|| MenuAction::Step(Choice::Face, 1))),
    (7.0, Step::Menu(|| MenuAction::Step(Choice::Skin, 2))),
    (
        7.1,
        Step::Menu(|| MenuAction::Step(Choice::FeatureColor, 2)),
    ),
    (9.5, Step::Shot), // a drake elementalist
    (9.7, Step::Menu(|| MenuAction::Create)),
    (11.0, Step::Menu(|| MenuAction::NewCharacter)),
    (11.5, Step::Type(FieldId::Name, "mira")),
    (11.5, Step::Menu(|| MenuAction::Race("lynari".into()))),
    (11.6, Step::Menu(|| MenuAction::Class("priest".into()))),
    (11.7, Step::Menu(|| MenuAction::Step(Choice::Hair, 3))),
    (14.0, Step::Shot), // a lynari priest
    (14.2, Step::Menu(|| MenuAction::Create)),
    (16.5, Step::Shot), // the list with both, the new one picked
    (16.7, Step::Menu(|| MenuAction::Select(0))),
    (18.5, Step::Shot), // the first one picked
    (18.7, Step::Menu(|| MenuAction::Play)),
    (23.0, Step::Shot), // in the game as Ari
];

/// Starts in the Training Grounds: each race up close, from the front and
/// from behind.
const RACES_DEMO: &[(f32, Step)] = &[
    (0.5, Step::Teleport(0.0, 2.0, 0.0)),
    (0.6, Step::Camera(0.12, 4.0)),
    (0.7, Step::CameraYaw(PI)),
    (1.0, Step::Look("human", 0, 1, 2, 0, 0, 0.5)),
    (3.5, Step::Shot), // Human, long auburn hair
    (3.7, Step::Look("elf", 1, 1, 0, 0, 0, 1.0)),
    (6.0, Step::Shot), // Elf, short silver hair, long ears
    (6.2, Step::Look("drake", 2, 2, 0, 0, 2, 0.6)),
    (8.5, Step::Shot), // Drake, swept horns, sapphire
    (8.7, Step::Look("demon", 0, 3, 2, 1, 0, 0.6)),
    (11.0, Step::Shot), // Demon, violet skin, ram horns
    (11.2, Step::Look("lynari", 1, 0, 0, 1, 0, 0.3)),
    (13.5, Step::Shot), // Lynari, lynx ears
    (13.7, Step::CameraYaw(PI * 0.75)),
    (14.0, Step::Look("drake", 3, 0, 1, 1, 3, 0.8)),
    (16.5, Step::Shot), // Drake from behind: crown of horns, tail
    (16.7, Step::Look("lynari", 0, 1, 3, 0, 0, 0.5)),
    (19.0, Step::Shot), // Lynari from behind: tail
];

/// Starts in the Training Grounds: each class's look up close (facing the
/// camera), the lantern held up while the flame changes, and a cast.
const MODELS_DEMO: &[(f32, Step)] = &[
    (0.5, Step::Teleport(0.0, 2.0, 0.0)),
    (0.6, Step::Camera(0.12, 4.5)),
    (0.7, Step::CameraYaw(PI)),
    (3.0, Step::Shot), // Blademaster with a greatsword
    (3.2, Step::Spec("dual_blades")),
    (5.0, Step::Shot), // two swords
    (5.2, Step::ChangeClass("shield_knight")),
    (6.0, Step::Shot), // the lantern held up while the flame changes
    (9.5, Step::Shot), // Shield Knight: sword and shield
    (9.7, Step::ChangeClass("elementalist")),
    (14.0, Step::Shot), // Elementalist with a staff
    (14.2, Step::ChangeClass("priest")),
    (18.5, Step::Shot), // Priest with a wand and a book
    (18.7, Step::CameraYaw(FRAC_PI_2)),
    (18.8, Step::Walk(Some((0.0, -1.0)))),
    (20.5, Step::Shot), // running, seen from the side
];

/// Starts in the Training Grounds: pick the Scissors specialization in the
/// lantern panel, then snip a dummy five times and Shear.
const SPECS_DEMO: &[(f32, Step)] = &[
    (0.5, Step::OpenLantern),
    (2.5, Step::Shot), // the specializations of the Blademaster
    (3.0, Step::Spec("scissors")),
    (5.0, Step::Shot), // Scissors chosen; the hotbar has changed
    (6.5, Step::CloseLantern),
    (6.6, Step::Teleport(0.0, -3.3, 0.0)),
    (6.7, Step::Camera(0.35, 7.0)),
    (6.8, Step::Target("training_dummy")),
    (7.0, Step::Press(5)),
    (8.6, Step::Press(5)),
    (10.2, Step::Press(5)),
    (11.8, Step::Press(5)),
    (13.4, Step::Press(5)),
    (14.5, Step::Shot), // five notches on the dummy
    (15.0, Step::Press(6)),
    (15.7, Step::Shot), // Shear cuts them all at once
];

/// Starts in Lanternhold: take Ilsa's quest, look at the map and the quest
/// log, finish it with Fen and take the next one.
const QUEST_DEMO: &[(f32, Step)] = &[
    (0.6, Step::Teleport(5.0, 15.0, 0.0)),
    (0.7, Step::Camera(0.3, 9.0)),
    (2.0, Step::Shot), // Ilsa's gold "!", the minimap
    (3.5, Step::Teleport(4.0, 11.0, 0.0)),
    (3.6, Step::Interact),
    (6.0, Step::Shot), // her dialogue
    (7.5, Step::SkipDialogue),
    (8.0, Step::Map(true)),
    (10.0, Step::Shot), // the big map: Fen in gold, the tracker
    (11.5, Step::Map(false)),
    (11.6, Step::Teleport(-4.0, -24.0, 0.0)),
    (11.8, Step::Interact),
    (14.0, Step::SkipDialogue),
    (14.5, Step::Interact), // Fen offers the next quest
    (17.0, Step::Shot),     // quest complete, Fen's dialogue
    (18.5, Step::SkipDialogue),
    (18.6, Step::Journal(true)),
    (20.5, Step::Shot), // the quest log
    (22.0, Step::Journal(false)),
    (22.1, Step::Map(true)),
    (24.0, Step::Shot), // the slide's doorway in gold
];

/// Starts in the trial (see `demo_start_zone`).
const TRIAL_DEMO: &[(f32, Step)] = &[
    (0.3, Step::ChangeClass("elementalist")),
    (3.1, Step::Target("rootwarden")),
    (3.2, Step::Camera(0.25, 9.0)),
    (4.1, Step::Shot), // the arena, its root wall and the boss, with the zone name
    (4.2, Step::Walk(Some((0.0, -1.0)))),
    (5.0, Step::Walk(None)),
    (5.1, Step::Camera(0.5, 13.0)),
    (5.2, Step::Target("rootwarden")),
    (5.3, Step::Press(1)), // Kindle: pulls the boss
    (6.1, Step::Press(0)), // Firebolt
    (9.7, Step::Shot),     // Root Slam's marker under us; the boss winds up
    (9.8, Step::Walk(Some((1.0, 0.0)))),
    (11.0, Step::Walk(None)),
    (11.55, Step::Shot), // Root Slam going off: dust over its area
    (11.8, Step::Target("rootwarden")),
    (11.9, Step::Press(0)), // Firebolt
    (16.5, Step::Shot),     // Crushing Bough's cone
    (16.8, Step::Camera(0.3, 14.0)),
    (17.0, Step::Defeat("rootwarden")),
    (18.6, Step::Shot), // falling backwards, roots drawing into the ground
    (21.5, Step::Shot), // fallen; the way out has appeared in front of it
];

/// Starts in Lanternhold at the dungeon board, then a quick (cheating) tour
/// of the Tangled Burrow.
const DUNGEON_DEMO: &[(f32, Step)] = &[
    (0.6, Step::Teleport(8.6, 1.0, -FRAC_PI_2)),
    (0.7, Step::Camera(0.3, 9.0)),
    (1.0, Step::Interact),
    (1.8, Step::Shot), // the dungeon board's list
    (1.9, Step::Board("tangled_burrow")),
    (2.0, Step::Camera(0.35, 11.0)),
    (3.6, Step::Shot), // the entrance hall, pups ahead
    (3.7, Step::Target("burrow_pup")),
    (3.8, Step::Press(0)),
    (5.5, Step::Shot), // the pups come running
    (5.6, Step::Defeat("burrow_pup")),
    (6.0, Step::Teleport(0.0, 22.5, 0.0)),
    (6.1, Step::Camera(0.4, 12.0)),
    (7.6, Step::Shot), // the Matriarch's den
    (7.7, Step::Walk(Some((0.0, -1.0)))),
    (8.3, Step::Walk(None)),
    (8.4, Step::Target("burrow_matriarch")),
    (8.5, Step::Press(0)),
    (12.1, Step::Shot), // her Pounce marker
    (12.2, Step::Defeat("burrow_matriarch")),
    (13.0, Step::Defeat("rot_spore")),
    (13.1, Step::Teleport(4.0, -9.0, 0.6)),
    (13.2, Step::Camera(0.5, 13.0)),
    (13.3, Step::Target("mother_sporecap")),
    (13.4, Step::Press(0)),
    (16.9, Step::Shot), // Mother Sporecap's ring of spores
    (17.0, Step::Defeat("mother_sporecap")),
    (17.1, Step::Defeat("burrow_pup")),
    (17.2, Step::Defeat("rot_spore")),
    (17.3, Step::Teleport(0.0, -36.5, 0.0)),
    (17.4, Step::Camera(0.3, 10.0)),
    (17.5, Step::Target("rotheart")),
    (17.6, Step::Press(0)),
    (19.4, Step::Shot), // the Rotheart, awake
    (19.5, Step::Defeat("rotheart")),
    (21.3, Step::Shot), // it topples, roots drawing in
    (24.0, Step::Shot), // the way out, and loot
    (24.1, Step::Teleport(6.5, -38.5, 0.0)),
    (24.3, Step::Interact),
    (26.0, Step::Shot), // back at the dungeon board, where we came from
];

/// Starts in Lanternhold: talk to the lamplighter, walk to the giant root,
/// slide down it, and arrive in Whisperwood.
const WORLD_DEMO: &[(f32, Step)] = &[
    (0.2, Step::Camera(0.35, 14.0)),
    (1.6, Step::Shot), // the plaza, fountain and the giant root beyond
    (1.7, Step::Walk(Some((1.0, 0.0)))),
    (2.5, Step::Walk(None)),
    (2.7, Step::Interact), // talk to Lamplighter Ilsa
    (3.4, Step::Shot),     // her speech box
    (3.45, Step::SkipDialogue),
    (3.5, Step::Walk(Some((0.0, -1.0)))),
    (10.7, Step::Walk(Some((-1.0, 0.0)))),
    (11.3, Step::Walk(None)),
    (11.5, Step::Shot), // at the root's entrance, with its prompt
    (11.6, Step::Interact),
    (14.0, Step::Shot), // sliding down inside the root
    (17.0, Step::Shot), // further down
    (20.6, Step::Shot), // arriving in Whisperwood
    (20.7, Step::Walk(Some((0.6, -0.8)))),
    (25.6, Step::Walk(None)),
    (26.4, Step::Shot), // the thornwolf pack notices us and comes running
    (26.5, Step::Teleport(54.0, -18.0, -FRAC_PI_2)),
    (26.6, Step::Camera(0.3, 10.0)),
    (28.0, Step::Shot), // the mouth of the Tangled Burrow
    (28.1, Step::Map(true)),
    (28.8, Step::Shot), // the map of Whisperwood
];

/// The character panel, then a fight with a bramble sprout in the meadow.
const PROGRESS_DEMO: &[(f32, Step)] = &[
    (0.5, Step::Secondary("priest", "pr_mend")),
    (0.8, Step::OpenLantern),
    (1.4, Step::Shot), // the secondary flame picker
    (1.5, Step::CloseLantern),
    (1.6, Step::OpenCharacter),
    (2.2, Step::Shot), // the character panel, party bonuses and the experience bar
    (2.3, Step::CloseCharacter),
    (2.1, Step::Walk(Some((-0.83, -0.56)))),
    (6.35, Step::Walk(None)),
    (6.8, Step::Target("bramble_sprout")),
    (6.9, Step::Camera(0.45, 10.0)),
    (7.0, Step::Press(0)),
    (9.0, Step::Press(1)),
    (11.0, Step::Press(2)),
    (12.0, Step::Shot), // fighting a sprout
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
    (7.6, Step::OpenOptions),
    (8.0, Step::Shot), // the options menu
];

pub struct DevToolsPlugin;

impl Plugin for DevToolsPlugin {
    fn build(&self, app: &mut App) {
        let Ok(path) = std::env::var(SCREENSHOT_ENV) else {
            return;
        };
        let steps: Vec<(f32, Step)> = match std::env::var(DEMO_ENV).as_deref() {
            Ok("classes") => CLASSES_DEMO.to_vec(),
            Ok("progress") => PROGRESS_DEMO.to_vec(),
            Ok("world") => WORLD_DEMO.to_vec(),
            Ok("dungeon") => DUNGEON_DEMO.to_vec(),
            Ok("quests") => QUEST_DEMO.to_vec(),
            Ok("specs") => SPECS_DEMO.to_vec(),
            Ok("models") => MODELS_DEMO.to_vec(),
            Ok("races") => RACES_DEMO.to_vec(),
            Ok("menus") => MENUS_DEMO.to_vec(),
            Ok("gear") => GEAR_DEMO.to_vec(),
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
    mut menus: (
        ResMut<LanternPanel>,
        ResMut<OptionsMenu>,
        ResMut<CharacterPanel>,
    ),
    mut player: Option<Single<(&mut Motion, &Zone), With<LocalPlayer>>>,
    enemies: Query<(Entity, &Motion, &EnemyKind, &Zone), Without<LocalPlayer>>,
    mut health: Query<&mut Health, With<EnemyKind>>,
    mut camera: Single<&mut FollowCamera>,
    mut scripted: ResMut<ScriptedMove>,
    mut panels: (ResMut<WorldMap>, ResMut<QuestJournal>, ResMut<Conversation>),
    mut extra: (
        MessageWriter<AppExit>,
        ResMut<MenuActions>,
        Query<&mut TextField>,
    ),
    me_entity: Option<Single<Entity, With<LocalPlayer>>>,
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
            Step::OpenLantern => menus.0.open = true,
            Step::OpenOptions => menus.1.open = true,
            Step::OpenCharacter => menus.2.set_open(true),
            Step::Secondary(class, ability) => send(
                &mut link,
                *me,
                ClientRequest::SetSecondary {
                    choice: Some(SecondaryChoice {
                        class: class.into(),
                        abilities: [Some(ability.into()), None],
                    }),
                },
            ),
            Step::CloseCharacter => menus.2.set_open(false),
            Step::CloseLantern => menus.0.open = false,
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
            Step::CameraYaw(yaw) => camera.yaw = yaw,
            Step::Target(kind) => {
                let Some(player) = player.as_ref() else {
                    continue;
                };
                let (me_at, my_zone) = (player.0.0.position, player.1);
                // The nearest one of that kind.
                let found = enemies
                    .iter()
                    .filter(|(_, _, k, z)| k.0 == kind && *z == my_zone)
                    .min_by(|a, b| {
                        a.1.0
                            .position
                            .distance(me_at)
                            .total_cmp(&b.1.0.position.distance(me_at))
                    });
                if let Some((entity, motion, ..)) = found {
                    target.0 = Some(entity);
                    let offset = motion.0.position - me_at;
                    camera.yaw = yaw_from_direction(Vec2::new(offset.x, offset.z));
                }
            }
            Step::Teleport(x, z, yaw) => {
                if let Some(player) = player.as_mut() {
                    let motion = &mut player.0;
                    motion.0.position = Vec3::new(x, 0.0, z);
                    motion.0.yaw = yaw;
                    camera.yaw = yaw;
                }
            }
            Step::Defeat(kind) => {
                let Some(player) = player.as_ref() else {
                    continue;
                };
                let my_zone = player.1;
                for (entity, _, k, z) in &enemies {
                    if k.0 == kind
                        && z == my_zone
                        && let Ok(mut health) = health.get_mut(entity)
                    {
                        health.current = 0;
                    }
                }
            }
            Step::Map(open) => panels.0.open = open,
            Step::Journal(open) => panels.1.open = open,
            Step::SkipDialogue => panels.2.skip(),
            Step::Look(race, face, skin, hair, feature, feature_color, height) => {
                if let Some(entity) = me_entity.as_deref() {
                    commands
                        .entity(*entity)
                        .insert(shared::appearance::Appearance {
                            race: race.into(),
                            face,
                            skin,
                            hair,
                            feature,
                            feature_color,
                            height,
                        });
                }
            }
            Step::Rarity(rarity) => {
                if let Some(entity) = me_entity.as_deref() {
                    commands.entity(*entity).insert(ShowRarity(rarity));
                }
            }
            Step::Type(id, text) => {
                for mut field in &mut extra.2 {
                    if field.id == id {
                        field.value = text.to_owned();
                    }
                }
            }
            Step::Menu(action) => extra.1.0.push(action()),
            Step::Spec(spec) => send(
                &mut link,
                *me,
                ClientRequest::ChangeSpec { spec: spec.into() },
            ),
            Step::Board(zone) => send(
                &mut link,
                *me,
                ClientRequest::EnterFromBoard { zone: zone.into() },
            ),
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
        extra.0.write(AppExit::Success);
    }
    *previous = now;
}
