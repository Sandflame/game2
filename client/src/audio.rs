//! Sound effects. `assets/data/client/sounds.ron` names every sound file
//! and says which sound plays for which game event; spell looks in
//! `vfx.ron` can name a sound too. Press M to mute.

use std::collections::HashMap;

use bevy::audio::Volume;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use serde::Deserialize;
use shared::data::{DataError, Problems, Validate, load_ron};
use shared::protocol::ServerEvent;

use crate::characters::LocalPlayer;
use crate::session::{LocalPlayerId, Received};
use crate::world::CurrentZone;

#[derive(Debug, Clone, Deserialize)]
pub struct SoundDef {
    /// Path inside the `assets` folder.
    pub file: String,
    /// 0.0 – 1.0.
    #[serde(default = "full")]
    pub volume: f32,
}

fn full() -> f32 {
    1.0
}

/// Things that happen in the game that can have a sound (keys of `events`).
pub const EVENTS: &[&str] = &[
    "hit_taken",
    "hit_dealt",
    "crit",
    "rejected",
    "flame_change",
    "flame_caught",
    "defeated",
    "revived",
    "portal",
    "pull",
    "victory",
    "wipe",
];

#[derive(Resource, Debug, Clone, Deserialize)]
pub struct SoundLibrary {
    pub master_volume: f32,
    pub sounds: HashMap<String, SoundDef>,
    /// Game event → sound name.
    #[serde(default)]
    pub events: HashMap<String, String>,
}

impl Validate for SoundLibrary {
    fn validate(&self) -> Vec<String> {
        let mut p = Problems::default();
        p.non_negative("master_volume", self.master_volume);
        for (name, sound) in &self.sounds {
            p.non_negative(&format!("sounds.{name}.volume"), sound.volume);
        }
        for (event, sound) in &self.events {
            if !EVENTS.contains(&event.as_str()) {
                p.push(format!(
                    "events: `{event}` is not a game event (known: {})",
                    EVENTS.join(", ")
                ));
            }
            if !self.sounds.contains_key(sound) {
                p.push(format!("events.{event}: no sound called `{sound}`"));
            }
        }
        p.0
    }
}

impl SoundLibrary {
    pub fn load(assets_dir: &std::path::Path) -> Result<Self, DataError> {
        load_ron(&assets_dir.join("data").join("client").join("sounds.ron"))
    }
}

/// Loaded sound files.
#[derive(Resource, Default)]
struct SoundHandles(HashMap<String, Handle<AudioSource>>);

/// Player's choice: all sound off (M).
#[derive(Resource, Default)]
pub struct Muted(pub bool);

/// Everything needed to play a sound from any system.
#[derive(SystemParam)]
pub struct Sounds<'w, 's> {
    commands: Commands<'w, 's>,
    library: Res<'w, SoundLibrary>,
    handles: Res<'w, SoundHandles>,
    muted: Res<'w, Muted>,
}

impl Sounds<'_, '_> {
    /// Play a sound by its name in `sounds.ron`.
    pub fn play(&mut self, name: &str) {
        if self.muted.0 {
            return;
        }
        let (Some(def), Some(handle)) = (self.library.sounds.get(name), self.handles.0.get(name))
        else {
            return;
        };
        let volume = def.volume * self.library.master_volume;
        self.commands.spawn((
            AudioPlayer::new(handle.clone()),
            PlaybackSettings::DESPAWN.with_volume(Volume::Linear(volume)),
        ));
    }

    /// Play whatever sound `sounds.ron` gives this game event, if any.
    pub fn event(&mut self, event: &str) {
        if let Some(name) = self.library.events.get(event).cloned() {
            self.play(&name);
        }
    }
}

pub struct SoundPlugin;

impl Plugin for SoundPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<SoundHandles>()
            .init_resource::<Muted>()
            .add_systems(Startup, load_sounds)
            .add_systems(Update, (toggle_mute, sounds_for_events));
    }
}

fn load_sounds(
    library: Res<SoundLibrary>,
    server: Res<AssetServer>,
    mut handles: ResMut<SoundHandles>,
) {
    for (name, def) in &library.sounds {
        handles
            .0
            .insert(name.clone(), server.load(def.file.clone()));
    }
}

fn toggle_mute(keys: Res<ButtonInput<KeyCode>>, mut muted: ResMut<Muted>) {
    if keys.just_pressed(KeyCode::KeyM) {
        muted.0 = !muted.0;
    }
}

/// Sounds for things that happen to (or are done by) the local player,
/// and for the fight in their zone.
fn sounds_for_events(
    mut received: MessageReader<Received>,
    me: Res<LocalPlayerId>,
    local: Option<Single<Entity, With<LocalPlayer>>>,
    current: Res<CurrentZone>,
    mut sounds: Sounds,
) {
    let local = local.map(|e| *e);
    let is_me = |entity: &Entity| Some(*entity) == local;
    let here = |zone: &str| current.0.as_deref() == Some(zone);
    for Received(event) in received.read() {
        match event {
            ServerEvent::Damage {
                source,
                target,
                crit,
                tick: false,
                ..
            } => {
                if *crit && (is_me(source) || is_me(target)) {
                    sounds.event("crit");
                } else if is_me(target) {
                    sounds.event("hit_taken");
                } else if is_me(source) {
                    sounds.event("hit_dealt");
                }
            }
            ServerEvent::Rejected { player, .. } if *player == me.0 => sounds.event("rejected"),
            ServerEvent::FlameChangeStarted { user, .. } if is_me(user) => {
                sounds.event("flame_change")
            }
            ServerEvent::ClassChanged { user, .. } if is_me(user) => sounds.event("flame_caught"),
            ServerEvent::Defeated { entity } if is_me(entity) => sounds.event("defeated"),
            ServerEvent::Revived { entity } if is_me(entity) => sounds.event("revived"),
            ServerEvent::ZoneChanged { entity, .. } if is_me(entity) => sounds.event("portal"),
            ServerEvent::EncounterStarted { zone, .. } if here(zone) => sounds.event("pull"),
            ServerEvent::EncounterWon { zone, .. } if here(zone) => sounds.event("victory"),
            ServerEvent::EncounterWiped { zone, .. } if here(zone) => sounds.event("wipe"),
            _ => {}
        }
    }
}
