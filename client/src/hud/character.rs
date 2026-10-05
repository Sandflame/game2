//! The character panel (C): every class's level, your stats, what you are
//! wearing, and your bag. Click an item in the bag to put it on, click a
//! worn item to take it off, right-click an item twice to throw it away.
//! Also the experience bar under the hotbar.

use bevy::prelude::*;

use crate::gear::rarity_color;
use crate::models::ModelLibrary;
use shared::classes::{CurrentClass, Secondaries, Stats};
use shared::components::{CharacterName, Zone};
use shared::gamedata::{GameData, Zones};
use shared::items::{Bag, Equipment, ItemDef, Slot};
use shared::progression::{ClassLevels, effective_level};
use shared::protocol::{ClientRequest, Link};
use shared::statuses::Statuses;

use super::{font, palette};
use crate::characters::LocalPlayer;
use crate::session::{LocalPlayerId, send};

pub struct CharacterPanelPlugin;

impl Plugin for CharacterPanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<CharacterPanel>()
            .add_systems(Startup, (spawn_panel, spawn_xp_bar))
            .add_systems(
                Update,
                (
                    toggle_panel,
                    rebuild_panel,
                    click_items,
                    show_item_details,
                    update_xp_bar,
                )
                    .chain(),
            );
    }
}

/// Whether the character panel is open.
#[derive(Resource, Default)]
pub struct CharacterPanel {
    pub open: bool,
    /// An item right-clicked once: a second right-click throws it away.
    discard_armed: Option<(u64, f32)>,
    /// The panel needs redrawing.
    dirty: bool,
}

impl CharacterPanel {
    pub fn set_open(&mut self, open: bool) {
        self.open = open;
        self.dirty = true;
    }
}

/// Seconds to right-click again to throw an item away.
const DISCARD_WINDOW: f32 = 3.0;
const PANEL_WIDTH: f32 = 900.0;
const COLUMN_WIDTH: f32 = 430.0;
const XP_BAR_WIDTH: f32 = 420.0;

#[derive(Component)]
struct PanelRoot;
/// The part of the panel that is rebuilt when something changes.
#[derive(Component)]
struct PanelContent;
/// The two halves of the panel: levels and stats on the left, gear on the right.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum Column {
    Left,
    Right,
}

/// The line at the bottom describing the item under the mouse.
#[derive(Component)]
struct DetailLine;

#[derive(Component, Clone, Copy)]
enum ItemButton {
    /// An item in the bag (click: wear it).
    InBag(u64),
    /// Something worn in a slot (click: take it off).
    Worn(Slot, u64),
}

#[derive(Component)]
struct XpLabel;
#[derive(Component)]
struct XpFill;

fn spawn_panel(mut commands: Commands) {
    commands
        .spawn((
            PanelRoot,
            Node {
                position_type: PositionType::Absolute,
                left: px(20),
                top: px(140),
                width: px(PANEL_WIDTH),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(12)),
                row_gap: px(8),
                border: UiRect::all(px(2)),
                ..default()
            },
            // Clicks on the panel don't reach the game world behind it.
            Interaction::default(),
            BackgroundColor(palette::PANEL.with_alpha(0.94)),
            BorderColor::all(palette::PANEL_BORDER),
            Visibility::Hidden,
            GlobalZIndex(5),
        ))
        .with_children(|panel| {
            panel
                .spawn((
                    PanelContent,
                    Node {
                        column_gap: px(16),
                        ..default()
                    },
                ))
                .with_children(|columns| {
                    for column in [Column::Left, Column::Right] {
                        columns.spawn((
                            column,
                            Node {
                                flex_direction: FlexDirection::Column,
                                row_gap: px(8),
                                width: px(COLUMN_WIDTH),
                                ..default()
                            },
                        ));
                    }
                });
            panel.spawn((
                DetailLine,
                Text::new(""),
                font(12.0),
                TextColor(palette::TEXT_DIM),
                Node {
                    min_height: px(32),
                    ..default()
                },
            ));
            panel.spawn((
                Text::new(
                    "Click a bag item to wear it, a worn item to take it off. \
                     Right-click twice to throw away. C or Esc closes.",
                ),
                font(11.0),
                TextColor(palette::TEXT_DIM),
            ));
        });
}

