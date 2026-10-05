//! Lanternflame's server: runs the game's rules for everyone who connects.
//!
//! `cargo run -p server` (or `server --port 5888`). Friends connect from
//! the game's login screen by typing this computer's address and port.
//! The world is saved in the same file the game uses on this computer
//! (`LANTERNFLAME_DB` to choose another).

use std::time::Duration;

use anyhow::Context;
use bevy::app::{ScheduleRunnerPlugin, TerminalCtrlCHandlerPlugin};
use bevy::log::LogPlugin;
use bevy::prelude::*;
use server::net::{NetServerPlugin, server_plugins};
use server::{AuthorityPlugin, Database};
use shared::data::find_assets_dir;
use shared::gamedata::GameData;

fn main() -> anyhow::Result<()> {
    let assets = find_assets_dir()?;
    let data = GameData::load(&assets).context("loading game data")?;
    let zones = data.load_zones(&assets).context("loading zones")?;
    let port = port_from_args()?.unwrap_or(data.config.network.port);
    // A clear message if another server (or program) already uses the port.
    std::net::UdpSocket::bind((std::net::Ipv4Addr::UNSPECIFIED, port)).with_context(|| {
        format!("port {port} is already in use (is another Lanternflame server running?)")
    })?;
    let save_file = server::database::save_file_path()
        .context("no place to keep the save file was found; set LANTERNFLAME_DB")?;
    let database = Database::start(&save_file)
        .with_context(|| format!("opening the save file {}", save_file.display()))?;

    println!("Lanternflame server");
    println!("  save file: {}", save_file.display());
    println!("  port:      {port} (UDP)");
    println!("  Players on this computer connect to 127.0.0.1:{port}.");
    println!("  Friends connect to your public address with :{port} on the end");
    println!("  (forward UDP port {port} to this computer on your router).");
    println!("  Press Ctrl+C to stop; everyone is saved first.");

    let tick = Duration::from_secs_f64(1.0 / data.config.simulation.tick_hz);
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins.set(ScheduleRunnerPlugin::run_loop(tick)),
        LogPlugin::default(),
        bevy::state::app::StatesPlugin,
        TerminalCtrlCHandlerPlugin,
    ))
    .insert_resource(Time::<Fixed>::from_duration(tick))
    .insert_resource(database)
    .insert_resource(data.clone())
    .insert_resource(zones)
    .add_plugins(AuthorityPlugin);
    server_plugins(&mut app, &data);
    app.add_plugins(NetServerPlugin { port });
    app.run();
    Ok(())
}

/// `--port N` on the command line.
fn port_from_args() -> anyhow::Result<Option<u16>> {
    let mut args = std::env::args().skip(1);
    match args.next().as_deref() {
        None => Ok(None),
        Some("--port") => {
            let value = args.next().context("--port needs a number")?;
            Ok(Some(value.parse().context("--port needs a number")?))
        }
        Some(other) => anyhow::bail!("unknown argument `{other}` (only --port N is understood)"),
    }
}
