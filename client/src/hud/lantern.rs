//! The lantern panel (L): pick which flame burns in your lantern, which
//! sets your class (out of combat; changing takes a moment and moving
//! cancels it), and your class's specialization (out of combat, instant).

use bevy::prelude::*;
use shared::classes::{ChosenSpecs, CurrentClass};
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
            .add_systems(
                Update,
                (
                    toggle_panel,
                    choose_flame,
                    choose_spec,
                    update_panel,
                    list_specs,
                )
                    .chain(),
            );
    }
}

/// Whether the lantern panel is open.
#[derive(Resource, Default)]
pub struct LanternPanel {
    pub open: bool,
}

#[derive(Component)]
struct PanelRoot;

/// Where the secondary flame picker goes.
#[derive(Component)]
pub struct SecondarySection;

/// A button that changes to this class.
#[derive(Component)]
struct FlameButton(String);

/// Where the current class's specializations are listed.
#[derive(Component)]
struct SpecSection;

/// A button that switches to this specialization.
#[derive(Component)]
struct SpecButton(String);

/// The specialization line on a flame button.
#[derive(Component)]
struct FlameSpecLine(String);

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
                top: percent(4),
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
                    width: px(860),
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
                panel
                    .spawn(Node {
                        column_gap: px(16),
                        ..default()
                    })
                    .with_children(|columns| {
                        columns
                            .spawn(Node {
                                flex_direction: FlexDirection::Column,
                                row_gap: px(7),
                                width: px(420),
                                ..default()
                            })
                            .with_children(|panel| {
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
                                                Text::new(format!(
                                                    "{}   ({})",
                                                    class.name,
                                                    class.role.label()
                                                )),
                                                font(16.0),
                                                TextColor(flame),
                                            ));
                                            button.spawn((
                                                Text::new(class.description.clone()),
                                                font(12.0),
                                                TextColor(palette::TEXT_DIM),
                                            ));
                                            button.spawn((
                                                FlameSpecLine(id.clone()),
                                                Text::new(format!(
                                                    "{spec}   Health {}   Power {:.0}%",
                                                    class.max_health, class.power
                                                )),
                                                font(12.0),
                                                TextColor(palette::TEXT_DIM),
                                            ));
                                        });
                                }
                            });
                        columns
                            .spawn(Node {
                                flex_direction: FlexDirection::Column,
                                row_gap: px(7),
                                width: px(400),
                                ..default()
                            })
                            .with_children(|panel| {
                                // Filled in by `list_specs`.
                                panel.spawn((
                                    SpecSection,
                                    Node {
                                        flex_direction: FlexDirection::Column,
                                        row_gap: px(4),
                                        ..default()
                                    },
                                ));
                                // Filled in by `secondary.rs`.
                                panel.spawn((
                                    SecondarySection,
                                    Node {
                                        flex_direction: FlexDirection::Column,
                                        row_gap: px(5),
                                        ..default()
                                    },
                                ));
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

/// Clicking a specialization switches to it (the panel stays open).
fn choose_spec(
    buttons: Query<(&Interaction, &SpecButton), Changed<Interaction>>,
    me: Res<LocalPlayerId>,
    mut link: ResMut<Link>,
) {
    for (interaction, button) in &buttons {
        if *interaction == Interaction::Pressed {
            send(
                &mut link,
                *me,
                ClientRequest::ChangeSpec {
                    spec: button.0.clone(),
                },
            );
        }
    }
}

/// List the current class's specializations (rebuilt when the class or
/// specialization changes).
fn list_specs(
    mut commands: Commands,
    data: Res<GameData>,
    player: Option<Single<(Ref<CurrentClass>, &ChosenSpecs), With<LocalPlayer>>>,
    section: Single<Entity, With<SpecSection>>,
    mut buttons: Query<(&Interaction, &mut BackgroundColor), With<SpecButton>>,
    mut spec_lines: Query<(&FlameSpecLine, &mut Text)>,
) {
    for (interaction, mut background) in &mut buttons {
        background.0 = match interaction {
            Interaction::Hovered | Interaction::Pressed => Color::srgba(1.0, 1.0, 1.0, 0.08),
            Interaction::None => Color::srgba(0.0, 0.0, 0.0, 0.35),
        };
    }
    let Some(player) = player else {
        return;
    };
    let (current, chosen) = &*player;
    if !current.is_changed() {
        return;
    }
    // Each flame shows the specialization it will burn with.
    for (line, mut text) in &mut spec_lines {
        if let Some(class) = data.classes.get(&line.0) {
            let spec_id = chosen.spec_of(&line.0, class);
            let spec = class.spec(&spec_id).map_or("", |s| s.name.as_str());
            text.0 = format!(
                "{spec}   Health {}   Power {:.0}%",
                class.max_health, class.power
            );
        }
    }
    let Some(class) = data.classes.get(&current.class) else {
        return;
    };
    commands.entity(*section).despawn_children();
    let (flame, _) = flame_look(&class.flame);
    commands.entity(*section).with_children(|list| {
        list.spawn((
            Text::new(format!(
                "{} specialization (out of combat, instant)",
                class.name
            )),
            font(14.0),
            TextColor(palette::TEXT),
        ));
        for spec in &class.specializations {
            let chosen = spec.id == current.spec;
            let abilities: Vec<&str> = spec
                .abilities
                .iter()
                .filter_map(|a| data.abilities.get(a).map(|a| a.name.as_str()))
                .collect();
            list.spawn((
                Button,
                SpecButton(spec.id.clone()),
                Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::axes(px(8), px(4)),
                    border: UiRect::all(px(if chosen { 2 } else { 1 })),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
                BorderColor::all(if chosen { flame } else { palette::PANEL_BORDER }),
                children![
                    (
                        Text::new(if chosen {
                            format!("{}  (current)", spec.name)
                        } else {
                            spec.name.clone()
                        }),
                        font(14.0),
                        TextColor(if chosen { flame } else { palette::TEXT }),
                    ),
                    (
                        Text::new(spec.description.clone()),
                        font(11.0),
                        TextColor(palette::TEXT_DIM),
                    ),
                    (
                        Text::new(format!("Slots 6-8: {}", abilities.join(", "))),
                        font(11.0),
                        TextColor(palette::TEXT_DIM),
                    ),
                ],
            ));
        }
    });
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
