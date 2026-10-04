//! Maps: a minimap in the top-right corner that follows you, and a big map
//! of the whole zone (M). Both are drawn from zone data, seen from above
//! with north (-Z) at the top: walls, buildings, trees and rocks, doorways
//! (gold when a quest wants you to go through them), people (gold when
//! they have a quest for you), enemies, and an arrow for you.

use std::collections::{HashMap, HashSet};

use bevy::asset::RenderAssetUsages;
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::ui::UiTransform;
use server::Defeated;
use shared::components::{Faction, Motion, NpcId, Zone};
use shared::gamedata::{GameData, Zones};
use shared::level::{Level, Shape, base_zone};
use shared::quests::{Goal, QuestLog, current_goal};

use super::{font, palette, text_shadow};
use crate::characters::LocalPlayer;
use crate::world::{CurrentZone, ground_color};

pub struct MapPlugin;

impl Plugin for MapPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<WorldMap>()
            .add_systems(Startup, (make_arrow, spawn_maps).chain())
            .add_systems(
                Update,
                (toggle_map, draw_zone, place_layers, place_dots, show_map).chain(),
            );
    }
}

/// Whether the big map is open.
#[derive(Resource, Default)]
pub struct WorldMap {
    pub open: bool,
}

/// The minimap's size on screen (pixels) and how many pixels a metre is.
const MINIMAP_SIZE: f32 = 200.0;
const MINIMAP_SCALE: f32 = 2.2;
/// The big map's size on screen (pixels); the zone is scaled to fit.
const MAP_SIZE: f32 = 600.0;
const ARROW_SIZE: f32 = 18.0;
const DOT_SIZE: f32 = 7.0;

/// Colours on the maps.
const ENEMY_DOT: Color = Color::srgb(0.95, 0.30, 0.25);
const PERSON_DOT: Color = Color::srgb(0.45, 0.95, 0.45);
const QUEST_GOLD: Color = Color::srgb(1.0, 0.82, 0.25);
const DOOR: Color = Color::srgb(0.45, 0.85, 1.0);
const CLOSED_DOOR: Color = Color::srgb(0.55, 0.55, 0.60);

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum View {
    Mini,
    Full,
}

impl View {
    fn size(self) -> f32 {
        match self {
            View::Mini => MINIMAP_SIZE,
            View::Full => MAP_SIZE,
        }
    }

    /// Pixels per metre in a zone of this half-size.
    fn scale(self, half_size: f32) -> f32 {
        match self {
            View::Mini => MINIMAP_SCALE,
            View::Full => MAP_SIZE / (half_size * 2.0),
        }
    }
}

/// The drawing of a zone (moved around under the minimap's window).
#[derive(Component)]
struct MapLayer {
    view: View,
    /// The zone drawn, and its scale and half-size.
    zone: Option<String>,
    scale: f32,
    half: f32,
    /// A dot for each character shown, by character.
    dots: HashMap<Entity, Entity>,
    /// Doorway marks, with the zone they lead to (to turn gold for quests).
    doors: Vec<(Entity, Vec<String>)>,
}

/// The arrow showing you (and which way you face).
#[derive(Component)]
struct PlayerArrow(View);

#[derive(Component)]
struct MapRoot;

#[derive(Component)]
struct MapTitle;

#[derive(Resource)]
struct ArrowImage(Handle<Image>);

