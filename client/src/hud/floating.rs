//! Text that follows the 3D world: nameplates above characters and
//! floating damage numbers. Also the message line ("Not ready yet.").

use bevy::prelude::*;
use shared::combat::Health;
use shared::components::{CharacterName, Faction};
use shared::protocol::ServerEvent;

use super::{font, palette, spawn_bar, text_shadow};
use crate::camera::FollowCamera;
use crate::characters::{DisplayMotion, LocalPlayer};
use crate::session::{LocalPlayerId, Received};

/// Nameplates are hidden beyond this distance from the camera.
const NAMEPLATE_RANGE: f32 = 45.0;
/// Height above a character's feet for its nameplate.
const NAMEPLATE_HEIGHT: f32 = 2.4;
const NAMEPLATE_WIDTH: f32 = 160.0;
/// How long damage numbers stay up, in seconds.
const DAMAGE_NUMBER_LIFE: f32 = 1.1;
const MESSAGE_LIFE: f32 = 1.6;

pub struct FloatingPlugin;

impl Plugin for FloatingPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_message_line).add_systems(
            Update,
            (
                spawn_nameplates,
                show_events,
                (place_nameplates, animate_damage_numbers, fade_message),
            )
                .chain(),
        );
    }
}

#[derive(Component)]
struct Nameplate {
    of: Entity,
}
#[derive(Component)]
struct NameplateFill {
    of: Entity,
}

#[derive(Component)]
struct DamageNumber {
    at: Vec3,
    age: f32,
    /// Sideways nudge in pixels so numbers don't stack exactly.
    nudge: f32,
}

#[derive(Component)]
struct MessageLine {
    age: f32,
}

fn spawn_nameplates(
    mut commands: Commands,
    new: Query<(Entity, &CharacterName, &Faction, Has<LocalPlayer>), Added<DisplayMotion>>,
) {
    for (entity, name, faction, is_me) in &new {
        // Your own name and health are on your frame instead.
        if is_me {
            continue;
        }
        commands
            .spawn((
                Nameplate { of: entity },
                Node {
                    position_type: PositionType::Absolute,
                    width: px(NAMEPLATE_WIDTH),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Center,
                    row_gap: px(2),
                    ..default()
                },
                Visibility::Hidden,
                ZIndex(-1),
            ))
            .with_children(|plate| {
                let color = if *faction == Faction::Enemy {
                    Color::srgb(1.0, 0.75, 0.65)
                } else {
                    Color::srgb(0.75, 0.90, 1.0)
                };
                plate.spawn((
                    Text::new(name.0.clone()),
                    font(14.0),
                    TextColor(color),
                    text_shadow(),
                ));
                let fill = spawn_bar(plate, 70.0, 5.0, palette::ENEMY_HEALTH);
                plate
                    .commands()
                    .entity(fill)
                    .insert(NameplateFill { of: entity });
            });
    }
}

/// Where a world position appears on screen, using the camera's
/// up-to-date transform (its global transform is a frame behind here).
fn to_screen(camera: &Camera, camera_transform: &Transform, world: Vec3) -> Option<Vec2> {
    camera
        .world_to_viewport(&GlobalTransform::from(*camera_transform), world)
        .ok()
}

fn place_nameplates(
    mut commands: Commands,
    camera: Single<(&Camera, &Transform), With<FollowCamera>>,
    characters: Query<(&Transform, &Health), Without<FollowCamera>>,
    mut plates: Query<(Entity, &Nameplate, &mut Node, &mut Visibility)>,
    mut fills: Query<(&NameplateFill, &mut Node), Without<Nameplate>>,
) {
    let (camera, camera_transform) = *camera;
    for (plate_entity, plate, mut node, mut visibility) in &mut plates {
        let Ok((transform, _)) = characters.get(plate.of) else {
            commands.entity(plate_entity).despawn();
            continue;
        };
        let head = transform.translation + Vec3::Y * NAMEPLATE_HEIGHT;
        let near = camera_transform.translation.distance(head) <= NAMEPLATE_RANGE;
        match to_screen(camera, camera_transform, head).filter(|_| near) {
            Some(screen) => {
                *visibility = Visibility::Visible;
                node.left = px(screen.x - NAMEPLATE_WIDTH / 2.0);
                node.top = px(screen.y - 24.0);
            }
            None => *visibility = Visibility::Hidden,
        }
    }
    for (fill, mut node) in &mut fills {
        if let Ok((_, health)) = characters.get(fill.of) {
            node.width = percent(health.fraction() * 100.0);
        }
    }
}

