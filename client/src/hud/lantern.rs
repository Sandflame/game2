//! The lantern panel (L): pick which flame burns in your lantern, which
//! sets your class. Only works out of combat; changing takes a moment and
//! moving cancels it.

use bevy::prelude::*;
use shared::classes::CurrentClass;
use shared::gamedata::GameData;
use shared::protocol::{ClientRequest, Link};

use super::{font, palette};
use crate::characters::{LanternSettings, LocalPlayer, flame_look};
use crate::session::{LocalPlayerId, send};

pub struct LanternPlugin;

impl Plugin for LanternPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<LanternPanel>()
            .add_systems(Startup, spawn_panel)
            .add_systems(Update, (toggle_panel, choose_flame, update_panel).chain());
    }
}

/// Whether the lantern panel is open.
#[derive(Resource, Default)]
pub struct LanternPanel {
    pub open: bool,
}

#[derive(Component)]
struct PanelRoot;

/// A button that changes to this class.
#[derive(Component)]
struct FlameButton(String);

/// The "show lantern at all times" switch, and its label.
#[derive(Component)]
struct ShowLanternButton;
#[derive(Component)]
struct ShowLanternLabel;

fn spawn_panel(mut commands: Commands, data: Res<GameData>) {
    commands
        .spawn((
            PanelRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                top: percent(10),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Visibility::Hidden,
        ))
        .with_children(|row| {
            row.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(12)),
                    row_gap: px(7),
                    border: UiRect::all(px(2)),
                    width: px(460),
                    ..default()
                },
                BackgroundColor(palette::PANEL.with_alpha(0.92)),
                BorderColor::all(palette::PANEL_BORDER),
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new("Lantern Flames"),
                    font(20.0),
                    TextColor(palette::TEXT),
                ));
                panel.spawn((
                    Text::new(
                        "Choose the flame for your lantern. Out of combat only; moving cancels.",
                    ),
                    font(12.0),
                    TextColor(palette::TEXT_DIM),
                ));
                for (id, class) in data.class_list() {
                    let (flame, _) = flame_look(&class.flame);
                    let spec = class
                        .spec(&class.default_spec)
                        .map_or("", |s| s.name.as_str());
                    panel
                        .spawn((
                            Button,
                            FlameButton(id.clone()),
                            Node {
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::all(px(8)),
                                border: UiRect::all(px(2)),
                                row_gap: px(2),
                                ..default()
                            },
                            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
                            BorderColor::all(palette::PANEL_BORDER),
                        ))
                        .with_children(|button| {
                            button.spawn((
                                Text::new(format!("{}   ({})", class.name, class.role.label())),
                                font(16.0),
                                TextColor(flame),
                            ));
                            button.spawn((
                                Text::new(class.description.clone()),
                                font(12.0),
                                TextColor(palette::TEXT_DIM),
                            ));
                            button.spawn((
                                Text::new(format!(
                                    "{spec}   Health {}   Power {:.0}%",
                                    class.max_health, class.power
                                )),
                                font(12.0),
                                TextColor(palette::TEXT_DIM),
                            ));
                        });
                }
                panel
                    .spawn((
                        Button,
                        ShowLanternButton,
                        Node {
                            padding: UiRect::all(px(6)),
                            border: UiRect::all(px(1)),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
                        BorderColor::all(palette::PANEL_BORDER),
                    ))
                    .with_child((
                        ShowLanternLabel,
                        Text::new(""),
                        font(13.0),
                        TextColor(palette::TEXT),
                    ));
                panel.spawn((
                    Text::new("Power 100% = the amounts listed on abilities; 110% = 10% more."),
                    font(11.0),
                    TextColor(palette::TEXT_DIM),
                ));
                panel.spawn((
                    Text::new("Press L to close."),
                    font(12.0),
                    TextColor(palette::TEXT_DIM),
                ));
            });
        });
}

pub(super) fn toggle_panel(keys: Res<ButtonInput<KeyCode>>, mut panel: ResMut<LanternPanel>) {
    if keys.just_pressed(KeyCode::KeyL) {
        panel.open = !panel.open;
    }
    if keys.just_pressed(KeyCode::Escape) {
        panel.open = false;
    }
}

fn choose_flame(
    buttons: Query<(&Interaction, &FlameButton), Changed<Interaction>>,
    show_lantern: Query<&Interaction, (Changed<Interaction>, With<ShowLanternButton>)>,
    mut settings: ResMut<LanternSettings>,
    me: Res<LocalPlayerId>,
    mut link: ResMut<Link>,
    mut panel: ResMut<LanternPanel>,
) {
    for interaction in &show_lantern {
        if *interaction == Interaction::Pressed {
            settings.always_show = !settings.always_show;
        }
    }
    for (interaction, button) in &buttons {
        if *interaction == Interaction::Pressed {
            send(
                &mut link,
                *me,
                ClientRequest::ChangeClass {
                    class: button.0.clone(),
                },
            );
            panel.open = false;
        }
    }
}

fn update_panel(
    panel: Res<LanternPanel>,
    data: Res<GameData>,
    player: Option<Single<&CurrentClass, With<LocalPlayer>>>,
    mut root: Single<&mut Visibility, With<PanelRoot>>,
    mut buttons: Query<(
        &FlameButton,
        &Interaction,
        &mut BorderColor,
        &mut BackgroundColor,
    )>,
    settings: Res<LanternSettings>,
    mut show_label: Single<&mut Text, With<ShowLanternLabel>>,
) {
    let wanted = if settings.always_show {
        "Show lantern at all times: On (click to change)"
    } else {
        "Show lantern at all times: Off (click to change)"
    };
    if show_label.0 != wanted {
        show_label.0 = wanted.to_owned();
    }
    **root = if panel.open {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    let current = player.map(|p| p.class.clone());
    for (button, interaction, mut border, mut background) in &mut buttons {
        let lit = current.as_deref() == Some(button.0.as_str());
        let flame = data
            .classes
            .get(&button.0)
            .map_or(palette::PANEL_BORDER, |c| flame_look(&c.flame).0);
        *border = BorderColor::all(if lit { flame } else { palette::PANEL_BORDER });
        background.0 = match interaction {
            Interaction::Hovered | Interaction::Pressed => Color::srgba(1.0, 1.0, 1.0, 0.08),
            Interaction::None => Color::srgba(0.0, 0.0, 0.0, 0.35),
        };
    }
}
