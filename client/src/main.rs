//! Lanternflame game client: window, rendering, input and UI.

mod camera;
mod devtools;
mod hud;
mod player;
mod toon;
mod world;

use bevy::prelude::*;
use shared::config::GameConfig;
use shared::data::find_assets_dir;
use shared::level::Level;

/// The zone loaded at startup (Milestone 1 has only one).
const START_ZONE: &str = "sandbox";

fn main() -> AppExit {
    // Load and check the game data before opening a window, so mistakes
    // in data files show a clear message instead of a crash later.
    let loaded = find_assets_dir().and_then(|assets| {
        let config = GameConfig::load(&assets)?;
        let level = Level::load(&assets, START_ZONE)?;
        Ok((assets, config, level))
    });
    let (assets_dir, config, level) = match loaded {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("Lanternflame could not start: {error}");
            return AppExit::error();
        }
    };

    App::new()
        .add_plugins(
            DefaultPlugins
                .set(WindowPlugin {
                    primary_window: Some(Window {
                        title: "Lanternflame".into(),
                        ..default()
                    }),
                    ..default()
                })
                .set(AssetPlugin {
                    file_path: assets_dir.to_string_lossy().into_owned(),
                    ..default()
                }),
        )
        .insert_resource(Time::<Fixed>::from_hz(config.simulation.tick_hz))
        .insert_resource(config)
        .insert_resource(level)
        .add_plugins((
            toon::ToonPlugin,
            world::WorldPlugin,
            player::PlayerPlugin,
            camera::CameraPlugin,
            hud::HudPlugin,
            devtools::DevToolsPlugin,
        ))
        .run()
}
