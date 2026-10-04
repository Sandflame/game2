//! Plays through the rules half without any window: a fake client sends
//! requests through the `Link` and checks the events that come back.
//! Uses its own small data set so rebalancing the real data files never
//! breaks these tests.

use std::collections::HashMap;
use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use server::{AuthorityPlugin, EnemyKind};
use shared::combat::{AbilityDef, CombatConfig, Health, Reject, TargetKind};
use shared::components::{Motion, PlayerId};
use shared::config::{GameConfig, SimulationConfig};
use shared::gamedata::{EnemyDef, GameData, PlayerConfig};
use shared::level::{EnemySpawn, Level};
use shared::movement::{MoveInput, MovementConfig};
use shared::protocol::{ClientRequest, Link, ServerEvent};

const TICK: f64 = 1.0 / 60.0;
const ME: PlayerId = PlayerId(1);

fn ability(
    id: &str,
    on_gcd: bool,
    cast_time: f32,
    cooldown: f32,
    range: f32,
    potency: u32,
) -> AbilityDef {
    AbilityDef {
        id: id.into(),
        name: id.into(),
        description: String::new(),
        on_gcd,
        cast_time,
        cooldown,
        range,
        target: TargetKind::Enemy,
        potency,
        vfx: String::new(),
    }
}

fn test_data() -> GameData {
    let abilities = [
        ability("strike", true, 0.0, 0.0, 3.0, 100),
        ability("bolt", true, 2.0, 0.0, 25.0, 300),
        ability("burst", false, 0.0, 15.0, 25.0, 50),
    ];
    GameData {
        config: GameConfig {
            simulation: SimulationConfig { tick_hz: 60.0 },
            movement: MovementConfig {
                walk_speed: 6.0,
                jump_height: 1.0,
                gravity: 25.0,
                max_fall_speed: 30.0,
                step_height: 0.4,
                character_radius: 0.4,
                character_height: 1.8,
            },
            combat: CombatConfig {
                gcd: 1.5,
                animation_lock: 0.6,
                cast_lock: 0.1,
                queue_window: 0.5,
                damage_per_potency: 1.0,
                tab_target_range: 40.0,
            },
        },
        player: PlayerConfig {
            max_health: 1000,
            hit_radius: 0.5,
            hotbar: vec!["strike".into(), "bolt".into(), "burst".into()],
        },
        abilities: abilities.into_iter().map(|a| (a.id.clone(), a)).collect(),
        enemies: HashMap::from([(
            "dummy".to_owned(),
            EnemyDef {
                name: "Dummy".into(),
                max_health: 5000,
                hit_radius: 1.0,
                visual: "training_dummy".into(),
                reset_after: Some(5.0),
            },
        )]),
    }
}

fn test_level() -> Level {
    Level {
        name: "test".into(),
        half_size: 100.0,
        spawn_point: Vec3::ZERO,
        obstacles: vec![],
        spawns: vec![EnemySpawn {
            enemy: "dummy".into(),
            position: Vec3::new(0.0, 0.0, -3.0),
            yaw: 0.0,
        }],
    }
}

struct Game {
    app: App,
    events: Vec<ServerEvent>,
}

impl Game {
    /// A world with one dummy 3 m in front of the player, who has joined.
    fn new() -> Self {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                TICK,
            )))
            .insert_resource(Time::<Fixed>::from_seconds(TICK))
            .insert_resource(test_data())
            .insert_resource(test_level())
            .add_plugins(AuthorityPlugin);
        let mut game = Self {
            app,
            events: vec![],
        };
        game.send(ClientRequest::Join {
            name: "Tester".into(),
        });
        game.run(0.1);
        assert!(
            game.events
                .iter()
                .any(|e| matches!(e, ServerEvent::Joined { player, .. } if *player == ME)),
            "{:?}",
            game.events
        );
        game.events.clear();
        game
    }

    fn send(&mut self, request: ClientRequest) {
        self.app
            .world_mut()
            .resource_mut::<Link>()
            .to_authority
            .push((ME, request));
    }

    /// Advance the game by roughly `seconds`, collecting events.
    fn run(&mut self, seconds: f64) {
        let ticks = (seconds / TICK).round().max(1.0) as usize;
        for _ in 0..ticks {
            self.app.update();
            let mut link = self.app.world_mut().resource_mut::<Link>();
            self.events.append(&mut link.to_client);
        }
    }

    fn dummy(&mut self) -> Entity {
        let world = self.app.world_mut();
        world
            .query_filtered::<Entity, With<EnemyKind>>()
            .single(world)
            .unwrap()
    }

    fn dummy_health(&mut self) -> u32 {
        let dummy = self.dummy();
        self.app.world().get::<Health>(dummy).unwrap().current
    }

    fn me(&mut self) -> Entity {
        let world = self.app.world_mut();
        world
            .query_filtered::<Entity, With<PlayerId>>()
            .single(world)
            .unwrap()
    }

    fn use_slot(&mut self, slot: usize) {
        let target = Some(self.dummy());
        self.send(ClientRequest::UseAbility { slot, target });
    }

    fn damage_events(&self) -> Vec<u32> {
        self.events
            .iter()
            .filter_map(|e| match e {
                ServerEvent::Damage { amount, .. } => Some(*amount),
                _ => None,
            })
            .collect()
    }

    fn rejections(&self) -> Vec<Reject> {
        self.events
            .iter()
            .filter_map(|e| match e {
                ServerEvent::Rejected { reason, .. } => Some(*reason),
                _ => None,
            })
            .collect()
    }
}

