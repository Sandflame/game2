//! Lanternflame game client: window, rendering, input and UI.
//!
//! Until multiplayer (Milestone 11) the client also runs the game's rules
//! half (`server::AuthorityPlugin`) inside the same program. The two halves
//! only talk through `shared::protocol::Link`; see DESIGN.md §3.3.

mod animation;
mod audio;
mod camera;
mod characters;
mod creatures;
mod devtools;
mod hud;
mod models;
mod particles;
mod props;
mod session;
mod settings;
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
        let models = models::ModelLibrary::load(&assets)?;
        Ok((assets, data, zones, vfx, particles, sounds, models))
    });
    let (assets_dir, mut data, zones, vfx, particles, sounds, models) = match loaded {
        Ok(loaded) => loaded,
        Err(error) => {
            eprintln!("Lanternflame could not start: {error}");
            return AppExit::error();
        }
    };
    // Scripted screenshot demos start where they need to be.
    if let Some(zone) = devtools::demo_start_zone() {
        data.player.start_zone = zone.to_owned();
    }
    // The save file. Scripted screenshot demos always start fresh.
    let database = if devtools::demo_mode() {
        None
    } else {
        match settings::save_file_path().map(|path| server::Database::start(&path)) {
            Some(Ok(database)) => Some(database),
            Some(Err(error)) => {
                eprintln!("Lanternflame could not start: {error}");
                return AppExit::error();
            }
            None => {
                eprintln!("No place to keep a save file was found; progress won't be saved.");
                None
            }
        }
    };
    let mut problems = vfx.check_references(&data, &zones, &particles, &sounds);
    problems.extend(models.check_references(&assets_dir, &data, &zones));
    if !problems.is_empty() {
        eprintln!(
            "Lanternflame could not start: problems in assets/data/client:\n  {}",
            problems.join("\n  ")
        );
        return AppExit::error();
    }

    let mut app = App::new();
    if let Some(database) = database {
        app.insert_resource(database);
    }
    app.add_plugins(
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
    .insert_resource(models)
    // The rules half, running in-process for now.
    .add_plugins(AuthorityPlugin)
    // The screen half.
    .add_plugins((
        session::SessionPlugin,
        toon::ToonPlugin,
        world::WorldPlugin,
        characters::CharactersPlugin,
        models::ModelsPlugin,
        camera::CameraPlugin,
        targeting::TargetingPlugin,
        telegraphs::TelegraphsPlugin,
        vfx::VfxPlugin,
        particles::ParticlesPlugin,
        animation::AnimationPlugin,
        audio::SoundPlugin,
        settings::SettingsPlugin,
        hud::HudPlugin,
        devtools::DevToolsPlugin,
    ))
    .run()
}
