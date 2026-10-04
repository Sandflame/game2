//! Small animations that make fights read better:
//! - characters flash white when hit,
//! - the Rootwarden sways, leans back while casting, slams forwards when
//!   its attack lands, and sinks into its roots when defeated,
//! - Thornlings hop.

use bevy::prelude::*;
use server::Defeated;
use shared::combat::ActionState;
use shared::components::VisualKey;
use shared::protocol::ServerEvent;

use crate::hud::game_now;
use crate::session::Received;
use crate::toon::ToonMaterial;

pub struct AnimationPlugin;

impl Plugin for AnimationPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                collect_body_materials,
                start_flashes,
                flash,
                animate_bosses,
                hop,
            )
                .chain(),
        );
    }
}

/// How bright a hit flash starts (added to the glow of every body part).
const FLASH_COLOR: LinearRgba = LinearRgba::rgb(0.9, 0.85, 0.8);
/// Damage over time ticks flash more gently.
const TICK_FLASH: f32 = 0.35;
/// How quickly a flash fades (per second).
const FLASH_FADE: f32 = 7.0;

/// A part whose material should never flash (the lantern flame has its
/// own glow animation).
#[derive(Component)]
pub struct NoFlash;

/// Every toon material on a character's body with its normal glow.
#[derive(Component)]
struct BodyMaterials(Vec<(Handle<ToonMaterial>, LinearRgba)>);

/// How much a character is flashing right now (0–1).
#[derive(Component)]
struct HitFlash(f32);

/// A body part that hops up and down (small adds).
#[derive(Component)]
pub struct Hop {
    /// Its height when standing still.
    pub rest: f32,
}

/// The posable parts of a big boss model.
#[derive(Component)]
pub struct BossRig {
    /// Everything above the roots; it bends at the base.
    torso: Entity,
    /// Shoulders the arms hang from.
    arms: [Entity; 2],
    /// 1 right after an attack lands, falling to 0.
    slam: f32,
    /// How far it has sunk into the ground after defeat (0–1).
    sunk: f32,
}

impl BossRig {
    pub fn new(torso: Entity, arms: [Entity; 2]) -> Self {
        Self {
            torso,
            arms,
            slam: 0.0,
            sunk: 0.0,
        }
    }
}

/// Bodies are built a frame after the character appears; once they have
/// parts, remember their materials.
fn collect_body_materials(
    mut commands: Commands,
    new: Query<Entity, (With<VisualKey>, With<Children>, Without<BodyMaterials>)>,
    children: Query<&Children>,
    parts: Query<&MeshMaterial3d<ToonMaterial>, Without<NoFlash>>,
    materials: Res<Assets<ToonMaterial>>,
) {
    for character in &new {
        let mut found: Vec<(Handle<ToonMaterial>, LinearRgba)> = Vec::new();
        for part in children.iter_descendants(character) {
            let Ok(material) = parts.get(part) else {
                continue;
            };
            if found.iter().any(|(h, _)| h == &material.0) {
                continue;
            }
            if let Some(m) = materials.get(&material.0) {
                found.push((material.0.clone(), m.base.emissive));
            }
        }
        if !found.is_empty() {
            commands.entity(character).insert(BodyMaterials(found));
        }
    }
}

fn start_flashes(
    mut commands: Commands,
    mut received: MessageReader<Received>,
    mut flashing: Query<&mut HitFlash>,
    bodies: Query<(), With<BodyMaterials>>,
) {
    for Received(event) in received.read() {
        let ServerEvent::Damage { target, tick, .. } = event else {
            continue;
        };
        let strength = if *tick { TICK_FLASH } else { 1.0 };
        if let Ok(mut flash) = flashing.get_mut(*target) {
            flash.0 = flash.0.max(strength);
        } else if bodies.contains(*target) {
            commands.entity(*target).insert(HitFlash(strength));
        }
    }
}

