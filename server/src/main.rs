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
    let zones = data.load_zones(&assets).context("loading zones")?;

    println!("Lanternflame server (placeholder until multiplayer, Milestone 11)");
    println!("  assets folder: {}", assets.display());
    println!(
        "  simulation:    {} ticks per second",
        data.config.simulation.tick_hz
    );
    println!("  classes:       {}", data.classes.len());
    println!("  abilities:     {}", data.abilities.len());
    println!("  enemy types:   {}", data.enemies.len());
    println!("  encounters:    {}", data.encounters.len());
    let mut names: Vec<_> = zones.0.iter().collect();
    names.sort_by_key(|(id, _)| id.as_str());
    for (id, level) in names {
        println!(
            "  zone {id}: {} ({} obstacles, {} enemies, {} portals{})",
            level.name,
            level.obstacles.len(),
            level.spawns.len(),
            level.portals.len(),
            level
                .encounter
                .as_ref()
                .map_or(String::new(), |e| format!(", encounter `{e}`"))
        );
    }
    println!("All game data loaded and valid.");
    Ok(())
}