#[test]
fn instant_ability_damages_the_dummy() {
    let mut game = Game::new();
    game.use_slot(0);
    game.run(0.1);
    assert_eq!(game.damage_events(), vec![100]);
    assert_eq!(game.dummy_health(), 4900);
}

#[test]
fn pressing_again_too_early_is_rejected() {
    let mut game = Game::new();
    game.use_slot(0);
    game.run(0.1);
    game.use_slot(0);
    game.run(0.1);
    assert_eq!(game.rejections(), vec![Reject::NotReady]);
    assert_eq!(game.damage_events().len(), 1);
}

#[test]
fn pressing_near_the_end_of_the_gcd_queues_the_ability() {
    let mut game = Game::new();
    game.use_slot(0);
    game.run(1.2);
    game.use_slot(0);
    game.run(0.1);
    assert!(
        game.events
            .iter()
            .any(|e| matches!(e, ServerEvent::Queued { .. }))
    );
    assert_eq!(game.damage_events().len(), 1, "not yet");
    game.run(0.5);
    assert_eq!(
        game.damage_events().len(),
        2,
        "queued strike fired when the GCD came back"
    );
}

#[test]
fn off_gcd_ability_can_be_weaved() {
    let mut game = Game::new();
    game.use_slot(0);
    game.run(0.7); // past the animation lock, inside the GCD
    game.use_slot(2);
    game.run(0.1);
    assert_eq!(game.damage_events(), vec![100, 50]);
}

#[test]
fn out_of_range_is_rejected() {
    let mut game = Game::new();
    // Walk away from the dummy for a second.
    game.send(ClientRequest::Move(MoveInput {
        direction: Vec2::new(0.0, 1.0),
        ..Default::default()
    }));
    game.run(1.0);
    game.send(ClientRequest::Move(MoveInput::default()));
    game.use_slot(0);
    game.run(0.1);
    assert_eq!(game.rejections(), vec![Reject::OutOfRange]);
}

#[test]
fn no_target_is_rejected() {
    let mut game = Game::new();
    game.send(ClientRequest::UseAbility {
        slot: 0,
        target: None,
    });
    game.run(0.1);
    assert_eq!(game.rejections(), vec![Reject::NoTarget]);
}

#[test]
fn cannot_target_yourself_with_an_attack() {
    let mut game = Game::new();
    let me = Some(game.me());
    game.send(ClientRequest::UseAbility {
        slot: 0,
        target: me,
    });
    game.run(0.1);
    assert_eq!(game.rejections(), vec![Reject::InvalidTarget]);
}

#[test]
fn empty_slot_is_rejected() {
    let mut game = Game::new();
    game.use_slot(7);
    game.run(0.1);
    assert_eq!(game.rejections(), vec![Reject::NotOnHotbar]);
}

#[test]
fn cast_lands_when_it_finishes() {
    let mut game = Game::new();
    game.use_slot(1);
    game.run(1.5);
    assert!(game.damage_events().is_empty(), "still casting");
    game.run(0.6);
    assert_eq!(game.damage_events(), vec![300]);
}

#[test]
fn moving_interrupts_a_cast() {
    let mut game = Game::new();
    game.use_slot(1);
    game.run(0.5);
    game.send(ClientRequest::Move(MoveInput {
        direction: Vec2::new(1.0, 0.0),
        ..Default::default()
    }));
    game.run(0.1);
    game.send(ClientRequest::Move(MoveInput::default()));
    game.run(2.0);
    assert!(
        game.events
            .iter()
            .any(|e| matches!(e, ServerEvent::CastInterrupted { .. }))
    );
    assert!(game.damage_events().is_empty());
}

#[test]
fn using_an_ability_turns_you_to_face_the_target() {
    let mut game = Game::new();
    // Face away from the dummy first.
    game.send(ClientRequest::Move(MoveInput {
        face_yaw: Some(std::f32::consts::PI),
        ..Default::default()
    }));
    game.run(0.1);
    game.send(ClientRequest::Move(MoveInput::default()));
    game.use_slot(0);
    game.run(0.1);
    let me = game.me();
    let yaw = game.app.world().get::<Motion>(me).unwrap().0.yaw;
    assert!(
        yaw.abs() < 1e-3,
        "dummy is straight ahead (-Z), yaw was {yaw}"
    );
}

#[test]
fn dummy_heals_when_left_alone() {
    let mut game = Game::new();
    game.use_slot(0);
    game.run(4.0);
    assert_eq!(game.dummy_health(), 4900);
    game.run(1.5);
    assert_eq!(game.dummy_health(), 5000);
}
