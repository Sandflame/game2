//! Unit frames (you and your target): name, class, health, shield, and
//! buffs/debuffs. Also your cast bar (spells and flame changes).

use bevy::prelude::*;
use server::{Defeated, FlameChange};
use shared::classes::CurrentClass;
use shared::combat::{ActionState, Health};
use shared::components::{CharacterName, Faction};
use shared::gamedata::GameData;
use shared::statuses::{StatusKind, Statuses};

use super::{font, game_now, palette, spawn_bar};
use crate::characters::LocalPlayer;
use crate::targeting::CurrentTarget;

pub struct FramesPlugin;

impl Plugin for FramesPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_frames)
            .add_systems(Update, (update_frames, update_cast_bar));
    }
}

/// Which character a frame shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FrameOf {
    Player,
    Target,
}

/// The parts of one frame, found by entity.
#[derive(Component)]
struct Frame {
    of: FrameOf,
    name: Entity,
    detail: Entity,
    health_fill: Entity,
    shield_fill: Entity,
    numbers: Entity,
    chips: Vec<(Entity, Entity)>,
    /// The target's cast bar (target frame only): row, fill and label.
    cast: Option<(Entity, Entity, Entity)>,
}

#[derive(Component)]
struct CastBarRoot;
#[derive(Component)]
struct CastBarFill;
#[derive(Component)]
struct CastBarLabel;

const PLAYER_BAR_WIDTH: f32 = 240.0;
const TARGET_BAR_WIDTH: f32 = 320.0;
const CAST_BAR_WIDTH: f32 = 280.0;
/// Buff/debuff chips shown per frame.
const CHIPS: usize = 8;

fn spawn_frames(mut commands: Commands) {
    // Your frame, top-left.
    let player_root = commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: px(16),
            left: px(16),
            ..default()
        })
        .id();
    spawn_frame(
        &mut commands,
        player_root,
        FrameOf::Player,
        PLAYER_BAR_WIDTH,
        palette::HEALTH,
    );

    // Target frame, centred along the top.
    let target_root = commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: px(16),
            width: percent(100),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .id();
    spawn_frame(
        &mut commands,
        target_root,
        FrameOf::Target,
        TARGET_BAR_WIDTH,
        palette::ENEMY_HEALTH,
    );

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
                let fill = spawn_bar(bar, CAST_BAR_WIDTH, 10.0, palette::CAST);
                bar.commands().entity(fill).insert(CastBarFill);
            });
        });
}

fn spawn_frame(commands: &mut Commands, parent: Entity, of: FrameOf, width: f32, color: Color) {
    let mut parts = None;
    commands.entity(parent).with_children(|root| {
        let mut panel = root.spawn((
            Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(px(8)),
                row_gap: px(3),
                ..default()
            },
            BackgroundColor(palette::PANEL),
        ));
        if of == FrameOf::Target {
            panel.insert(Visibility::Hidden);
        }
        let frame_entity = panel.id();
        panel.with_children(|frame| {
            let name = frame
                .spawn((Text::new(""), font(15.0), TextColor(palette::TEXT)))
                .id();
            let detail = frame
                .spawn((Text::new(""), font(12.0), TextColor(palette::TEXT_DIM)))
                .id();
            let health_fill = spawn_bar(frame, width, 12.0, color);
            let shield_fill = spawn_bar(frame, width, 4.0, palette::SHIELD);
            let numbers = frame
                .spawn((Text::new(""), font(12.0), TextColor(palette::TEXT_DIM)))
                .id();
            let mut chips = Vec::new();
            frame
                .spawn(Node {
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: px(4),
                    row_gap: px(4),
                    max_width: px(width),
                    ..default()
                })
                .with_children(|row| {
                    for _ in 0..CHIPS {
                        let mut text = Entity::PLACEHOLDER;
                        let chip = row
                            .spawn((
                                Node {
                                    padding: UiRect::axes(px(5), px(2)),
                                    border: UiRect::all(px(1)),
                                    ..default()
                                },
                                BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
                                BorderColor::all(palette::BUFF),
                                Visibility::Hidden,
                            ))
                            .with_children(|chip| {
                                text = chip
                                    .spawn((Text::new(""), font(11.0), TextColor(palette::TEXT)))
                                    .id();
                            })
                            .id();
                        chips.push((chip, text));
                    }
                });
            // Enemies' casts show under the target frame, so you can react.
            let cast = (of == FrameOf::Target).then(|| {
                let mut fill = Entity::PLACEHOLDER;
                let mut label = Entity::PLACEHOLDER;
                let row = frame
                    .spawn((Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(2),
                        margin: UiRect::top(px(4)),
                        display: Display::None,
                        ..default()
                    },))
                    .with_children(|row| {
                        label = row
                            .spawn((Text::new(""), font(14.0), TextColor(palette::ENEMY_CAST)))
                            .id();
                        fill = spawn_bar(row, width, 8.0, palette::ENEMY_CAST);
                    })
                    .id();
                (row, fill, label)
            });
            parts = Some(Frame {
                of,
                name,
                detail,
                health_fill,
                shield_fill,
                numbers,
                chips,
                cast,
            });
        });
        if let Some(parts) = parts.take() {
            root.commands().entity(frame_entity).insert(parts);
        }
    });
}

