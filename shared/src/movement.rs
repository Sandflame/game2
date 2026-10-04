//! Character movement rules. This is a plain function so that the server
//! and the client can run exactly the same code: the server to decide where
//! everyone really is, the client to predict its own character instantly.

use bevy::math::{Vec2, Vec3};
use serde::Deserialize;

use crate::data::{Problems, Validate};
use crate::level::Level;

/// Tunable movement numbers (`assets/data/config/movement.ron`).
#[derive(Debug, Clone, Deserialize)]
pub struct MovementConfig {
    /// Metres per second.
    pub walk_speed: f32,
    /// How high a jump goes, in metres.
    pub jump_height: f32,
    /// Downward acceleration, metres per second squared.
    pub gravity: f32,
    /// Fastest falling speed, metres per second.
    pub max_fall_speed: f32,
    /// Ledges up to this tall are walked up without jumping, in metres.
    pub step_height: f32,
    /// Size of a character's collision cylinder.
    pub character_radius: f32,
    pub character_height: f32,
}

impl MovementConfig {
    /// Upward speed needed to reach `jump_height` against `gravity`.
    pub fn jump_speed(&self) -> f32 {
        (2.0 * self.gravity * self.jump_height).sqrt()
    }
}

impl Validate for MovementConfig {
    fn validate(&self) -> Vec<String> {
        let mut p = Problems::default();
        p.positive("walk_speed", self.walk_speed);
        p.non_negative("jump_height", self.jump_height);
        p.positive("gravity", self.gravity);
        p.positive("max_fall_speed", self.max_fall_speed);
        p.non_negative("step_height", self.step_height);
        p.positive("character_radius", self.character_radius);
        p.positive("character_height", self.character_height);
        p.0
    }
}

/// What the player wants to do this tick.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MoveInput {
    /// Desired direction on the ground in world space (x = world X,
    /// y = world Z). Longer than 1 is treated as 1.
    pub direction: Vec2,
    pub jump: bool,
    /// If set, face this way (radians around the vertical axis) instead of
    /// the direction of travel. Used while the camera is steering.
    pub face_yaw: Option<f32>,
}

/// Where a character is and how it is moving.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MoveState {
    /// Position of the character's feet.
    pub position: Vec3,
    pub vertical_speed: f32,
    pub grounded: bool,
    /// Facing direction in radians around the vertical axis.
    /// 0 faces -Z (Bevy's "forward").
    pub yaw: f32,
}

impl MoveState {
    pub fn spawn_at(position: Vec3) -> Self {
        Self {
            position,
            grounded: true,
            ..Default::default()
        }
    }
}

/// The yaw angle that faces along a ground direction (see [`MoveState::yaw`]).
pub fn yaw_from_direction(direction: Vec2) -> f32 {
    (-direction.x).atan2(-direction.y)
}

