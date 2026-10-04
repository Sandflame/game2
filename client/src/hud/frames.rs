//! Unit frames (your health, your target's health) and your cast bar.

use bevy::prelude::*;
use shared::combat::{ActionState, Health};
use shared::components::{CharacterName, Faction};
use shared::gamedata::GameData;

use super::{font, game_now, palette, spawn_bar};
use crate::characters::LocalPlayer;
use crate::targeting::CurrentTarget;

pub struct FramesPlugin;

impl Plugin for FramesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_frames).add_systems(
            Update,
            (update_player_frame, update_target_frame, update_cast_bar),
        );
    }
}

/// Which character a frame shows.
#[derive(Component, Clone, Copy, PartialEq, Eq)]
enum FrameOf {
    Player,
    Target,
}

#[derive(Component)]
struct FrameRoot(FrameOf);
#[derive(Component)]
struct FrameName(FrameOf);
#[derive(Component)]
struct FrameFill(FrameOf);
#[derive(Component)]
struct FrameNumbers(FrameOf);

#[derive(Component)]
struct CastBarRoot;
#[derive(Component)]
struct CastBarFill;
#[derive(Component)]
struct CastBarLabel;

const PLAYER_BAR_WIDTH: f32 = 220.0;
const TARGET_BAR_WIDTH: f32 = 320.0;
const CAST_BAR_WIDTH: f32 = 280.0;

fn spawn_frames(mut commands: Commands) {
    spawn_frame(
        &mut commands,
        FrameOf::Player,
        Node {
            position_type: PositionType::Absolute,
            top: px(16),
            left: px(16),
            ..frame_node()
        },
        PLAYER_BAR_WIDTH,
        palette::HEALTH,
    );
    // The target frame is centred along the top of the screen.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: px(16),
            width: percent(100),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|row| {
            let frame = row.spawn_empty().id();
            row.commands().entity(frame).insert((
                FrameRoot(FrameOf::Target),
                frame_node(),
                BackgroundColor(palette::PANEL),
                Visibility::Hidden,
            ));
            frame_contents(
                &mut row.commands(),
                frame,
                FrameOf::Target,
                TARGET_BAR_WIDTH,
                palette::ENEMY_HEALTH,
            );
        });

    // Cast bar, just above the hotbar.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: px(100),
            width: percent(100),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                CastBarRoot,
                Node {
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: px(2),
                    ..default()
                },
                Visibility::Hidden,
            ))
            .with_children(|bar| {
                bar.spawn((
                    CastBarLabel,
                    Text::new(""),
                    font(14.0),
                    TextColor(palette::TEXT),
                ));
                let fill = super::spawn_bar(bar, CAST_BAR_WIDTH, 10.0, palette::CAST);
                bar.commands().entity(fill).insert(CastBarFill);
            });
        });
}

fn frame_node() -> Node {
    Node {
        flex_direction: FlexDirection::Column,
        padding: UiRect::all(px(8)),
        row_gap: px(4),
        ..default()
    }
}

fn spawn_frame(commands: &mut Commands, of: FrameOf, node: Node, width: f32, color: Color) {
    let frame = commands
        .spawn((FrameRoot(of), node, BackgroundColor(palette::PANEL)))
        .id();
    frame_contents(commands, frame, of, width, color);
}

fn frame_contents(commands: &mut Commands, frame: Entity, of: FrameOf, width: f32, color: Color) {
    commands.entity(frame).with_children(|parent| {
        parent.spawn((
            FrameName(of),
            Text::new(""),
            font(15.0),
            TextColor(palette::TEXT),
        ));
        let fill = spawn_bar(parent, width, 12.0, color);
        parent.commands().entity(fill).insert(FrameFill(of));
        parent.spawn((
            FrameNumbers(of),
            Text::new(""),
            font(12.0),
            TextColor(palette::TEXT_DIM),
        ));
    });
}

type FrameTexts<'w, 's> = Query<
    'w,
    's,
    (
        &'static mut Text,
        Option<&'static FrameName>,
        Option<&'static FrameNumbers>,
    ),
    Or<(With<FrameName>, With<FrameNumbers>)>,
>;

fn show_in_frame(
    of: FrameOf,
    name: &str,
    health: &Health,
    texts: &mut FrameTexts,
    fills: &mut Query<(&FrameFill, &mut Node)>,
) {
    for (mut text, name_tag, numbers_tag) in texts.iter_mut() {
        if name_tag.is_some_and(|t| t.0 == of) {
            text.0 = name.to_owned();
        } else if numbers_tag.is_some_and(|t| t.0 == of) {
            text.0 = format!("{} / {}", health.current, health.max);
        }
    }
    for (fill, mut node) in fills.iter_mut() {
        if fill.0 == of {
            node.width = percent(health.fraction() * 100.0);
        }
    }
}

fn update_player_frame(
    player: Option<Single<(&CharacterName, &Health), With<LocalPlayer>>>,
    mut texts: FrameTexts,
    mut fills: Query<(&FrameFill, &mut Node)>,
) {
    if let Some(player) = player {
        let (name, health) = *player;
        show_in_frame(FrameOf::Player, &name.0, health, &mut texts, &mut fills);
    }
}

fn update_target_frame(
    target: Res<CurrentTarget>,
    characters: Query<(&CharacterName, &Health, &Faction)>,
    mut roots: Query<(&FrameRoot, &mut Visibility)>,
    mut texts: FrameTexts,
    mut fills: Query<(&FrameFill, &mut Node)>,
    mut colors: Query<(&FrameFill, &mut BackgroundColor)>,
) {
    let shown = target.0.and_then(|t| characters.get(t).ok());
    for (root, mut visibility) in &mut roots {
        if root.0 == FrameOf::Target {
            *visibility = if shown.is_some() {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
    }
    let Some((name, health, faction)) = shown else {
        return;
    };
    show_in_frame(FrameOf::Target, &name.0, health, &mut texts, &mut fills);
    for (fill, mut color) in &mut colors {
        if fill.0 == FrameOf::Target {
            color.0 = if *faction == Faction::Enemy {
                palette::ENEMY_HEALTH
            } else {
                palette::HEALTH
            };
        }
    }
}

fn update_cast_bar(
    fixed: Res<Time<Fixed>>,
    data: Res<GameData>,
    player: Option<Single<&ActionState, With<LocalPlayer>>>,
    mut root: Single<&mut Visibility, With<CastBarRoot>>,
    mut fill: Single<&mut Node, With<CastBarFill>>,
    mut label: Single<&mut Text, With<CastBarLabel>>,
) {
    let now = game_now(&fixed);
    let cast = player.and_then(|actions| {
        let cast = actions.cast.clone()?;
        Some((cast, actions.cast_progress(now)?))
    });
    let Some((cast, progress)) = cast else {
        **root = Visibility::Hidden;
        return;
    };
    **root = Visibility::Visible;
    fill.width = percent(progress * 100.0);
    let name = data
        .abilities
        .get(&cast.ability)
        .map_or(cast.ability.as_str(), |a| a.name.as_str());
    let remaining = (cast.ends - now).max(0.0);
    label.0 = format!("{name}  {remaining:.1}s");
}
