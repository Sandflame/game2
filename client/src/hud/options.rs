//! The options menu (Esc with nothing targeted, or O): sound volume
//! 0–100, mute, lantern display, and quitting the game. Choices are saved
//! between sessions (`settings.rs`).

use bevy::prelude::*;
use bevy::ui::RelativeCursorPosition;

use super::lantern::LanternPanel;
use super::{font, palette};
use crate::audio::{Muted, SoundVolume};
use crate::characters::LanternSettings;
use crate::targeting::CurrentTarget;

pub struct OptionsPlugin;

impl Plugin for OptionsPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OptionsMenu>()
            .add_systems(Startup, spawn_menu)
            .add_systems(
                Update,
                (
                    toggle_menu
                        .before(crate::targeting::TargetingInput)
                        .before(super::lantern::toggle_panel),
                    (drag_volume, press_buttons, update_menu).chain(),
                ),
            );
    }
}

/// Whether the options menu is open. While it is, the mouse works the
/// menu instead of turning the camera.
#[derive(Resource, Default)]
pub struct OptionsMenu {
    pub open: bool,
}

/// How much the − and + buttons change the volume.
const VOLUME_STEP: u8 = 5;
const SLIDER_WIDTH: f32 = 260.0;

#[derive(Component)]
struct MenuRoot;
#[derive(Component)]
struct VolumeSlider;
#[derive(Component)]
struct VolumeFill;
#[derive(Component)]
struct VolumeLabel;
#[derive(Component)]
struct MuteLabel;
#[derive(Component)]
struct LanternLabel;

#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum MenuButton {
    Quieter,
    Louder,
    Mute,
    Lantern,
    Close,
    Quit,
}

fn button(
    parent: &mut ChildSpawnerCommands,
    kind: MenuButton,
    label: impl Bundle,
    width: Option<f32>,
) {
    parent
        .spawn((
            Button,
            kind,
            Node {
                padding: UiRect::axes(px(10), px(6)),
                border: UiRect::all(px(1)),
                justify_content: JustifyContent::Center,
                width: width.map_or(Val::Auto, px),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
            BorderColor::all(palette::PANEL_BORDER),
        ))
        .with_child(label);
}

fn spawn_menu(mut commands: Commands) {
    commands
        .spawn((
            MenuRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                top: percent(22),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Visibility::Hidden,
            GlobalZIndex(10),
        ))
        .with_children(|row| {
            row.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(16)),
                    row_gap: px(12),
                    border: UiRect::all(px(2)),
                    width: px(400),
                    ..default()
                },
                BackgroundColor(palette::PANEL.with_alpha(0.95)),
                BorderColor::all(palette::PANEL_BORDER),
            ))
            .with_children(|panel| {
                panel.spawn((Text::new("Options"), font(22.0), TextColor(palette::TEXT)));

                // Sound volume.
                panel.spawn((
                    VolumeLabel,
                    Text::new(""),
                    font(15.0),
                    TextColor(palette::TEXT),
                ));
                panel
                    .spawn(Node {
                        column_gap: px(8),
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|line| {
                        button(
                            line,
                            MenuButton::Quieter,
                            (Text::new("-"), font(16.0), TextColor(palette::TEXT)),
                            Some(32.0),
                        );
                        line.spawn((
                            Button,
                            VolumeSlider,
                            RelativeCursorPosition::default(),
                            Node {
                                width: px(SLIDER_WIDTH),
                                height: px(16),
                                border: UiRect::all(px(1)),
                                ..default()
                            },
                            BackgroundColor(palette::BAR_BACK),
                            BorderColor::all(palette::PANEL_BORDER),
                        ))
                        .with_child((
                            VolumeFill,
                            Node {
                                width: percent(50),
                                height: percent(100),
                                ..default()
                            },
                            BackgroundColor(palette::CAST),
                        ));
                        button(
                            line,
                            MenuButton::Louder,
                            (Text::new("+"), font(16.0), TextColor(palette::TEXT)),
                            Some(32.0),
                        );
                    });
                button(
                    panel,
                    MenuButton::Mute,
                    (
                        MuteLabel,
                        Text::new(""),
                        font(14.0),
                        TextColor(palette::TEXT),
                    ),
                    None,
                );
                button(
                    panel,
                    MenuButton::Lantern,
                    (
                        LanternLabel,
                        Text::new(""),
                        font(14.0),
                        TextColor(palette::TEXT),
                    ),
                    None,
                );
                panel
                    .spawn(Node {
                        column_gap: px(10),
                        justify_content: JustifyContent::SpaceBetween,
                        ..default()
                    })
                    .with_children(|line| {
                        button(
                            line,
                            MenuButton::Close,
                            (
                                Text::new("Back to the game (Esc)"),
                                font(14.0),
                                TextColor(palette::TEXT),
                            ),
                            None,
                        );
                        button(
                            line,
                            MenuButton::Quit,
                            (
                                Text::new("Quit game"),
                                font(14.0),
                                TextColor(palette::WARNING),
                            ),
                            None,
                        );
                    });
                panel.spawn((
                    Text::new("Settings are saved on this computer."),
                    font(11.0),
                    TextColor(palette::TEXT_DIM),
                ));
            });
        });
}