pub(super) fn toggle_panel(keys: Res<ButtonInput<KeyCode>>, mut panel: ResMut<CharacterPanel>) {
    if keys.just_pressed(KeyCode::KeyC) {
        panel.open = !panel.open;
        panel.dirty = true;
    }
    if keys.just_pressed(KeyCode::Escape) && panel.open {
        panel.open = false;
    }
}

fn heading(parent: &mut ChildSpawnerCommands, text: &str) {
    parent.spawn((Text::new(text), font(15.0), TextColor(palette::BANNER)));
}

fn item_button(parent: &mut ChildSpawnerCommands, kind: ItemButton, label: String, color: Color) {
    parent
        .spawn((
            Button,
            kind,
            Node {
                padding: UiRect::axes(px(6), px(3)),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
            BorderColor::all(palette::PANEL_BORDER.with_alpha(0.5)),
        ))
        .with_child((Text::new(label), font(12.0), TextColor(color)));
}

/// Can the current class wear this item?
fn wearable(def: &ItemDef, class: &str, level: u32) -> bool {
    def.level <= level && def.class.as_deref().is_none_or(|c| c == class)
}

/// Redraw the panel when it opens or anything shown in it changes.
fn rebuild_panel(
    mut commands: Commands,
    mut panel: ResMut<CharacterPanel>,
    (data, models): (Res<GameData>, Res<ModelLibrary>),
    zones: Res<Zones>,
    player: Option<
        Single<
            (
                &CharacterName,
                &CurrentClass,
                &ClassLevels,
                &Stats,
                &Bag,
                &Equipment,
                &Zone,
                Ref<Bag>,
                Ref<Equipment>,
                Ref<ClassLevels>,
                Ref<Stats>,
                (&Secondaries, &Statuses),
            ),
            With<LocalPlayer>,
        >,
    >,
    mut root: Single<&mut Visibility, With<PanelRoot>>,
    columns: Query<(Entity, &Column)>,
    mut shown_bonuses: Local<Vec<String>>,
) {
    **root = if panel.open {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    let Some(player) = player else {
        return;
    };
    let (
        name,
        class,
        levels,
        stats,
        bag,
        worn,
        zone,
        bag_ref,
        worn_ref,
        levels_ref,
        stats_ref,
        (secondaries, statuses),
    ) = player.into_inner();
    // Party synergy bonuses the character has right now.
    let bonuses: Vec<String> = data
        .synergy
        .statuses()
        .filter(|id| statuses.has(id))
        .map(str::to_owned)
        .collect();
    let changed = bag_ref.is_changed()
        || worn_ref.is_changed()
        || levels_ref.is_changed()
        || stats_ref.is_changed()
        || *shown_bonuses != bonuses;
    *shown_bonuses = bonuses.clone();
    if !panel.open || !(panel.dirty || changed) {
        return;
    }
    panel.dirty = false;
    let class_id = class.class.as_str();
    let class_name = data
        .classes
        .get(class_id)
        .map_or(class_id, |c| c.name.as_str());
    let level = levels.get(class_id).level;
    let sync = zones.get(&zone.0).and_then(|z| z.level_sync);

    let column = |which: Column| columns.iter().find(|(_, c)| **c == which).map(|(e, _)| e);
    let (Some(left), Some(right)) = (column(Column::Left), column(Column::Right)) else {
        return;
    };
    commands.entity(left).despawn_children();
    commands.entity(right).despawn_children();
    commands.entity(left).with_children(|panel| {
        panel.spawn((
            Text::new(name.0.clone()),
            font(20.0),
            TextColor(palette::TEXT),
        ));

        // Classes and levels.
        heading(panel, "Lantern flames");
        for (id, def) in data.class_list() {
            let progress = levels.get(id);
            let next = data
                .progression
                .xp_needed(progress.level)
                .map_or("max level".to_owned(), |n| {
                    format!("{} / {n} XP", progress.xp)
                });
            let current = id == class_id;
            panel.spawn((
                Text::new(format!(
                    "{}{}   Level {}   {next}",
                    if current { "> " } else { "   " },
                    def.name,
                    progress.level
                )),
                font(13.0),
                TextColor(if current {
                    palette::TEXT
                } else {
                    palette::TEXT_DIM
                }),
            ));
        }

        // Stats.
        heading(panel, &format!("{class_name}, level {level}"));
        let synced = effective_level(level, sync);
        if synced < level {
            panel.spawn((
                Text::new(format!("Level synced to {synced} here.")),
                font(12.0),
                TextColor(palette::QUEUED),
            ));
        }
        let guard = if stats.guard > 0.0 {
            format!("-{:.0}%", stats.guard)
        } else {
            "normal".to_owned()
        };
        panel.spawn((
            Text::new(format!(
                "Health {}    Power {:.0}%    Crit {:.0}%    Damage taken {guard}",
                stats.max_health,
                stats.power,
                stats.crit_chance * 100.0,
            )),
            font(13.0),
            TextColor(palette::TEXT),
        ));
        let secondary = secondaries.0.get(class_id).and_then(|c| {
            data.classes
                .get(&c.class)
                .map(|def| (def, levels.get(&c.class).level))
        });
        panel.spawn((
            Text::new(match secondary {
                Some((def, lvl)) => {
                    format!(
                        "Secondary flame: {} (level {lvl}). Change it with L.",
                        def.name
                    )
                }
                None => "Secondary flame: none. Pick one with L.".to_owned(),
            }),
            font(12.0),
            TextColor(palette::TEXT_DIM),
        ));
        if !bonuses.is_empty() {
            heading(panel, "Party bonuses (for roles nobody here covers)");
            for id in &bonuses {
                if let Some(def) = data.statuses.get(id) {
                    panel.spawn((
                        Text::new(format!("{}: {}", def.name, def.description)),
                        font(12.0),
                        TextColor(palette::BUFF),
                    ));
                }
            }
        }
    });
    commands.entity(right).with_children(|panel| {
        // Worn gear.
        heading(panel, "Wearing");
        panel
            .spawn(Node {
                flex_wrap: FlexWrap::Wrap,
                column_gap: px(6),
                row_gap: px(4),
                ..default()
            })
            .with_children(|row| {
                for slot in Slot::ALL {
                    match worn
                        .in_slot(slot, class_id)
                        .and_then(|id| bag.get(id).map(|owned| (id, owned)))
                    {
                        Some((id, owned)) => {
                            let def = data.items.get(&owned.item);
                            let item = def.map_or(owned.item.as_str(), |d| d.name.as_str());
                            let color = def
                                .and_then(|d| rarity_color(&models, d.rarity))
                                .unwrap_or(palette::TEXT);
                            item_button(
                                row,
                                ItemButton::Worn(slot, id),
                                format!("{}: {item}", slot.label()),
                                color,
                            );
                        }
                        None => {
                            row.spawn((
                                Text::new(format!("{}: -", slot.label())),
                                font(12.0),
                                TextColor(palette::TEXT_DIM),
                                Node {
                                    padding: UiRect::axes(px(6), px(4)),
                                    ..default()
                                },
                            ));
                        }
                    }
                }
            });

        // The bag (everything not worn by this class).
        let unworn: Vec<_> = bag
            .items
            .iter()
            .filter(|owned| !worn.worn_by(class_id).any(|id| id == owned.id))
            .collect();
        heading(
            panel,
            &format!(
                "Bag   ({} / {} items)",
                bag.items.len(),
                data.progression.bag_size
            ),
        );
        panel
            .spawn(Node {
                flex_wrap: FlexWrap::Wrap,
                column_gap: px(6),
                row_gap: px(4),
                ..default()
            })
            .with_children(|row| {
                if unworn.is_empty() {
                    row.spawn((
                        Text::new("(empty)"),
                        font(12.0),
                        TextColor(palette::TEXT_DIM),
                    ));
                }
                for owned in unworn {
                    let Some(def) = data.items.get(&owned.item) else {
                        continue;
                    };
                    let other_weapon = worn.is_worn(owned.id);
                    // Rarity colour if you can wear it now, else dimmed.
                    let color = if other_weapon || !wearable(def, class_id, level) {
                        palette::TEXT_DIM
                    } else {
                        rarity_color(&models, def.rarity).unwrap_or(palette::TEXT)
                    };
                    let mut label = def.name.clone();
                    if other_weapon {
                        label.push_str(" (other flame)");
                    }
                    item_button(row, ItemButton::InBag(owned.id), label, color);
                }
            });
    });
}

fn click_items(
    time: Res<Time>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut panel: ResMut<CharacterPanel>,
    data: Res<GameData>,
    buttons: Query<(&Interaction, &ItemButton)>,
    player: Option<Single<&Bag, With<LocalPlayer>>>,
    me: Res<LocalPlayerId>,
    mut link: ResMut<Link>,
    mut detail: Single<&mut Text, With<DetailLine>>,
) {
    if !panel.open {
        return;
    }
    let now = time.elapsed_secs();
    if panel
        .discard_armed
        .is_some_and(|(_, at)| now - at > DISCARD_WINDOW)
    {
        panel.discard_armed = None;
    }
    let left = mouse.just_pressed(MouseButton::Left);
    let right = mouse.just_pressed(MouseButton::Right);
    if !left && !right {
        return;
    }
    for (interaction, button) in &buttons {
        if *interaction == Interaction::None {
            continue;
        }
        let id = match *button {
            ItemButton::InBag(id) | ItemButton::Worn(_, id) => id,
        };
        if left {
            let request = match *button {
                ItemButton::InBag(id) => ClientRequest::Equip { item: id },
                ItemButton::Worn(slot, _) => ClientRequest::Unequip { slot },
            };
            send(&mut link, *me, request);
        } else if panel.discard_armed.is_some_and(|(armed, _)| armed == id) {
            send(&mut link, *me, ClientRequest::Discard { item: id });
            panel.discard_armed = None;
        } else {
            panel.discard_armed = Some((id, now));
            let name = player
                .as_ref()
                .and_then(|bag| bag.get(id))
                .and_then(|owned| data.items.get(&owned.item))
                .map_or("this item".to_owned(), |d| d.name.clone());
            detail.0 = format!("Right-click {name} again to throw it away.");
        }
    }
}

/// Describe the item under the mouse.
fn show_item_details(
    panel: Res<CharacterPanel>,
    data: Res<GameData>,
    buttons: Query<(&Interaction, &ItemButton), Changed<Interaction>>,
    player: Option<Single<&Bag, With<LocalPlayer>>>,
    mut detail: Single<&mut Text, With<DetailLine>>,
) {
    if !panel.open || panel.discard_armed.is_some() {
        return;
    }
    let Some(bag) = player else {
        return;
    };
    for (interaction, button) in &buttons {
        if *interaction != Interaction::Hovered {
            continue;
        }
        let id = match *button {
            ItemButton::InBag(id) | ItemButton::Worn(_, id) => id,
        };
        let Some(def) = bag.get(id).and_then(|owned| data.items.get(&owned.item)) else {
            continue;
        };
        let class = def
            .class
            .as_ref()
            .and_then(|c| data.classes.get(c))
            .map_or(String::new(), |c| format!(" ({} only)", c.name));
        detail.0 = format!(
            "{}: {} level {} {}{class}. {}\n{}",
            def.name,
            def.rarity.label().to_lowercase(),
            def.level,
            def.slot.label().to_lowercase(),
            def.summary(),
            def.description
        );
    }
}

fn spawn_xp_bar(mut commands: Commands) {
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: px(3),
            width: percent(100),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            column_gap: px(8),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                XpLabel,
                Text::new(""),
                font(11.0),
                TextColor(palette::TEXT),
                super::text_shadow(),
            ));
            row.spawn((
                Node {
                    width: px(XP_BAR_WIDTH),
                    height: px(6),
                    border: UiRect::all(px(1)),
                    ..default()
                },
                BackgroundColor(palette::BAR_BACK),
                BorderColor::all(Color::srgba(0.0, 0.0, 0.0, 0.9)),
            ))
            .with_child((
                XpFill,
                Node {
                    width: percent(0),
                    height: percent(100),
                    ..default()
                },
                BackgroundColor(palette::COMBO),
            ));
        });
}

fn update_xp_bar(
    data: Res<GameData>,
    player: Option<Single<(&CurrentClass, &ClassLevels), With<LocalPlayer>>>,
    mut label: Single<&mut Text, With<XpLabel>>,
    mut fill: Single<&mut Node, With<XpFill>>,
) {
    let Some(player) = player else {
        return;
    };
    let (class, levels) = *player;
    let progress = levels.get(&class.class);
    let (text, fraction) = match data.progression.xp_needed(progress.level) {
        Some(needed) => (
            format!("Level {}   {} / {needed} XP", progress.level, progress.xp),
            progress.xp as f32 / needed as f32,
        ),
        None => (format!("Level {} (max)", progress.level), 1.0),
    };
    if label.0 != text {
        label.0 = text;
    }
    fill.width = percent(fraction * 100.0);
}
