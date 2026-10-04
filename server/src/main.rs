//! Lanternflame headless server.
//!
//! Until multiplayer arrives (Milestone 11) the game runs its rules inside
//! the client program, so this only checks that the game data loads.

use anyhow::Context;
use shared::data::find_assets_dir;
use shared::gamedata::GameData;

fn main() -> anyhow::Result<()> {
    let assets = find_assets_dir()?;
    let data = GameData::load(&assets).context("loading game data")?;
    let level = data
        .load_level(&assets, "sandbox")
        .context("loading the sandbox zone")?;

    println!("Lanternflame server (placeholder until multiplayer, Milestone 11)");
    println!("  assets folder: {}", assets.display());
    println!(
        "  simulation:    {} ticks per second",
        data.config.simulation.tick_hz
    );
    println!("  abilities:     {}", data.abilities.len());
    println!("  enemy types:   {}", data.enemies.len());
    println!(
        "  zone:          {} ({} obstacles, {} enemies)",
        level.name,
        level.obstacles.len(),
        level.spawns.len()
    );
    println!("All game data loaded and valid.");
    Ok(())
}
