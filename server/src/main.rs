//! Lanternflame headless server.
//!
//! Milestone 1: only checks that the game data loads, then exits.
//! Networking arrives in Milestone 2.

use anyhow::Context;
use shared::config::GameConfig;
use shared::data::find_assets_dir;
use shared::level::Level;

fn main() -> anyhow::Result<()> {
    let assets = find_assets_dir()?;
    let config = GameConfig::load(&assets).context("loading config")?;
    let level = Level::load(&assets, "sandbox").context("loading the sandbox zone")?;

    println!("Lanternflame server (Milestone 1 placeholder)");
    println!("  assets folder: {}", assets.display());
    println!(
        "  simulation:    {} ticks per second",
        config.simulation.tick_hz
    );
    println!(
        "  zone:          {} ({} obstacles)",
        level.name,
        level.obstacles.len()
    );
    println!("All game data loaded and valid. Networking arrives in Milestone 2.");
    Ok(())
}