/// A small white arrow pointing up, drawn into an image once.
fn make_arrow(mut commands: Commands, mut images: ResMut<Assets<Image>>) {
    const N: u32 = 32;
    let mut pixels = vec![0u8; (N * N * 4) as usize];
    for y in 0..N {
        for x in 0..N {
            // A triangle from the top centre to the bottom corners, with a
            // notch cut out of the bottom.
            let u = (x as f32 + 0.5) / N as f32 - 0.5;
            let v = (y as f32 + 0.5) / N as f32;
            let inside = u.abs() < v * 0.5 && v < 0.95 && !(v > 0.7 && u.abs() < (v - 0.7) * 1.6);
            if inside {
                let i = ((y * N + x) * 4) as usize;
                pixels[i..i + 4].copy_from_slice(&[255, 255, 255, 255]);
            }
        }
    }
    let image = Image::new(
        Extent3d {
            width: N,
            height: N,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        pixels,
        TextureFormat::Rgba8UnormSrgb,
        RenderAssetUsages::RENDER_WORLD,
    );
    commands.insert_resource(ArrowImage(images.add(image)));
}

/// N, E, S and W around the edge of a map window.
fn compass(parent: &mut ChildSpawnerCommands, size: f32, text_size: f32) {
    let middle = size / 2.0 - text_size * 0.35;
    for (letter, left, top) in [
        ("N", middle, 2.0),
        ("S", middle, size - text_size - 4.0),
        ("W", 4.0, size / 2.0 - text_size * 0.6),
        (
            "E",
            size - text_size * 0.75 - 4.0,
            size / 2.0 - text_size * 0.6,
        ),
    ] {
        parent.spawn((
            Text::new(letter),
            font(text_size),
            TextColor(if letter == "N" {
                palette::BANNER
            } else {
                palette::TEXT
            }),
            text_shadow(),
            Node {
                position_type: PositionType::Absolute,
                left: px(left),
                top: px(top),
                ..default()
            },
            ZIndex(3),
        ));
    }
}

/// A window onto a zone drawing, with an arrow and the compass.
fn map_window(parent: &mut ChildSpawnerCommands, view: View, arrow: &Handle<Image>) {
    let size = view.size();
    parent
        .spawn((
            Node {
                width: px(size),
                height: px(size),
                border: UiRect::all(px(2)),
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(Color::srgba(0.03, 0.03, 0.06, 0.85)),
            BorderColor::all(palette::PANEL_BORDER),
        ))
        .with_children(|window| {
            window.spawn((
                MapLayer {
                    view,
                    zone: None,
                    scale: 1.0,
                    half: 1.0,
                    dots: HashMap::new(),
                    doors: Vec::new(),
                },
                Node {
                    position_type: PositionType::Absolute,
                    ..default()
                },
            ));
            window.spawn((
                PlayerArrow(view),
                ImageNode::new(arrow.clone()).with_color(palette::COMBO),
                Node {
                    position_type: PositionType::Absolute,
                    width: px(ARROW_SIZE),
                    height: px(ARROW_SIZE),
                    left: px(size / 2.0 - ARROW_SIZE / 2.0),
                    top: px(size / 2.0 - ARROW_SIZE / 2.0),
                    ..default()
                },
                UiTransform::default(),
                ZIndex(2),
            ));
            compass(window, size, if view == View::Mini { 14.0 } else { 20.0 });
        });
}

fn spawn_maps(mut commands: Commands, arrow: Res<ArrowImage>) {
    // The minimap, top right.
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: px(16),
            right: px(16),
            ..default()
        })
        .with_children(|corner| map_window(corner, View::Mini, &arrow.0));
    // The big map, in the middle (hidden until M).
    commands
        .spawn((
            MapRoot,
            Node {
                position_type: PositionType::Absolute,
                width: percent(100),
                height: percent(100),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            Visibility::Hidden,
            GlobalZIndex(9),
        ))
        .with_children(|screen| {
            screen
                .spawn((
                    Node {
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Center,
                        padding: UiRect::all(px(12)),
                        row_gap: px(8),
                        border: UiRect::all(px(2)),
                        ..default()
                    },
                    BackgroundColor(palette::PANEL.with_alpha(0.95)),
                    BorderColor::all(palette::PANEL_BORDER),
                ))
                .with_children(|panel| {
                    panel.spawn((MapTitle, Text::new(""), font(22.0), TextColor(palette::TEXT)));
                    map_window(panel, View::Full, &arrow.0);
                    panel.spawn((
                        Text::new(
                            "You (arrow)   Enemies (red)   People (green)   Doorways (blue)   Quests (gold)   M or Esc to close",
                        ),
                        font(12.0),
                        TextColor(palette::TEXT_DIM),
                    ));
                });
        });
}

/// M opens and closes the big map (Ctrl+M is mute); Esc closes it.
pub(super) fn toggle_map(keys: Res<ButtonInput<KeyCode>>, mut map: ResMut<WorldMap>) {
    let ctrl = keys.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    if keys.just_pressed(KeyCode::KeyM) && !ctrl {
        map.open = !map.open;
    } else if keys.just_pressed(KeyCode::Escape) {
        map.open = false;
    }
}

fn show_map(
    map: Res<WorldMap>,
    current: Res<CurrentZone>,
    zones: Res<Zones>,
    mut root: Single<&mut Visibility, With<MapRoot>>,
    mut title: Single<&mut Text, With<MapTitle>>,
) {
    let wanted = if map.open {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    if **root != wanted {
        **root = wanted;
    }
    let name = current
        .0
        .as_deref()
        .and_then(|z| zones.get(z))
        .map_or(String::new(), |l| l.name.clone());
    if title.0 != name {
        title.0 = name;
    }
}

/// Where a world position is on a layer (pixels from its top-left).
fn on_layer(position: Vec3, half: f32, scale: f32) -> Vec2 {
    Vec2::new(position.x + half, position.z + half) * scale
}

/// Look of solid things on the map.
fn obstacle_color(visual: &str) -> Color {
    match visual {
        "pine" | "tree" | "ancient_tree" => Color::srgb(0.20, 0.42, 0.24),
        "house" => Color::srgb(0.80, 0.70, 0.55),
        "tower" | "fountain" | "standing_stone" => Color::srgb(0.62, 0.62, 0.70),
        "giant_root" => Color::srgb(0.45, 0.32, 0.22),
        "burrow_wall" => Color::srgb(0.16, 0.12, 0.09),
        "rock" => Color::srgb(0.50, 0.50, 0.50),
        _ => Color::srgb(0.40, 0.40, 0.45),
    }
}

/// Redraw each map when the zone changes.
fn draw_zone(
    mut commands: Commands,
    current: Res<CurrentZone>,
    zones: Res<Zones>,
    data: Res<GameData>,
    mut layers: Query<(Entity, &mut MapLayer, &mut Node)>,
) {
    let Some(zone) = &current.0 else {
        return;
    };
    let Some(level) = zones.get(zone) else {
        return;
    };
    for (entity, mut layer, mut node) in &mut layers {
        if layer.zone.as_ref() == Some(zone) {
            continue;
        }
        commands.entity(entity).despawn_children();
        layer.zone = Some(zone.clone());
        layer.dots.clear();
        layer.half = level.half_size;
        layer.scale = layer.view.scale(level.half_size);
        let side = level.half_size * 2.0 * layer.scale;
        node.width = px(side);
        node.height = px(side);
        let ground = if level.ground == "none" {
            Color::srgb(0.08, 0.06, 0.05)
        } else {
            ground_color(&level.ground)
        };
        commands.entity(entity).insert(BackgroundColor(ground));
        layer.doors = draw_level(&mut commands, entity, level, &data, &layer);
    }
}

/// Walls, buildings, trees, doorways and labels. Returns the doorway marks.
fn draw_level(
    commands: &mut Commands,
    layer_entity: Entity,
    level: &Level,
    data: &GameData,
    layer: &MapLayer,
) -> Vec<(Entity, Vec<String>)> {
    let (half, scale) = (layer.half, layer.scale);
    let mut doors = Vec::new();
    commands.entity(layer_entity).with_children(|map| {
        for obstacle in &level.obstacles {
            let (size, round) = match obstacle.shape {
                Shape::Box { half_x, half_z, .. } => (Vec2::new(half_x, half_z) * 2.0, false),
                Shape::Cylinder { radius, .. } => (Vec2::splat(radius * 2.0), true),
            };
            let corner = on_layer(obstacle.position, half, scale) - size * scale / 2.0;
            map.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    left: px(corner.x),
                    top: px(corner.y),
                    width: px((size.x * scale).max(2.0)),
                    height: px((size.y * scale).max(2.0)),
                    border_radius: if round {
                        BorderRadius::MAX
                    } else {
                        BorderRadius::ZERO
                    },
                    ..default()
                },
                BackgroundColor(obstacle_color(&obstacle.visual)),
            ));
        }
        for portal in &level.portals {
            let at = on_layer(portal.position, half, scale);
            let mark = (portal.radius * 2.0 * scale).max(8.0);
            // Where it leads (for quest highlighting).
            let mut leads: Vec<String> = Vec::new();
            if !portal.to.is_empty() {
                leads.push(portal.to.clone());
            }
            if let Some(ride) = portal.ride.as_ref().and_then(|r| data.rides.get(r)) {
                leads.push(ride.to.clone());
            }
            let color = if portal.closed.is_some() {
                CLOSED_DOOR
            } else {
                DOOR
            };
            let door = map
                .spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: px(at.x - mark / 2.0),
                        top: px(at.y - mark / 2.0),
                        width: px(mark),
                        height: px(mark),
                        border: UiRect::all(px(2)),
                        border_radius: BorderRadius::MAX,
                        ..default()
                    },
                    BorderColor::all(color),
                    BackgroundColor(color.with_alpha(0.35)),
                    ZIndex(1),
                ))
                .id();
            doors.push((door, leads));
            if layer.view == View::Full {
                label(
                    map,
                    &portal.label,
                    at + Vec2::new(0.0, mark / 2.0 + 2.0),
                    color,
                );
            }
        }
        if layer.view == View::Full {
            for npc in &level.npcs {
                let at = on_layer(npc.position, half, scale);
                label(map, &npc.name, at + Vec2::new(0.0, DOT_SIZE), PERSON_DOT);
            }
        }
    });
    doors
}