fn spawn_message_line(mut commands: Commands) {
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: percent(22),
            width: percent(100),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_child((
            MessageLine { age: MESSAGE_LIFE },
            Text::new(""),
            font(20.0),
            TextColor(palette::WARNING),
            text_shadow(),
        ));
}

fn show_events(
    mut commands: Commands,
    mut received: MessageReader<Received>,
    me: Res<LocalPlayerId>,
    player: Option<Single<Entity, With<LocalPlayer>>>,
    characters: Query<&Transform>,
    mut message: Single<(&mut Text, &mut TextColor, &mut MessageLine)>,
    mut count: Local<u32>,
) {
    let my_entity = player.map(|p| *p);
    for Received(event) in received.read() {
        match event {
            ServerEvent::Damage { target, amount, .. } => {
                let Ok(transform) = characters.get(*target) else {
                    continue;
                };
                *count = count.wrapping_add(1);
                let nudge = [0.0, 18.0, -18.0, 9.0, -9.0][(*count % 5) as usize];
                let color = if Some(*target) == my_entity {
                    palette::WARNING
                } else {
                    Color::srgb(1.0, 0.95, 0.75)
                };
                commands.spawn((
                    DamageNumber {
                        at: transform.translation + Vec3::Y * 1.8,
                        age: 0.0,
                        nudge,
                    },
                    Text::new(amount.to_string()),
                    font(26.0),
                    TextColor(color),
                    text_shadow(),
                    Node {
                        position_type: PositionType::Absolute,
                        ..default()
                    },
                    Visibility::Hidden,
                ));
            }
            ServerEvent::Rejected { player, reason } if *player == me.0 => {
                let (text, color, line) = &mut *message;
                text.0 = reason.message().to_owned();
                color.0 = palette::WARNING;
                line.age = 0.0;
            }
            ServerEvent::CastInterrupted { user, .. } if Some(*user) == my_entity => {
                let (text, color, line) = &mut *message;
                text.0 = "Interrupted!".to_owned();
                color.0 = palette::CAST;
                line.age = 0.0;
            }
            _ => {}
        }
    }
}

fn animate_damage_numbers(
    time: Res<Time>,
    mut commands: Commands,
    camera: Single<(&Camera, &Transform), With<FollowCamera>>,
    mut numbers: Query<(
        Entity,
        &mut DamageNumber,
        &mut Node,
        &mut TextColor,
        &mut Visibility,
    )>,
) {
    let (camera, camera_transform) = *camera;
    for (entity, mut number, mut node, mut color, mut visibility) in &mut numbers {
        number.age += time.delta_secs();
        if number.age >= DAMAGE_NUMBER_LIFE {
            commands.entity(entity).despawn();
            continue;
        }
        let rise = Vec3::Y * number.age * 1.2;
        match to_screen(camera, camera_transform, number.at + rise) {
            Some(screen) => {
                *visibility = Visibility::Visible;
                node.left = px(screen.x - 20.0 + number.nudge);
                node.top = px(screen.y);
            }
            None => *visibility = Visibility::Hidden,
        }
        let fade = 1.0 - (number.age / DAMAGE_NUMBER_LIFE).powi(3);
        color.0.set_alpha(fade);
    }
}

fn fade_message(time: Res<Time>, mut message: Single<(&mut TextColor, &mut MessageLine)>) {
    let (color, line) = &mut *message;
    line.age += time.delta_secs();
    let alpha = (1.0 - (line.age - MESSAGE_LIFE * 0.6) / (MESSAGE_LIFE * 0.4)).clamp(0.0, 1.0);
    color.0.set_alpha(alpha);
}
