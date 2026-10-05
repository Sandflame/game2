//! The player's own settings (volume, mute, lantern), kept in a small file
//! on their computer so they are the same next time:
//! - Windows: `%APPDATA%\Lanternflame\settings.ron`
//! - Linux: `~/.config/lanternflame/settings.ron`

use std::path::PathBuf;

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::audio::{Muted, SoundVolume};
use crate::characters::LanternSettings;

const FILE_NAME: &str = "settings.ron";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SavedSettings {
    /// Sound volume, 0–100.
    pub volume: u8,
    pub muted: bool,
    pub always_show_lantern: bool,
    /// The server last played on ("" = this computer).
    pub server: String,
}

/// Volume when there is no settings file yet.
pub const DEFAULT_VOLUME: u8 = 50;

impl Default for SavedSettings {
    fn default() -> Self {
        Self {
            volume: DEFAULT_VOLUME,
            muted: false,
            always_show_lantern: false,
            server: String::new(),
        }
    }
}

/// The server address typed on the login screen (remembered for next time).
#[derive(Resource, Default)]
pub struct LastServer(pub String);

impl SavedSettings {
    pub fn from_text(text: &str) -> Self {
        let mut settings: Self = ron::from_str(text).unwrap_or_default();
        settings.volume = settings.volume.min(100);
        settings
    }

    pub fn to_text(&self) -> String {
        // Writing these three plain values cannot fail; fall back to empty.
        ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default()).unwrap_or_default()
    }
}

/// Where the settings file lives on this computer.
fn settings_path() -> Option<PathBuf> {
    if cfg!(windows) {
        let base = std::env::var_os("APPDATA")?;
        return Some(PathBuf::from(base).join("Lanternflame").join(FILE_NAME));
    }
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("lanternflame").join(FILE_NAME))
}

/// Where the game's save file (`world.db`) lives on this computer:
/// `LANTERNFLAME_DB` if set, otherwise
/// - Windows: `%APPDATA%\Lanternflame\world.db`
/// - Linux: `~/.local/share/lanternflame/world.db`
pub fn save_file_path() -> Option<PathBuf> {
    server::database::save_file_path()
}

pub fn load() -> SavedSettings {
    settings_path()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .map(|text| SavedSettings::from_text(&text))
        .unwrap_or_default()
}

fn save(settings: &SavedSettings) {
    let Some(path) = settings_path() else {
        return;
    };
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|_| std::fs::write(&path, settings.to_text()));
    if let Err(error) = written {
        warn!("could not save settings to {}: {error}", path.display());
    }
}

pub struct SettingsPlugin;

impl Plugin for SettingsPlugin {
    fn build(&self, app: &mut App) {
        let saved = load();
        app.insert_resource(SoundVolume(saved.volume))
            .insert_resource(Muted(saved.muted))
            .insert_resource(LanternSettings {
                always_show: saved.always_show_lantern,
            })
            .insert_resource(LastServer(saved.server.clone()))
            .insert_resource(LastSaved(saved))
            .add_systems(Last, save_changes);
    }
}

/// What is in the file now, so it is only written when something changed.
#[derive(Resource)]
struct LastSaved(SavedSettings);

/// Save after a change, but not on every frame of a slider drag.
fn save_changes(
    mouse: Res<ButtonInput<MouseButton>>,
    volume: Res<SoundVolume>,
    muted: Res<Muted>,
    lantern: Res<LanternSettings>,
    server: Res<LastServer>,
    mut last: ResMut<LastSaved>,
) {
    if mouse.pressed(MouseButton::Left) {
        return;
    }
    let now = SavedSettings {
        volume: volume.0,
        muted: muted.0,
        always_show_lantern: lantern.always_show,
        server: server.0.clone(),
    };
    if now != last.0 {
        save(&now);
        last.0 = now;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_survive_bad_files() {
        let settings = SavedSettings {
            volume: 35,
            muted: true,
            always_show_lantern: true,
            server: "10.0.0.2:5888".into(),
        };
        assert_eq!(SavedSettings::from_text(&settings.to_text()), settings);
        assert_eq!(
            SavedSettings::from_text("nonsense"),
            SavedSettings::default()
        );
        assert_eq!(SavedSettings::from_text("(volume: 250)").volume, 100);
        // Missing fields get their defaults.
        assert_eq!(
            SavedSettings::from_text("(muted: true)").volume,
            DEFAULT_VOLUME
        );
    }
}