/// What a frame shows about one character.
struct Shown<'a> {
    /// What the character is casting and how far along (0–1).
    cast: Option<(String, f32)>,
    name: &'a str,
    detail: String,
    health: Health,
    shield: u32,
    statuses: Option<&'a Statuses>,
    enemy: bool,
}

fn update_frames(
    fixed: Res<Time<Fixed>>,
    data: Res<GameData>,
    target: Res<CurrentTarget>,
    player: Option<Single<Entity, With<LocalPlayer>>>,
    characters: Query<(
        &CharacterName,
        &Health,
        &Faction,
        Option<&Statuses>,
        Option<&CurrentClass>,
        Option<&ActionState>,
        Has<Defeated>,
    )>,
    frames: Query<(&Frame, Entity)>,
    mut visibility: Query<&mut Visibility>,
    mut texts: Query<&mut Text>,
    mut text_colors: Query<&mut TextColor>,
    mut nodes: Query<&mut Node>,
    mut colors: Query<&mut BackgroundColor>,
    mut borders: Query<&mut BorderColor>,
    parents: Query<&ChildOf>,
) {
    let now = game_now(&fixed);
    for (frame, frame_entity) in &frames {
        let who = match frame.of {
            FrameOf::Player => player.as_deref().copied(),
            FrameOf::Target => target.0,
        };
        let shown = who.and_then(|e| characters.get(e).ok()).map(
            |(name, health, faction, statuses, class, actions, defeated)| {
                let class_text = class
                    .and_then(|c| {
                        let def = data.classes.get(&c.class)?;
                        let spec = def.spec(&c.spec).map_or("", |s| s.name.as_str());
                        Some(format!("{} - {}  ({})", def.name, spec, def.role.label()))
                    })
                    .unwrap_or_else(|| {
                        if *faction == Faction::Enemy {
                            "Enemy".into()
                        } else {
                            String::new()
                        }
                    });
                let cast = actions.and_then(|a| {
                    let cast = a.cast.as_ref()?;
                    let ability = data
                        .abilities
                        .get(&cast.ability)
                        .map_or(cast.ability.clone(), |a| a.name.clone());
                    Some((ability, a.cast_progress(now)?))
                });
                Shown {
                    cast,
                    name: &name.0,
                    detail: if defeated {
                        "Defeated".to_owned()
                    } else {
                        class_text
                    },
                    health: *health,
                    shield: statuses.map_or(0, Statuses::total_absorb),
                    statuses,
                    enemy: *faction == Faction::Enemy,
                }
            },
        );
        if let Ok(mut vis) = visibility.get_mut(frame_entity) {
            *vis = if shown.is_some() {
                Visibility::Visible
            } else {
                Visibility::Hidden
            };
        }
        let Some(shown) = shown else {
            continue;
        };
        set_text(&mut texts, frame.name, shown.name);
        set_text(&mut texts, frame.detail, &shown.detail);
        let numbers = if shown.shield > 0 {
            format!(
                "{} / {}   +{} shield",
                shown.health.current, shown.health.max, shown.shield
            )
        } else {
            format!("{} / {}", shown.health.current, shown.health.max)
        };
        set_text(&mut texts, frame.numbers, &numbers);
        if let Ok(mut node) = nodes.get_mut(frame.health_fill) {
            node.width = percent(shown.health.fraction() * 100.0);
        }
        if let Ok(mut color) = colors.get_mut(frame.health_fill) {
            color.0 = if shown.enemy {
                palette::ENEMY_HEALTH
            } else {
                palette::HEALTH
            };
        }
        if let Ok(mut node) = nodes.get_mut(frame.shield_fill) {
            let fraction = (shown.shield as f32 / shown.health.max.max(1) as f32).min(1.0);
            node.width = percent(fraction * 100.0);
        }
        // The shield bar only shows while there is a shield.
        if let Ok(bar) = parents.get(frame.shield_fill)
            && let Ok(mut node) = nodes.get_mut(bar.parent())
        {
            node.display = if shown.shield > 0 {
                Display::Flex
            } else {
                Display::None
            };
        }

        if let Some((row, fill, label)) = frame.cast {
            if let Ok(mut node) = nodes.get_mut(row) {
                node.display = if shown.cast.is_some() {
                    Display::Flex
                } else {
                    Display::None
                };
            }
            if let Some((ability, progress)) = &shown.cast {
                set_text(&mut texts, label, ability);
                if let Ok(mut node) = nodes.get_mut(fill) {
                    node.width = percent(progress * 100.0);
                }
            }
        }

        // Buffs first, then debuffs, each soonest-ending first.
        let mut active: Vec<_> = shown
            .statuses
            .map(|s| {
                s.0.iter()
                    .filter_map(|a| Some((a, data.statuses.get(&a.id)?)))
                    .collect()
            })
            .unwrap_or_default();
        active.sort_by(|a, b| {
            (a.1.kind == StatusKind::Debuff)
                .cmp(&(b.1.kind == StatusKind::Debuff))
                .then(a.0.expires.total_cmp(&b.0.expires))
        });
        for (i, (chip, text)) in frame.chips.iter().enumerate() {
            let Ok(mut vis) = visibility.get_mut(*chip) else {
                continue;
            };
            match active.get(i) {
                Some((status, def)) => {
                    *vis = Visibility::Inherited;
                    // Lasting bonuses (party synergy) show no timer.
                    let lasting = status.expires.is_infinite();
                    let name = if status.stacks > 1 {
                        format!("{} x{}", def.name, status.stacks)
                    } else {
                        def.name.clone()
                    };
                    let label = if lasting {
                        name
                    } else if status.is_shield {
                        format!(
                            "{name} {}  {:.0}s",
                            status.absorb,
                            status.remaining(now).ceil()
                        )
                    } else {
                        format!("{name}  {:.0}s", status.remaining(now).ceil())
                    };
                    set_text(&mut texts, *text, &label);
                    let tint = if def.kind == StatusKind::Buff {
                        palette::BUFF
                    } else {
                        palette::DEBUFF
                    };
                    if let Ok(mut border) = borders.get_mut(*chip) {
                        *border = BorderColor::all(tint);
                    }
                    if let Ok(mut color) = text_colors.get_mut(*text) {
                        color.0 = tint.mix(&palette::TEXT, 0.6);
                    }
                }
                None => *vis = Visibility::Hidden,
            }
        }
    }
}