/// Small text centred under a point on the big map.
fn label(map: &mut ChildSpawnerCommands, text: &str, at: Vec2, color: Color) {
    const WIDTH: f32 = 150.0;
    map.spawn((
        Text::new(text),
        font(11.0),
        TextColor(color),
        TextLayout::justify(Justify::Center),
        text_shadow(),
        Node {
            position_type: PositionType::Absolute,
            left: px(at.x - WIDTH / 2.0),
            top: px(at.y),
            width: px(WIDTH),
            ..default()
        },
        ZIndex(2),
    ));
}

/// Keep the minimap centred on you, and the arrows pointing your way.
fn place_layers(
    player: Option<Single<&Motion, With<LocalPlayer>>>,
    mut layers: Query<(&MapLayer, &mut Node)>,
    mut arrows: Query<(&PlayerArrow, &mut Node, &mut UiTransform), Without<MapLayer>>,
) {
    let Some(motion) = player else {
        return;
    };
    let here = motion.0.position;
    let mut full_scale = None;
    for (layer, mut node) in &mut layers {
        let me = on_layer(here, layer.half, layer.scale);
        match layer.view {
            View::Mini => {
                let middle = MINIMAP_SIZE / 2.0;
                node.left = px(middle - me.x);
                node.top = px(middle - me.y);
            }
            View::Full => {
                // The whole zone fits, so it stays put; the arrow moves.
                node.left = px(0.0);
                node.top = px(0.0);
                full_scale = Some(me);
            }
        }
    }
    for (arrow, mut node, mut transform) in &mut arrows {
        // Yaw 0 faces north (up); the UI turns clockwise.
        transform.rotation = Rot2::radians(-motion.0.yaw);
        if arrow.0 == View::Full
            && let Some(me) = full_scale
        {
            node.left = px(me.x - ARROW_SIZE / 2.0);
            node.top = px(me.y - ARROW_SIZE / 2.0);
        }
    }
}

