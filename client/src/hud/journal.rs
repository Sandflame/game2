//! Quests on screen: the tracker (current step of each quest, under the
//! minimap), the quest log (J: every quest with its summary and steps),
//! and the gold `!` / `?` over the heads of people with quests.

use bevy::prelude::*;
use shared::components::NpcId;
use shared::gamedata::GameData;
use shared::quests::{QuestLog, step_text};

use super::{font, palette, text_shadow};
use crate::characters::LocalPlayer;

pub struct JournalPlugin;

impl Plugin for JournalPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<QuestJournal>()
            .add_systems(Startup, (spawn_tracker, spawn_journal))
            .add_systems(
                Update,
                (
                    toggle_journal,
                    update_tracker,
                    update_journal,
                    update_markers,
                ),
            );
    }
}

/// Whether the quest log (J) is open.
#[derive(Resource, Default)]
pub struct QuestJournal {
    pub open: bool,
}

/// Where the tracker starts (just under the minimap).
pub const TRACKER_TOP: f32 = 250.0;
const TRACKER_WIDTH: f32 = 230.0;

#[derive(Component)]
struct Tracker;

#[derive(Component)]
struct JournalRoot;

#[derive(Component)]
struct JournalText;

/// The `!` or `?` over a person's head (spawned with their nameplate).
#[derive(Component)]
pub struct QuestMarker {
    pub of: Entity,
}

fn spawn_tracker(mut commands: Commands) {
    commands.spawn((
        Tracker,
        Node {
            position_type: PositionType::Absolute,
            top: px(TRACKER_TOP),
            right: px(16),
            width: px(TRACKER_WIDTH),
            flex_direction: FlexDirection::Column,
            row_gap: px(6),
            ..default()
        },
    ));
}

fn spawn_journal(mut commands: Commands) {
    commands
        .spawn((
            JournalRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                top: percent(12),
                justify_content: JustifyContent::Center,
                ..default()
            },
            Visibility::Hidden,
            GlobalZIndex(9),
        ))
        .with_child((
            Node {
                width: px(560),
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(16)),
                row_gap: px(6),
                border: UiRect::all(px(2)),
                ..default()
            },
            BackgroundColor(palette::PANEL.with_alpha(0.95)),
            BorderColor::all(palette::PANEL_BORDER),
            children![
                (
                    Text::new("Quests  (J to close)"),
                    font(22.0),
                    TextColor(palette::TEXT)
                ),
                (
                    JournalText,
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(4),
                        ..default()
                    },
                ),
            ],
        ));
}

pub(super) fn toggle_journal(keys: Res<ButtonInput<KeyCode>>, mut journal: ResMut<QuestJournal>) {
    if keys.just_pressed(KeyCode::KeyJ) {
        journal.open = !journal.open;
    } else if keys.just_pressed(KeyCode::Escape) {
        journal.open = false;
    }
}

/// Quest names and current steps, for quests in progress (sorted by name).
fn active_quests<'a>(data: &'a GameData, log: &QuestLog) -> Vec<(&'a str, String)> {
    let mut list: Vec<(&str, String)> = log
        .active
        .iter()
        .filter_map(|(id, active)| {
            let def = data.quests.get(id)?;
            Some((def.name.as_str(), step_text(&data.quests, id, active)))
        })
        .collect();
    list.sort();
    list
}

fn update_tracker(
    mut commands: Commands,
    data: Res<GameData>,
    log: Option<Single<Ref<QuestLog>, With<LocalPlayer>>>,
    tracker: Single<Entity, With<Tracker>>,
) {
    let Some(log) = log else {
        return;
    };
    if !log.is_changed() {
        return;
    }
    commands.entity(*tracker).despawn_children();
    let quests = active_quests(&data, &log);
    if quests.is_empty() {
        return;
    }
    commands.entity(*tracker).with_children(|list| {
        list.spawn((
            Text::new("Quests"),
            font(14.0),
            TextColor(palette::TEXT_DIM),
            text_shadow(),
        ));
        for (name, step) in quests {
            list.spawn((
                Node {
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::axes(px(8), px(4)),
                    ..default()
                },
                BackgroundColor(Color::srgba(0.05, 0.04, 0.10, 0.55)),
                children![
                    (
                        Text::new(name),
                        font(14.0),
                        TextColor(palette::BANNER),
                        text_shadow()
                    ),
                    (
                        Text::new(format!("- {step}")),
                        font(13.0),
                        TextColor(palette::TEXT),
                        text_shadow()
                    ),
                ],
            ));
        }
    });
}

fn update_journal(
    mut commands: Commands,
    data: Res<GameData>,
    journal: Res<QuestJournal>,
    log: Option<Single<Ref<QuestLog>, With<LocalPlayer>>>,
    mut root: Single<&mut Visibility, With<JournalRoot>>,
    text: Single<Entity, With<JournalText>>,
) {
    let wanted = if journal.open {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    if **root != wanted {
        **root = wanted;
    }
    let Some(log) = log else {
        return;
    };
    if !journal.open || !(journal.is_changed() || log.is_changed()) {
        return;
    }
    commands.entity(*text).despawn_children();
    let mut ids: Vec<&String> = log.active.keys().collect();
    ids.sort_by_key(|id| data.quests.get(*id).map(|q| q.name.clone()));
    let mut finished: Vec<&str> = log
        .done
        .iter()
        .filter_map(|id| data.quests.get(id).map(|q| q.name.as_str()))
        .collect();
    finished.sort();
    commands.entity(*text).with_children(|list| {
        if ids.is_empty() {
            list.spawn((
                Text::new("No quests right now. Look for people with a gold ! over their heads."),
                font(14.0),
                TextColor(palette::TEXT_DIM),
            ));
        }
        for id in ids {
            let (Some(def), Some(active)) = (data.quests.get(id), log.active.get(id)) else {
                continue;
            };
            list.spawn((
                Text::new(&def.name),
                font(17.0),
                TextColor(palette::BANNER),
                Node {
                    margin: UiRect::top(px(8)),
                    ..default()
                },
            ));
            list.spawn((
                Text::new(&def.summary),
                font(13.0),
                TextColor(palette::TEXT),
            ));
            for (i, step) in def.steps.iter().enumerate() {
                let (mark, color) = if i < active.step {
                    ("[x]", palette::TEXT_DIM)
                } else if i == active.step {
                    ("[ ]", palette::TEXT)
                } else {
                    ("[ ]", palette::TEXT_DIM.with_alpha(0.5))
                };
                let text = if i == active.step {
                    step_text(&data.quests, id, active)
                } else {
                    step.text.clone()
                };
                list.spawn((
                    Text::new(format!("  {mark} {text}")),
                    font(13.0),
                    TextColor(color),
                ));
            }
        }
        if !finished.is_empty() {
            list.spawn((
                Text::new(format!("Finished: {}", finished.join(", "))),
                font(13.0),
                TextColor(palette::TEXT_DIM),
                Node {
                    margin: UiRect::top(px(10)),
                    ..default()
                },
            ));
        }
    });
}

fn update_markers(
    data: Res<GameData>,
    log: Option<Single<&QuestLog, With<LocalPlayer>>>,
    people: Query<&NpcId>,
    mut markers: Query<(&QuestMarker, &mut Text)>,
) {
    let Some(log) = log else {
        return;
    };
    for (marker, mut text) in &mut markers {
        let wanted = people
            .get(marker.of)
            .ok()
            .and_then(|id| log.marker(&data.quests, &id.0))
            .map_or(String::new(), String::from);
        if text.0 != wanted {
            text.0 = wanted;
        }
    }
}
