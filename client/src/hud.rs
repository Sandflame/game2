//! On-screen help text. The real HUD (hotbar, target frame) arrives in M3.

use bevy::prelude::*;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_help);
    }
}

fn spawn_help(mut commands: Commands) {
    let font = TextFont {
        font_size: FontSize::Px(16.0),
        ..default()
    };
    let lines = [
        "WASD: move    Space: jump",
        "Left-drag: look around    Right-drag: look and turn",
        "Both mouse buttons: run forward    Wheel: zoom",
    ];
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                bottom: px(12),
                left: px(12),
                padding: UiRect::all(px(8)),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.04, 0.10, 0.55)),
        ))
        .with_children(|parent| {
            for line in lines {
                parent.spawn((Text::new(line), font.clone(), TextColor(Color::WHITE)));
            }
        });
}