/// Dots for enemies and people; gold for quest people and doorways.
fn place_dots(
    mut commands: Commands,
    data: Res<GameData>,
    zones: Res<Zones>,
    current: Res<CurrentZone>,
    log: Option<Single<&QuestLog, With<LocalPlayer>>>,
    characters: Query<
        (
            Entity,
            &Motion,
            &Zone,
            &Faction,
            Option<&NpcId>,
            Has<Defeated>,
        ),
        Without<LocalPlayer>,
    >,
    mut layers: Query<(Entity, &mut MapLayer)>,
    mut dots: Query<(&mut Node, &mut BackgroundColor)>,
    mut doors: Query<&mut BorderColor>,
) {
    let Some(zone) = &current.0 else {
        return;
    };
    // Zones the player's quests want them to go to.
    let goals: HashSet<String> = log
        .as_ref()
        .map(|log| quest_destinations(&data, &zones, log))
        .unwrap_or_default();
    for (layer_entity, mut layer) in &mut layers {
        let (half, scale) = (layer.half, layer.scale);
        let mut seen = HashSet::new();
        for (entity, motion, char_zone, faction, npc, defeated) in &characters {
            if &char_zone.0 != zone || defeated {
                continue;
            }
            let color = match (faction, npc) {
                (Faction::Enemy, _) => ENEMY_DOT,
                (Faction::Neutral, Some(id)) => {
                    let quest = log
                        .as_ref()
                        .is_some_and(|log| log.marker(&data.quests, &id.0).is_some());
                    if quest { QUEST_GOLD } else { PERSON_DOT }
                }
                (Faction::Neutral, None) => PERSON_DOT,
                (Faction::Player, _) => palette::BUFF,
            };
            seen.insert(entity);
            let at = on_layer(motion.0.position, half, scale) - Vec2::splat(DOT_SIZE / 2.0);
            let dot = *layer.dots.entry(entity).or_insert_with(|| {
                commands
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            width: px(DOT_SIZE),
                            height: px(DOT_SIZE),
                            border_radius: BorderRadius::MAX,
                            ..default()
                        },
                        BackgroundColor(color),
                        ZIndex(1),
                        ChildOf(layer_entity),
                    ))
                    .id()
            });
            if let Ok((mut node, mut background)) = dots.get_mut(dot) {
                node.left = px(at.x);
                node.top = px(at.y);
                background.0 = color;
            }
        }
        layer.dots.retain(|character, dot| {
            let keep = seen.contains(character);
            if !keep && let Ok(mut gone) = commands.get_entity(*dot) {
                gone.despawn();
            }
            keep
        });
        for (door, leads) in &layer.doors {
            if let Ok(mut border) = doors.get_mut(*door) {
                let quest = leads.iter().any(|z| goals.contains(z));
                let wanted = if quest { QUEST_GOLD } else { DOOR };
                // Closed gates keep their grey.
                if !leads.is_empty() {
                    *border = BorderColor::all(wanted);
                }
            }
        }
    }
}

/// Zones that the player's current quest steps send them to: places to
/// reach, and where boss fights to win are.
fn quest_destinations(data: &GameData, zones: &Zones, log: &QuestLog) -> HashSet<String> {
    let mut goals = HashSet::new();
    for (quest, active) in &log.active {
        match current_goal(&data.quests, quest, active) {
            Some(Goal::Reach(zone)) => {
                goals.insert(base_zone(zone).to_owned());
            }
            Some(Goal::Win(fight)) => {
                for (id, level) in &zones.0 {
                    if level.encounters.contains(fight) {
                        goals.insert(id.clone());
                    }
                }
            }
            _ => {}
        }
    }
    goals
}
