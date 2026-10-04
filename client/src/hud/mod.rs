//! The heads-up display: hotbar, unit frames, cast bar, nameplates,
//! floating damage numbers, messages and the controls help.

mod banner;
pub mod character;
mod floating;
mod frames;
mod hotbar;
pub mod lantern;
pub mod options;
mod secondary;

use bevy::prelude::*;

pub struct HudPlugin;

impl Plugin for HudPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_help)
            .add_systems(Update, toggle_help)
            .add_plugins((
                hotbar::HotbarPlugin,
                frames::FramesPlugin,
                floating::FloatingPlugin,
                lantern::LanternPlugin,
                options::OptionsPlugin,
                character::CharacterPanelPlugin,
                secondary::SecondaryPlugin,
                banner::BannerPlugin,
            ));
    }
}

/// HUD colours, in one place so the look stays consistent.
pub mod palette {
    use bevy::prelude::*;

    pub const PANEL: Color = Color::srgba(0.06, 0.05, 0.12, 0.72);
    pub const PANEL_BORDER: Color = Color::srgba(0.75, 0.65, 0.40, 0.9);
    pub const TEXT: Color = Color::srgb(0.96, 0.94, 0.90);
    pub const TEXT_DIM: Color = Color::srgb(0.70, 0.68, 0.75);
    pub const HEALTH: Color = Color::srgb(0.35, 0.80, 0.35);
    pub const ENEMY_HEALTH: Color = Color::srgb(0.85, 0.30, 0.25);
    pub const BAR_BACK: Color = Color::srgba(0.0, 0.0, 0.0, 0.6);
    pub const CAST: Color = Color::srgb(0.95, 0.70, 0.30);
    pub const ENEMY_CAST: Color = Color::srgb(1.0, 0.45, 0.35);
    pub const BANNER: Color = Color::srgb(1.0, 0.88, 0.55);
    pub const WARNING: Color = Color::srgb(1.0, 0.40, 0.35);
    pub const QUEUED: Color = Color::srgb(1.0, 0.85, 0.30);
    pub const COMBO: Color = Color::srgb(0.45, 0.90, 1.0);
    pub const HEAL: Color = Color::srgb(0.45, 1.0, 0.55);
    pub const SHIELD: Color = Color::srgb(0.85, 0.90, 1.0);
    pub const BUFF: Color = Color::srgb(0.40, 0.70, 1.0);
    pub const DEBUFF: Color = Color::srgb(0.95, 0.40, 0.35);
}

/// The current time on the game clock, smoothed between ticks.
pub fn game_now(fixed: &Time<Fixed>) -> f64 {
    fixed.elapsed_secs_f64() + fixed.overstep().as_secs_f64()
}

/// A crisp one-pixel shadow that keeps text readable over bright scenery.
pub fn text_shadow() -> TextShadow {
    TextShadow {
        offset: Vec2::new(1.0, 1.0),
        color: Color::srgba(0.0, 0.0, 0.0, 0.85),
    }
}

/// A text style for HUD text.
pub fn font(size: f32) -> TextFont {
    TextFont {
        font_size: FontSize::Px(size),
        ..default()
    }
}

/// Spawn a horizontal bar (background + fill) and return the fill entity.
pub fn spawn_bar(
    parent: &mut ChildSpawnerCommands,
    width: f32,
    height: f32,
    color: Color,
) -> Entity {
    let mut fill = Entity::PLACEHOLDER;
    parent
        .spawn((
            Node {
                width: px(width),
                height: px(height),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(palette::BAR_BACK),
            BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.9)),
        ))
        .with_children(|bar| {
            fill = bar
                .spawn((
                    Node {
                        width: percent(100),
                        height: percent(100),
                        ..default()
                    },
                    BackgroundColor(color),
                ))
                .id();
        });
    fill
}

/// The controls list; H shows or hides it.
#[derive(Component)]
struct HelpLines;

fn spawn_help(mut commands: Commands) {
    let lines = [
        "WASD move   Space jump   Wheel zoom",
        "Left-drag look   Right-drag look + turn",
        "Both mouse buttons: run forward",
        "Tab/click target   F1 yourself   Esc clear",
        "1-0, -, = or click hotbar: abilities (9, 0: borrowed)",
        "L: lantern (class, secondary)   C: character, gear",
        "E: use a portal   M: mute sound",
        "O (or Esc): options, sound volume",
    ];
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: px(16),
                right: px(16),
                padding: UiRect::all(px(6)),
                flex_direction: FlexDirection::Column,
                ..default()
            },
            BackgroundColor(Color::srgba(0.05, 0.04, 0.10, 0.55)),
        ))
        .with_children(|parent| {
            parent.spawn((
                Text::new("H: show/hide controls"),
                font(12.0),
                TextColor(palette::TEXT),
            ));
            parent
                .spawn((
                    HelpLines,
                    Node {
                        flex_direction: FlexDirection::Column,
                        ..default()
                    },
                ))
                .with_children(|list| {
                    for line in lines {
                        list.spawn((Text::new(line), font(12.0), TextColor(palette::TEXT_DIM)));
                    }
                });
        });
}

fn toggle_help(keys: Res<ButtonInput<KeyCode>>, mut help: Single<&mut Node, With<HelpLines>>) {
    if keys.just_pressed(KeyCode::KeyH) {
        help.display = if help.display == Display::None {
            Display::Flex
        } else {
            Display::None
        };
    }
}