/// O opens and closes the menu. Esc closes it, or opens it when there is
/// nothing else for Esc to do (no target, lantern panel closed).
fn toggle_menu(
    keys: Res<ButtonInput<KeyCode>>,
    target: Res<CurrentTarget>,
    lantern: Res<LanternPanel>,
    mut menu: ResMut<OptionsMenu>,
) {
    if keys.just_pressed(KeyCode::KeyO) {
        menu.open = !menu.open;
    }
    if keys.just_pressed(KeyCode::Escape) {
        if menu.open {
            menu.open = false;
        } else if target.0.is_none() && !lantern.open {
            menu.open = true;
        }
    }
}

/// Click or drag along the slider to set the volume.
fn drag_volume(
    menu: Res<OptionsMenu>,
    slider: Single<(&Interaction, &RelativeCursorPosition), With<VolumeSlider>>,
    mut volume: ResMut<SoundVolume>,
) {
    let (interaction, cursor) = *slider;
    if !menu.open || *interaction != Interaction::Pressed {
        return;
    }
    // `normalized` runs from -0.5 (left edge) to 0.5 (right edge).
    if let Some(at) = cursor.normalized {
        let wanted = ((at.x + 0.5).clamp(0.0, 1.0) * 100.0).round() as u8;
        if volume.0 != wanted {
            volume.0 = wanted;
        }
    }
}

fn press_buttons(
    buttons: Query<(&Interaction, &MenuButton), Changed<Interaction>>,
    mut menu: ResMut<OptionsMenu>,
    mut volume: ResMut<SoundVolume>,
    mut muted: ResMut<Muted>,
    mut lantern: ResMut<LanternSettings>,
    mut exit: MessageWriter<AppExit>,
) {
    if !menu.open {
        return;
    }
    for (interaction, kind) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match kind {
            MenuButton::Quieter => volume.0 = volume.0.saturating_sub(VOLUME_STEP),
            MenuButton::Louder => volume.0 = (volume.0 + VOLUME_STEP).min(100),
            MenuButton::Mute => muted.0 = !muted.0,
            MenuButton::Lantern => lantern.always_show = !lantern.always_show,
            MenuButton::Close => menu.open = false,
            MenuButton::Quit => {
                exit.write(AppExit::Success);
            }
        }
    }
}

fn update_menu(
    menu: Res<OptionsMenu>,
    volume: Res<SoundVolume>,
    muted: Res<Muted>,
    lantern: Res<LanternSettings>,
    mut root: Single<&mut Visibility, With<MenuRoot>>,
    mut fill: Single<&mut Node, With<VolumeFill>>,
    mut labels: ParamSet<(
        Single<&mut Text, With<VolumeLabel>>,
        Single<&mut Text, With<MuteLabel>>,
        Single<&mut Text, With<LanternLabel>>,
    )>,
    mut buttons: Query<(&Interaction, &mut BackgroundColor), With<MenuButton>>,
) {
    **root = if menu.open {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    if !menu.open {
        return;
    }
    fill.width = percent(volume.0);
    let texts = [
        format!("Sound volume: {}", volume.0),
        format!("Mute all sound (M): {}", if muted.0 { "On" } else { "Off" }),
        format!(
            "Show lantern at all times: {}",
            if lantern.always_show { "On" } else { "Off" }
        ),
    ];
    let set = |text: &mut Text, wanted: &str| {
        if text.0 != wanted {
            text.0 = wanted.to_owned();
        }
    };
    set(&mut labels.p0(), &texts[0]);
    set(&mut labels.p1(), &texts[1]);
    set(&mut labels.p2(), &texts[2]);
    for (interaction, mut background) in &mut buttons {
        background.0 = match interaction {
            Interaction::Hovered | Interaction::Pressed => Color::srgba(1.0, 1.0, 1.0, 0.08),
            Interaction::None => Color::srgba(0.0, 0.0, 0.0, 0.35),
        };
    }
}