/// Advance one character by `dt` seconds.
pub fn step(
    state: MoveState,
    input: MoveInput,
    config: &MovementConfig,
    level: &Level,
    dt: f32,
) -> MoveState {
    let mut next = state;
    let direction = input.direction.clamp_length_max(1.0);

    // Facing.
    if let Some(yaw) = input.face_yaw {
        next.yaw = yaw;
    } else if direction.length_squared() > 1e-6 {
        next.yaw = yaw_from_direction(direction);
    }

    // Jumping and gravity.
    let jumped = state.grounded && input.jump;
    if jumped {
        next.vertical_speed = config.jump_speed();
        next.grounded = false;
    }
    next.vertical_speed = (next.vertical_speed - config.gravity * dt).max(-config.max_fall_speed);

    // Horizontal move, then push out of walls.
    let feet = state.position.y;
    let horizontal =
        Vec2::new(state.position.x, state.position.z) + direction * config.walk_speed * dt;
    let horizontal = level.resolve_horizontal(
        horizontal,
        config.character_radius,
        feet,
        config.character_height,
        config.step_height,
    );

    // Vertical move and landing.
    let ground = level.ground_height(
        horizontal,
        config.character_radius,
        feet,
        config.step_height,
    );
    let mut y = feet + next.vertical_speed * dt;
    // Stick to the ground when walking down small steps instead of falling.
    let snap_down = state.grounded && !jumped && feet - ground <= config.step_height;
    if y <= ground || snap_down {
        y = ground;
        next.vertical_speed = 0.0;
        next.grounded = true;
    } else {
        next.grounded = false;
    }

    next.position = Vec3::new(horizontal.x, y, horizontal.y);
    next
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::{Obstacle, Shape};

    const DT: f32 = 1.0 / 60.0;

    fn config() -> MovementConfig {
        MovementConfig {
            walk_speed: 6.0,
            jump_height: 1.2,
            gravity: 25.0,
            max_fall_speed: 30.0,
            step_height: 0.4,
            character_radius: 0.4,
            character_height: 1.8,
        }
    }

    fn flat() -> Level {
        Level {
            name: "flat".into(),
            spawns: vec![],
            half_size: 100.0,
            spawn_point: Vec3::ZERO,
            obstacles: vec![],
            ..Level::empty()
        }
    }

    fn with(obstacles: Vec<Obstacle>) -> Level {
        Level {
            obstacles,
            ..flat()
        }
    }

    fn block(x: f32, z: f32, half: f32, height: f32) -> Obstacle {
        Obstacle {
            shape: Shape::Box {
                half_x: half,
                half_z: half,
                height,
            },
            position: Vec3::new(x, 0.0, z),
            visual: String::new(),
        }
    }

    fn run(mut state: MoveState, input: MoveInput, level: &Level, ticks: usize) -> MoveState {
        for _ in 0..ticks {
            state = step(state, input, &config(), level, DT);
        }
        state
    }

    fn forward() -> MoveInput {
        MoveInput {
            direction: Vec2::new(0.0, -1.0),
            ..Default::default()
        }
    }

    #[test]
    fn standing_still_stays_put() {
        let start = MoveState::spawn_at(Vec3::ZERO);
        let end = run(start, MoveInput::default(), &flat(), 60);
        assert_eq!(end.position, Vec3::ZERO);
        assert!(end.grounded);
    }

    #[test]
    fn walks_at_walk_speed() {
        let end = run(MoveState::spawn_at(Vec3::ZERO), forward(), &flat(), 60);
        assert!((end.position.z + 6.0).abs() < 1e-3, "{}", end.position);
        assert!(end.grounded);
    }

    #[test]
    fn diagonal_is_not_faster() {
        let input = MoveInput {
            direction: Vec2::new(1.0, 1.0),
            ..Default::default()
        };
        let end = run(MoveState::spawn_at(Vec3::ZERO), input, &flat(), 60);
        let travelled = Vec2::new(end.position.x, end.position.z).length();
        assert!((travelled - 6.0).abs() < 1e-3, "{travelled}");
    }

    #[test]
    fn faces_direction_of_travel() {
        let right = MoveInput {
            direction: Vec2::X,
            ..Default::default()
        };
        let end = run(MoveState::spawn_at(Vec3::ZERO), right, &flat(), 1);
        // Rotating Bevy's forward (-Z) by the yaw should point along +X.
        let facing = bevy::math::Quat::from_rotation_y(end.yaw) * Vec3::NEG_Z;
        assert!((facing - Vec3::X).length() < 1e-5, "{facing}");
    }

    #[test]
    fn camera_steering_overrides_facing() {
        let input = MoveInput {
            face_yaw: Some(1.0),
            ..forward()
        };
        let end = run(MoveState::spawn_at(Vec3::ZERO), input, &flat(), 1);
        assert_eq!(end.yaw, 1.0);
    }

    #[test]
    fn jump_reaches_jump_height_and_lands() {
        let mut state = MoveState::spawn_at(Vec3::ZERO);
        let jump = MoveInput {
            jump: true,
            ..Default::default()
        };
        state = step(state, jump, &config(), &flat(), DT);
        assert!(!state.grounded);
        let mut peak: f32 = 0.0;
        for _ in 0..120 {
            state = step(state, MoveInput::default(), &config(), &flat(), DT);
            peak = peak.max(state.position.y);
        }
        assert!((peak - 1.2).abs() < 0.1, "peak {peak}");
        assert!(state.grounded);
        assert_eq!(state.position.y, 0.0);
    }

    #[test]
    fn cannot_jump_in_mid_air() {
        let mut state = MoveState::spawn_at(Vec3::ZERO);
        let jump = MoveInput {
            jump: true,
            ..Default::default()
        };
        state = run(state, jump, &flat(), 10);
        let speed_before = state.vertical_speed;
        state = step(state, jump, &config(), &flat(), DT);
        assert!(state.vertical_speed < speed_before);
    }

    #[test]
    fn walls_block_walking() {
        // Wall face at z = -2.
        let level = with(vec![block(0.0, -3.0, 1.0, 3.0)]);
        let end = run(MoveState::spawn_at(Vec3::ZERO), forward(), &level, 120);
        assert!(
            (end.position.z - (-2.0 + 0.4)).abs() < 1e-3,
            "{}",
            end.position
        );
    }

    #[test]
    fn walks_up_small_steps() {
        let level = with(vec![block(0.0, -3.0, 1.0, 0.3)]);
        let end = run(MoveState::spawn_at(Vec3::ZERO), forward(), &level, 30);
        assert_eq!(end.position.y, 0.3);
        assert!(end.grounded);
    }

    #[test]
    fn walks_down_small_steps_without_falling() {
        let level = with(vec![block(0.0, 0.0, 1.0, 0.3)]);
        let mut state = MoveState::spawn_at(Vec3::new(0.0, 0.3, 0.0));
        state = run(state, forward(), &level, 30);
        assert_eq!(state.position.y, 0.0);
        assert!(state.grounded);
    }

    #[test]
    fn can_jump_onto_a_crate() {
        let level = with(vec![block(0.0, -2.0, 1.0, 1.0)]);
        let mut state = MoveState::spawn_at(Vec3::ZERO);
        state = step(
            state,
            MoveInput {
                jump: true,
                ..forward()
            },
            &config(),
            &level,
            DT,
        );
        state = run(state, forward(), &level, 25);
        state = run(state, MoveInput::default(), &level, 60);
        assert!(state.grounded);
        assert_eq!(state.position.y, 1.0, "{}", state.position);
    }

    #[test]
    fn falls_off_tall_ledges() {
        let level = with(vec![block(0.0, 0.0, 1.0, 2.0)]);
        let mut state = MoveState::spawn_at(Vec3::new(0.0, 2.0, 0.0));
        state = run(state, forward(), &level, 15);
        assert!(!state.grounded, "{state:?}");
        state = run(state, MoveInput::default(), &level, 120);
        assert!(state.grounded);
        assert_eq!(state.position.y, 0.0);
    }

    #[test]
    fn fall_speed_is_capped() {
        let mut state = MoveState::spawn_at(Vec3::new(0.0, 1000.0, 0.0));
        state.grounded = false;
        state = run(state, MoveInput::default(), &flat(), 600);
        assert!(state.vertical_speed >= -30.0);
    }

    #[test]
    fn config_validation() {
        assert!(config().validate().is_empty());
        let bad = MovementConfig {
            walk_speed: -1.0,
            gravity: 0.0,
            ..config()
        };
        assert_eq!(bad.validate().len(), 2);
    }
}
