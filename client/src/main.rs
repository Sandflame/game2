//! Lanternflame game client: window, rendering, input and UI.
//!
//! Until multiplayer (Milestone 11) the client also runs the game's rules
//! half (`server::AuthorityPlugin`) inside the same program. The two halves
//! only talk through `shared::protocol::Link`; see DESIGN.md §3.3.

mod camera;
mod characters;
mod devtools;
mod hud;
mod session;
mod targeting;
mod toon;
mod world;

use bevy::prelude::*;
use server::AuthorityPlugin;
use shared::data::find_assets_dir;
use shared::gamedata::GameData;

/// The zone loaded at startup.
const START_ZONE: &str = "sandbox";

fn main() -> AppExit {
    // Load and check the game data before opening a window, so mistakes
    // in data files show a clear message instead of a crash later.
    let loaded = find_assets_dir().and_then(|assets| {
        let data = GameData::load(&assets)?;
        let level = data.load_level(&assets, START_ZONE)?;
        Ok((assets, data, level))
    });
    let (assets_dir, data, level) = match loaded {
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
        .insert_resource(Time::<Fixed>::from_hz(data.config.simulation.tick_hz))
        .insert_resource(data)
        .insert_resource(level)
        // The rules half, running in-process for now.
        .add_plugins(AuthorityPlugin)
        // The screen half.
        .add_plugins((
            session::SessionPlugin,
            toon::ToonPlugin,
            world::WorldPlugin,
            characters::CharactersPlugin,
            camera::CameraPlugin,
            targeting::TargetingPlugin,
            hud::HudPlugin,
            devtools::DevToolsPlugin,
        ))
        .run()
}
