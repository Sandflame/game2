//! Developer helpers, switched on by environment variables.
//!
//! `LANTERNFLAME_SCREENSHOT=shot.png` takes a screenshot a few seconds after
//! start-up and then quits. Used to check rendering on machines with no
//! screen (for example automated test runs).

use bevy::prelude::*;
use bevy::render::view::screenshot::{Screenshot, save_to_disk};

const SCREENSHOT_ENV: &str = "LANTERNFLAME_SCREENSHOT";
/// Frames to wait so shaders compile and shadows settle.
const SCREENSHOT_AFTER_FRAMES: u32 = 120;
const EXIT_AFTER_FRAMES: u32 = SCREENSHOT_AFTER_FRAMES + 30;

pub struct DevToolsPlugin;

impl Plugin for DevToolsPlugin {
    fn build(&self, app: &mut App) {
        if let Ok(path) = std::env::var(SCREENSHOT_ENV) {
            app.insert_resource(ScreenshotRequest { path, frames: 0 })
                .add_systems(Update, screenshot_and_quit);
        }
    }
}

#[derive(Resource)]
struct ScreenshotRequest {
    path: String,
    frames: u32,
}

fn screenshot_and_quit(
    mut commands: Commands,
    mut request: ResMut<ScreenshotRequest>,
    mut exit: MessageWriter<AppExit>,
) {
    request.frames += 1;
    if request.frames == SCREENSHOT_AFTER_FRAMES {
        commands
            .spawn(Screenshot::primary_window())
            .observe(save_to_disk(request.path.clone()));
    }
    if request.frames >= EXIT_AFTER_FRAMES {
        exit.write(AppExit::Success);
    }
}