fn flash(
    mut commands: Commands,
    time: Res<Time>,
    mut flashing: Query<(Entity, &mut HitFlash, &BodyMaterials)>,
    mut materials: ResMut<Assets<ToonMaterial>>,
) {
    for (entity, mut flash, body) in &mut flashing {
        flash.0 = (flash.0 - time.delta_secs() * FLASH_FADE).max(0.0);
        let amount = flash.0 * flash.0;
        for (handle, glow) in &body.0 {
            if let Some(mut material) = materials.get_mut(handle) {
                material.base.emissive = *glow + FLASH_COLOR * amount;
            }
        }
        if flash.0 <= 0.0 {
            commands.entity(entity).remove::<HitFlash>();
        }
    }
}

/// How far the torso leans back at the end of a cast (radians).
const WIND_UP_LEAN: f32 = 0.2;
/// How far the arms lift at the end of a cast (radians).
const WIND_UP_ARMS: f32 = 0.7;
/// How far the torso pitches forwards in a slam (radians).
const SLAM_LEAN: f32 = 0.3;
/// How quickly a slam recovers (per second).
const SLAM_RECOVERY: f32 = 2.5;
/// How deep a defeated boss sinks (metres) and how fast (per second).
const SINK_DEPTH: f32 = 3.5;
const SINK_SPEED: f32 = 0.4;

fn animate_bosses(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    mut received: MessageReader<Received>,
    mut bosses: Query<(Entity, &mut BossRig, &ActionState, Has<Defeated>)>,
    mut parts: Query<&mut Transform>,
) {
    for Received(event) in received.read() {
        if let ServerEvent::AbilityLanded { user, .. } = event
            && let Ok((_, mut rig, _, _)) = bosses.get_mut(*user)
        {
            rig.slam = 1.0;
        }
    }
    let now = game_now(&fixed);
    let t = time.elapsed_secs();
    let dt = time.delta_secs();
    for (_, mut rig, actions, defeated) in &mut bosses {
        rig.slam = (rig.slam - dt * SLAM_RECOVERY).max(0.0);
        rig.sunk = if defeated {
            (rig.sunk + dt * SINK_SPEED).min(1.0)
        } else {
            0.0
        };
        let casting = smooth(actions.cast_progress(now).unwrap_or(0.0));
        // A quick snap forwards that eases back.
        let slam = rig.slam * rig.slam;

        if let Ok(mut torso) = parts.get_mut(rig.torso) {
            let sway = Quat::from_rotation_z(0.035 * (t * 0.8).sin())
                * Quat::from_rotation_x(0.02 * (t * 0.6).sin());
            let lean = WIND_UP_LEAN * casting - SLAM_LEAN * slam;
            let breathe = 1.0 + 0.015 * (t * 1.6).sin();
            torso.rotation = sway * Quat::from_rotation_x(lean + 0.25 * rig.sunk);
            torso.scale = Vec3::new(1.0, breathe, 1.0);
            torso.translation.y = -SINK_DEPTH * smooth(rig.sunk);
        }
        for (shoulder, side) in rig.arms.into_iter().zip([-1.0, 1.0]) {
            if let Ok(mut arm) = parts.get_mut(shoulder) {
                let swing = 0.08 * (t * 0.9 + side).sin();
                let lift = WIND_UP_ARMS * casting - 0.5 * slam;
                arm.rotation = Quat::from_rotation_z(-side * (lift + swing))
                    * Quat::from_rotation_x(-0.4 * slam);
            }
        }
    }
}

fn hop(time: Res<Time>, mut hoppers: Query<(Entity, &Hop, &mut Transform)>) {
    let t = time.elapsed_secs();
    for (entity, hop, mut transform) in &mut hoppers {
        // Each one hops slightly out of step with the others.
        let phase = (entity.index_u32() % 7) as f32;
        transform.translation.y = hop.rest + 0.12 * (t * 5.0 + phase).sin().abs();
    }
}

/// Ease in and out (0 → 1).
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