fn set_text(texts: &mut Query<&mut Text>, entity: Entity, value: &str) {
    if let Ok(mut text) = texts.get_mut(entity)
        && text.0 != value
    {
        text.0 = value.to_owned();
    }
}

fn update_cast_bar(
    fixed: Res<Time<Fixed>>,
    data: Res<GameData>,
    player: Option<Single<(&ActionState, Option<&FlameChange>), With<LocalPlayer>>>,
    mut root: Single<&mut Visibility, With<CastBarRoot>>,
    mut fill: Single<&mut Node, With<CastBarFill>>,
    mut label: Single<&mut Text, With<CastBarLabel>>,
) {
    let now = game_now(&fixed);
    let bar = player.and_then(|player| {
        let (actions, flame_change) = *player;
        if let Some(change) = flame_change {
            let total = f64::from(data.config.combat.flame_change_time).max(1e-6);
            let progress = (1.0 - (change.ends - now) / total).clamp(0.0, 1.0) as f32;
            let class = data
                .classes
                .get(&change.class)
                .map_or(change.class.as_str(), |c| c.name.as_str());
            return Some((
                format!("Changing flame: {class}"),
                progress,
                change.ends - now,
            ));
        }
        let cast = actions.cast.as_ref()?;
        let name = data
            .abilities
            .get(&cast.ability)
            .map_or(cast.ability.as_str(), |a| a.name.as_str());
        Some((
            name.to_owned(),
            actions.cast_progress(now)?,
            cast.ends - now,
        ))
    });
    let Some((name, progress, remaining)) = bar else {
        **root = Visibility::Hidden;
        return;
    };
    **root = Visibility::Visible;
    fill.width = percent(progress * 100.0);
    label.0 = format!("{name}  {:.1}s", remaining.max(0.0));
}
