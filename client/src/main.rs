//! Lanternflame game client: window, rendering, input and UI.
//!
//! Until multiplayer (Milestone 11) the client also runs the game's rules
//! half (`server::AuthorityPlugin`) inside the same program. The two halves
//! only talk through `shared::protocol::Link`; see DESIGN.md §3.3.

mod animation;
mod audio;
mod camera;
mod characters;
mod devtools;
mod hud;
mod particles;
mod session;
mod targeting;
mod telegraphs;
mod toon;
mod vfx;
mod world;

use bevy::prelude::*;
use server::AuthorityPlugin;
use shared::data::find_assets_dir;
use shared::gamedata::GameData;

fn main() -> AppExit {
    // Load and check the game data before opening a window, so mistakes
    // in data files show a clear message instead of a crash later.
    let loaded = find_assets_dir().and_then(|assets| {
        let data = GameData::load(&assets)?;
        let zones = data.load_zones(&assets)?;
        let vfx = vfx::VfxLibrary::load(&assets)?;
        let particles = particles::ParticleLibrary::load(&assets)?;
        let sounds = audio::SoundLibrary::load(&assets)?;
        Ok((assets, data, zones, vfx, particles, sounds))
    });
    let (assets_dir, data, zones, vfx, particles, sounds) = match loaded {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("Lanternflame could not start: {error}");
            return AppExit::error();
        }
    };
    let problems = vfx.check_references(&data, &zones, &particles, &sounds);
    if !problems.is_empty() {
        eprintln!(
            "Lanternflame could not start: problems in assets/data/client:\n  {}",
            problems.join("\n  ")
        );
        return AppExit::error();
    }

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
        .insert_resource(zones)
        .insert_resource(vfx)
        .insert_resource(particles)
        .insert_resource(sounds)
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
            telegraphs::TelegraphsPlugin,
            vfx::VfxPlugin,
            particles::ParticlesPlugin,
            animation::AnimationPlugin,
            audio::SoundPlugin,
            hud::HudPlugin,
            devtools::DevToolsPlugin,
        ))
        .run()
}
