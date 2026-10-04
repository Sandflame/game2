//! The dungeon board (in Lanternhold): press E at it to see every dungeon
//! and trial, and pick one to go straight there. It closes with Esc, its
//! close button, or by walking away.

use bevy::prelude::*;
use shared::components::{Motion, Zone};
use shared::gamedata::Zones;
use shared::protocol::{ClientRequest, Link, ServerEvent};

use super::{font, palette};
use crate::characters::LocalPlayer;
use crate::session::{LocalPlayerId, Received, send};

pub struct BoardPlugin;

impl Plugin for BoardPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DungeonBoard>()
            .add_systems(Startup, spawn_board)
            .add_systems(
                Update,
                (open_board, close_board, press_buttons, show_board).chain(),
            );
    }
}

/// Whether the board's list is open.
#[derive(Resource, Default)]
pub struct DungeonBoard {
    pub open: bool,
    /// The list needs rebuilding (it was just opened).
    fresh: bool,
}

#[derive(Component)]
struct BoardRoot;

#[derive(Component)]
struct BoardList;

#[derive(Component, Clone, PartialEq, Eq)]
enum BoardButton {
    /// Go to this zone.
    Go(String),
    Close,
}

fn spawn_board(mut commands: Commands) {
    commands
        .spawn((
            BoardRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                top: percent(14),
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
                    width: px(520),
                    ..default()
                },
                BackgroundColor(palette::PANEL.with_alpha(0.95)),
                BorderColor::all(palette::PANEL_BORDER),
            ))
            .with_children(|panel| {
                panel.spawn((
                    Text::new("Dungeon board"),
                    font(22.0),
                    TextColor(palette::TEXT),
                ));
                panel.spawn((
                    Text::new(
                        "Pick a place to go. You can also walk to each one's entrance in the world.",
                    ),
                    font(13.0),
                    TextColor(palette::TEXT_DIM),
                ));
                panel.spawn((
                    BoardList,
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(10),
                        ..default()
                    },
                ));
                button(
                    panel,
                    BoardButton::Close,
                    "Close (Esc)",
                    palette::TEXT,
                    AlignSelf::FlexStart,
                );
            });
        });
}

fn button(
    parent: &mut ChildSpawnerCommands,
    kind: BoardButton,
    label: &str,
    color: Color,
    align: AlignSelf,
) {
    parent
        .spawn((
            Button,
            kind,
            Node {
                padding: UiRect::axes(px(12), px(6)),
                border: UiRect::all(px(1)),
                align_self: align,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
            BorderColor::all(palette::PANEL_BORDER),
        ))
        .with_child((Text::new(label), font(14.0), TextColor(color)));
}

/// The authority says we're at the board: show the list.
fn open_board(
    mut received: MessageReader<Received>,
    me: Res<LocalPlayerId>,
    mut board: ResMut<DungeonBoard>,
) {
    for Received(event) in received.read() {
        if let ServerEvent::OpenBoard { player } = event
            && *player == me.0
        {
            board.open = true;
            board.fresh = true;
        }
    }
}

/// Esc, or walking away from the board, closes it.
pub(super) fn close_board(
    keys: Res<ButtonInput<KeyCode>>,
    zones: Res<Zones>,
    player: Option<Single<(&Motion, &Zone), With<LocalPlayer>>>,
    mut board: ResMut<DungeonBoard>,
) {
    if !board.open {
        return;
    }
    let at_board = player.is_some_and(|player| {
        let (motion, zone) = *player;
        zones
            .get(&zone.0)
            .and_then(|level| level.portal_at(motion.0.position))
            .is_some_and(|portal| portal.board)
    });
    if keys.just_pressed(KeyCode::Escape) || !at_board {
        board.open = false;
    }
}

fn press_buttons(
    buttons: Query<(&Interaction, &BoardButton), Changed<Interaction>>,
    mut board: ResMut<DungeonBoard>,
    mut link: ResMut<Link>,
    me: Res<LocalPlayerId>,
) {
    if !board.open {
        return;
    }
    for (interaction, kind) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        match kind {
            BoardButton::Go(zone) => {
                send(
                    &mut link,
                    *me,
                    ClientRequest::EnterFromBoard { zone: zone.clone() },
                );
                board.open = false;
            }
            BoardButton::Close => board.open = false,
        }
    }
}

/// Show or hide the board, filling in the list when it opens.
fn show_board(
    mut commands: Commands,
    zones: Res<Zones>,
    mut board: ResMut<DungeonBoard>,
    mut root: Single<&mut Visibility, With<BoardRoot>>,
    list: Single<Entity, With<BoardList>>,
    mut buttons: Query<(&Interaction, &mut BackgroundColor), With<BoardButton>>,
) {
    let wanted = if board.open {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    if **root != wanted {
        **root = wanted;
    }
    if board.fresh {
        board.fresh = false;
        commands.entity(*list).despawn_children();
        // Dungeons first, then trials, each by name.
        let mut places: Vec<_> = zones
            .0
            .iter()
            .filter_map(|(id, level)| level.listing.as_ref().map(|l| (id, level, l)))
            .collect();
        places.sort_by(|a, b| (&a.2.kind, &a.1.name).cmp(&(&b.2.kind, &b.1.name)));
        commands.entity(*list).with_children(|list| {
            for (id, level, listing) in places {
                let (fewest, most) = listing.players;
                let mut facts = format!("{}   |   for {fewest} to {most} players", listing.kind);
                if let Some(sync) = level.level_sync {
                    facts.push_str(&format!("   |   level {sync}"));
                }
                list.spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(4),
                        padding: UiRect::all(px(10)),
                        border: UiRect::all(px(1)),
                        ..default()
                    },
                    BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.25)),
                    BorderColor::all(palette::PANEL_BORDER.with_alpha(0.4)),
                ))
                .with_children(|entry| {
                    entry.spawn((
                        Text::new(&level.name),
                        font(17.0),
                        TextColor(palette::BANNER),
                    ));
                    entry.spawn((Text::new(facts), font(13.0), TextColor(palette::TEXT_DIM)));
                    entry.spawn((
                        Text::new(&listing.description),
                        font(13.0),
                        TextColor(palette::TEXT),
                    ));
                    button(
                        entry,
                        BoardButton::Go(id.clone()),
                        "Go there",
                        palette::HEAL,
                        AlignSelf::FlexEnd,
                    );
                });
            }
        });
    }
    for (interaction, mut background) in &mut buttons {
        background.0 = match interaction {
            Interaction::Hovered | Interaction::Pressed => Color::srgba(1.0, 1.0, 1.0, 0.08),
            Interaction::None => Color::srgba(0.0, 0.0, 0.0, 0.35),
        };
    }
}
