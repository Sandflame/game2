//! Plays through the rules half without any window: a fake client sends
//! requests through the `Link` and checks the events that come back.
//! Uses its own small data set (below) so rebalancing the real data files
//! never breaks these tests.

use std::collections::HashMap;
use std::path::Path;
use std::time::Duration;

use bevy::prelude::*;
use bevy::time::TimeUpdateStrategy;
use server::{AuthorityPlugin, CombatRng, Defeated, EnemyKind};
use shared::classes::{ClassDef, CurrentClass};
use shared::combat::{Health, Reject};
use shared::components::{CharacterName, Hotbar, Motion, PlayerId, Zone};
use shared::config::{GameConfig, SimulationConfig};
use shared::data::{Validate, parse_ron};
use shared::encounters::EncounterDef;
use shared::formulas::Rng;
use shared::gamedata::{AbilityFile, EnemyDef, GameData, PlayerConfig, Zones};
use shared::level::{EnemySpawn, Level, Portal};
use shared::movement::MoveInput;
use shared::protocol::{ClientRequest, Link, ServerEvent};
use shared::statuses::{StatusFile, Statuses};
use shared::telegraphs::Telegraph;

const TICK: f64 = 1.0 / 60.0;
const ME: PlayerId = PlayerId(1);
const FRIEND: PlayerId = PlayerId(2);

const COMBAT: &str = r#"(
    gcd: 1.5, animation_lock: 0.6, cast_lock: 0.1, queue_window: 0.5, combo_window: 15.0,
    crit_chance: 0.0, crit_multiplier: 1.5, tick_interval: 3.0,
    combat_timeout: 6.0, flame_change_time: 2.0, out_of_combat_regen: 0.0, revive_after: 5.0,
    tab_target_range: 40.0, marker_grace: 0.0,
)"#;

const MOVEMENT: &str = r#"(
    walk_speed: 6.0, jump_height: 1.0, gravity: 25.0, max_fall_speed: 30.0,
    step_height: 0.4, character_radius: 0.4, character_height: 1.8,
)"#;

const ABILITIES: &str = r#"[
    (id: "strike", name: "Strike", on_gcd: true, range: 3.0, target: Enemy,
     effects: [(effect: Damage(amount: 100))]),
    (id: "followup", name: "Followup", on_gcd: true, range: 3.0, target: Enemy,
     effects: [(effect: Damage(amount: 50))], combo: Some((after: "strike", amount: 300))),
    (id: "bolt", name: "Bolt", on_gcd: true, cast_time: 2.0, range: 25.0, target: Enemy,
     effects: [(effect: Damage(amount: 300))]),
    (id: "burst", name: "Burst", on_gcd: false, cooldown: 15.0, range: 25.0, target: Enemy,
     effects: [(effect: Damage(amount: 50))]),
    (id: "mend", name: "Mend", on_gcd: true, range: 30.0, target: Ally,
     effects: [(effect: Heal(amount: 300))]),
    (id: "ignite", name: "Ignite", on_gcd: true, range: 25.0, target: Enemy,
     effects: [(effect: ApplyStatus(status: "burn"))]),
    (id: "guard", name: "Guard", on_gcd: false, cooldown: 60.0, target: Myself,
     effects: [(effect: ApplyStatus(status: "guard"))]),
    (id: "barrier", name: "Barrier", on_gcd: false, cooldown: 60.0, target: Myself,
     effects: [(effect: Shield(amount: 250, status: "barrier"))]),
    (id: "provoke", name: "Provoke", on_gcd: false, cooldown: 30.0, range: 20.0, target: Enemy,
     effects: [(effect: Taunt)]),
    (id: "sweep", name: "Sweep", on_gcd: true, target: Myself,
     effects: [(to: EnemiesAround(centre: Me, radius: 5.0), effect: Damage(amount: 10))]),
    (id: "swat", name: "Swat", on_gcd: false, range: 30.0, target: Enemy,
     effects: [(effect: Damage(amount: 100))]),
    (id: "second_wind", name: "Second Wind", on_gcd: false, cooldown: 60.0, target: Myself,
     effects: [(effect: Heal(amount: 300))]),
    (id: "rekindle", name: "Rekindle", on_gcd: true, cast_time: 3.0, cooldown: 60.0, range: 25.0,
     target: DefeatedAlly, effects: [(effect: Raise(health_percent: 30))]),
    (id: "slam", name: "Slam", on_gcd: false, cast_time: 2.0, range: 60.0, target: Enemy,
     telegraph: Some((shape: Circle(radius: 3.0), placement: Target)),
     effects: [(to: InTelegraph, effect: Damage(amount: 300))]),
    (id: "sap", name: "Sap", on_gcd: false, cast_time: 2.0, range: 60.0, target: Enemy,
     telegraph: Some((shape: Circle(radius: 4.0), placement: StackOnTarget)),
     effects: [(to: InTelegraph, effect: SharedDamage(amount: 600))]),
    (id: "seeds", name: "Seeds", on_gcd: false, cast_time: 2.0, range: 60.0, target: Enemy,
     telegraph: Some((shape: Circle(radius: 4.0), placement: SpreadOnEveryone)),
     effects: [(to: InTelegraph, effect: Damage(amount: 200))]),
    (id: "quake", name: "Quake", on_gcd: false, cast_time: 1.0, target: Myself,
     effects: [(to: EnemiesAround(centre: Me, radius: 60.0), effect: Damage(amount: 50))]),
]"#;

const WARDEN: &str = r#"(name: "Warden", max_health: 1000, hit_radius: 1.0, visual: "rootwarden",
    actions: [(ability: "swat", every: 2.0)])"#;

const SAPLING: &str = r#"(name: "Sapling", max_health: 100, hit_radius: 0.5, visual: "thornling")"#;

/// Phase one: slam at 1 s, a stack at 4 s, spreads at 7 s (each a 2 s cast).
/// Phase two (below 50%): a sapling appears, then quakes.
const TRIAL: &str = r#"(
    name: "The Warden", boss: "warden", boss_position: (0.0, 0.0, 0.0),
    extra_health_per_player: 50,
    phases: [
        (name: "One", until_health: 50, loop_after: 12.0, timeline: [
            (at: 1.0, action: Use("slam")),
            (at: 4.0, action: Use("sap")),
            (at: 7.0, action: Use("seeds")),
        ]),
        (name: "Two", until_health: 0, loop_after: 5.0,
         on_start: [Say("Grow!"), Spawn(enemy: "sapling", at: (3.0, 0.0, 0.0))],
         timeline: [(at: 1.0, action: Use("quake"))]),
    ],
)"#;

const STATUSES: &str = r#"[
    (id: "burn", name: "Burn", kind: Debuff, duration: 9.5, tick: Some(Damage(amount: 40))),
    (id: "guard", name: "Guard", kind: Buff, duration: 10.0, modifiers: (damage_taken: 0.5)),
    (id: "barrier", name: "Barrier", kind: Buff, duration: 10.0),
]"#;

const FIGHTER: &str = r#"(
    name: "Fighter", flame: "crimson", role: Damage, max_health: 1000, power: 100,
    threat_multiplier: 1.0,
    core: ["strike", "bolt", "burst", "mend", "followup"],
    specializations: [(id: "main", name: "Main", abilities: ["ignite", "guard", "barrier"])],
    default_spec: "main",
)"#;

const GUARDIAN: &str = r#"(
    name: "Guardian", flame: "azure", role: Durable, max_health: 2000, power: 100,
    threat_multiplier: 5.0,
    core: ["strike", "provoke", "sweep", "guard", "barrier"],
    specializations: [(id: "main", name: "Main", abilities: ["mend", "bolt", "burst"])],
    default_spec: "main",
)"#;

const DUMMY: &str = r#"(name: "Dummy", max_health: 5000, hit_radius: 1.0, visual: "training_dummy",
    reset_after: 5.0)"#;

const HITTER: &str = r#"(name: "Hitter", max_health: 50000, hit_radius: 1.0, visual: "sparring_dummy",
    reset_after: 30.0, actions: [(ability: "swat", every: 2.0)])"#;

fn parse<T: serde::de::DeserializeOwned + Validate>(text: &str) -> T {
    parse_ron(text, Path::new("test data")).unwrap()
}

fn test_data() -> GameData {
    let abilities: AbilityFile = parse(ABILITIES);
    let statuses: StatusFile = parse(STATUSES);
    GameData {
        config: GameConfig {
            simulation: SimulationConfig { tick_hz: 60.0 },
            movement: parse(MOVEMENT),
            combat: parse(COMBAT),
        },
        player: PlayerConfig {
            hit_radius: 0.5,
            start_class: "fighter".into(),
            start_zone: "test".into(),
            shared_abilities: vec!["second_wind".into(), "rekindle".into()],
        },
        abilities: abilities.0.into_iter().map(|a| (a.id.clone(), a)).collect(),
        statuses: statuses.0.into_iter().map(|s| (s.id.clone(), s)).collect(),
        classes: HashMap::from([
            ("fighter".to_owned(), parse::<ClassDef>(FIGHTER)),
            ("guardian".to_owned(), parse::<ClassDef>(GUARDIAN)),
        ]),
        enemies: HashMap::from([
            ("dummy".to_owned(), parse::<EnemyDef>(DUMMY)),
            ("hitter".to_owned(), parse::<EnemyDef>(HITTER)),
            ("warden".to_owned(), parse::<EnemyDef>(WARDEN)),
            ("sapling".to_owned(), parse::<EnemyDef>(SAPLING)),
        ]),
        encounters: HashMap::from([("trial".to_owned(), parse::<EncounterDef>(TRIAL))]),
    }
}

/// Two zones. "test" has the given enemies (players start at the origin)
/// and a portal 6 m behind the start; "arena" holds the trial, with the
/// boss at the origin and the entrance 8 m away.
fn test_zones(enemies: &[(&str, Vec3)]) -> Zones {
    let field = Level {
        name: "test".into(),
        spawns: enemies
            .iter()
            .map(|(enemy, position)| EnemySpawn {
                enemy: (*enemy).into(),
                position: *position,
                yaw: 0.0,
            })
            .collect(),
        portals: vec![Portal {
            position: PORTAL,
            radius: 1.5,
            to: "arena".into(),
            arrive: ARENA_ENTRANCE,
            label: "To the arena".into(),
        }],
        ..Level::empty()
    };
    let arena = Level {
        name: "arena".into(),
        spawn_point: ARENA_ENTRANCE,
        revive_in_place: false,
        encounter: Some("trial".into()),
        portals: vec![Portal {
            position: Vec3::new(0.0, 0.0, 12.0),
            radius: 1.5,
            to: "test".into(),
            arrive: Vec3::ZERO,
            label: "Back".into(),
        }],
        ..Level::empty()
    };
    Zones(HashMap::from([
        ("test".to_owned(), field),
        ("arena".to_owned(), arena),
    ]))
}

const PORTAL: Vec3 = Vec3::new(0.0, 0.0, 6.0);
const ARENA_ENTRANCE: Vec3 = Vec3::new(0.0, 0.0, 8.0);

// Hotbar slots of the "fighter" class.
const STRIKE: usize = 0;
const BOLT: usize = 1;
const BURST: usize = 2;
const MEND: usize = 3;
const FOLLOWUP: usize = 4;
const IGNITE: usize = 5;
const GUARD: usize = 6;
const BARRIER: usize = 7;
// Shared lantern abilities, after the class's own.
const SECOND_WIND: usize = 8;
const REKINDLE: usize = 9;

struct Game {
    app: App,
    events: Vec<ServerEvent>,
}

impl Game {
    /// One training dummy 3 m in front of the player, who has joined.
    fn new() -> Self {
        Self::with_enemies(&[("dummy", Vec3::new(0.0, 0.0, -3.0))])
    }

    fn with_enemies(enemies: &[(&str, Vec3)]) -> Self {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins)
            .insert_resource(TimeUpdateStrategy::ManualDuration(Duration::from_secs_f64(
                TICK,
            )))
            .insert_resource(Time::<Fixed>::from_seconds(TICK))
            .insert_resource(test_data())
            .insert_resource(test_zones(enemies))
            .insert_resource(CombatRng(Rng::new(1)))
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
        self.send_as(ME, request);
    }

    fn send_as(&mut self, player: PlayerId, request: ClientRequest) {
        self.app
            .world_mut()
            .resource_mut::<Link>()
            .to_authority
            .push((player, request));
    }

    /// Bring in a second player; returns their character.
    fn join_friend(&mut self) -> Entity {
        self.send_as(
            FRIEND,
            ClientRequest::Join {
                name: "Friend".into(),
            },
        );
        self.run(0.1);
        self.character(FRIEND)
    }

    fn character(&mut self, player: PlayerId) -> Entity {
        let world = self.app.world_mut();
        world
            .query::<(Entity, &PlayerId)>()
            .iter(world)
            .find(|(_, p)| **p == player)
            .unwrap()
            .0
    }

    /// Walk a player into the portal and through it to the arena.
    fn enter_arena(&mut self, player: PlayerId) {
        let entity = self.character(player);
        self.app
            .world_mut()
            .get_mut::<Motion>(entity)
            .unwrap()
            .0
            .position = PORTAL;
        self.send_as(player, ClientRequest::Interact);
        self.run(0.1);
    }

    fn zone_of(&mut self, entity: Entity) -> String {
        self.app.world().get::<Zone>(entity).unwrap().0.clone()
    }

    fn set_health(&mut self, entity: Entity, current: u32) {
        self.app
            .world_mut()
            .get_mut::<Health>(entity)
            .unwrap()
            .current = current;
    }

    fn damage_from(&self, target: Entity, cause: &str) -> Vec<u32> {
        self.events
            .iter()
            .filter_map(|e| match e {
                ServerEvent::Damage {
                    target: t,
                    amount,
                    cause: c,
                    ..
                } if *t == target && c == cause => Some(*amount),
                _ => None,
            })
            .collect()
    }

    fn markers(&mut self) -> usize {
        let world = self.app.world_mut();
        world.query::<&Telegraph>().iter(world).count()
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

    fn enemy(&mut self, name: &str) -> Entity {
        let world = self.app.world_mut();
        let mut query = world.query_filtered::<(Entity, &CharacterName), With<EnemyKind>>();
        query.iter(world).find(|(_, n)| n.0 == name).unwrap().0
    }

    fn dummy(&mut self) -> Entity {
        self.enemy("Dummy")
    }

    fn health(&mut self, entity: Entity) -> Health {
        *self.app.world().get::<Health>(entity).unwrap()
    }

    fn me(&mut self) -> Entity {
        self.character(ME)
    }

    fn use_on(&mut self, slot: usize, target: Option<Entity>) {
        self.send(ClientRequest::UseAbility { slot, target });
    }

    fn use_slot(&mut self, slot: usize) {
        let target = Some(self.dummy());
        self.use_on(slot, target);
    }

    fn damage_to(&self, target: Entity) -> Vec<u32> {
        self.events
            .iter()
            .filter_map(|e| match e {
                ServerEvent::Damage {
                    target: t, amount, ..
                } if *t == target => Some(*amount),
                _ => None,
            })
            .collect()
    }

    fn dummy_damage(&mut self) -> Vec<u32> {
        let dummy = self.dummy();
        self.damage_to(dummy)
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

    fn walk(&mut self, direction: Vec2, seconds: f64) {
        self.send(ClientRequest::Move(MoveInput {
            direction,
            ..Default::default()
        }));
        self.run(seconds);
        self.send(ClientRequest::Move(MoveInput::default()));
        self.run(TICK);
    }
}

// ---------- Timing (from Milestone 2) ----------

#[test]
fn instant_ability_damages_the_dummy() {
    let mut game = Game::new();
    game.use_slot(STRIKE);
    game.run(0.1);
    assert_eq!(game.dummy_damage(), vec![100]);
    let dummy = game.dummy();
    assert_eq!(game.health(dummy).current, 4900);
}

#[test]
fn pressing_again_too_early_is_rejected() {
    let mut game = Game::new();
    game.use_slot(STRIKE);
    game.run(0.1);
    game.use_slot(STRIKE);
    game.run(0.1);
    assert_eq!(game.rejections(), vec![Reject::NotReady]);
}

#[test]
fn pressing_near_the_end_of_the_gcd_queues_the_ability() {
    let mut game = Game::new();
    game.use_slot(STRIKE);
    game.run(1.2);
    game.use_slot(STRIKE);
    game.run(0.1);
    assert!(
        game.events
            .iter()
            .any(|e| matches!(e, ServerEvent::Queued { .. }))
    );
    assert_eq!(game.dummy_damage().len(), 1, "not yet");
    game.run(0.5);
    assert_eq!(
        game.dummy_damage().len(),
        2,
        "queued strike fired when the GCD came back"
    );
}

#[test]
fn off_gcd_ability_can_be_weaved() {
    let mut game = Game::new();
    game.use_slot(STRIKE);
    game.run(0.7);
    game.use_slot(BURST);
    game.run(0.1);
    assert_eq!(game.dummy_damage(), vec![100, 50]);
}

#[test]
fn out_of_range_is_rejected() {
    let mut game = Game::new();
    game.walk(Vec2::new(0.0, 1.0), 1.0);
    game.use_slot(STRIKE);
    game.run(0.1);
    assert_eq!(game.rejections(), vec![Reject::OutOfRange]);
}

#[test]
fn attacks_need_an_enemy_target() {
    let mut game = Game::new();
    game.use_on(STRIKE, None);
    game.run(0.1);
    let me = Some(game.me());
    game.use_on(STRIKE, me);
    game.run(0.1);
    game.use_slot(10); // past the end of the hotbar
    game.run(0.1);
    assert_eq!(
        game.rejections(),
        vec![Reject::NoTarget, Reject::InvalidTarget, Reject::NotOnHotbar]
    );
}

#[test]
fn cast_lands_when_it_finishes() {
    let mut game = Game::new();
    game.use_slot(BOLT);
    game.run(1.5);
    assert!(game.dummy_damage().is_empty(), "still casting");
    game.run(0.6);
    assert_eq!(game.dummy_damage(), vec![300]);
}

#[test]
fn moving_interrupts_a_cast() {
    let mut game = Game::new();
    game.use_slot(BOLT);
    game.run(0.5);
    game.walk(Vec2::new(1.0, 0.0), 0.1);
    game.run(2.0);
    assert!(
        game.events
            .iter()
            .any(|e| matches!(e, ServerEvent::CastInterrupted { .. }))
    );
    assert!(game.dummy_damage().is_empty());
}

#[test]
fn using_an_ability_turns_you_to_face_the_target() {
    let mut game = Game::new();
    game.send(ClientRequest::Move(MoveInput {
        face_yaw: Some(std::f32::consts::PI),
        ..Default::default()
    }));
    game.run(0.1);
    game.send(ClientRequest::Move(MoveInput::default()));
    game.use_slot(STRIKE);
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
    game.use_slot(STRIKE);
    game.run(4.0);
    let dummy = game.dummy();
    assert_eq!(game.health(dummy).current, 4900);
    game.run(1.5);
    assert_eq!(game.health(dummy).current, 5000);
}

// ---------- Effects and statuses (Milestone 3) ----------

#[test]
fn combo_uses_the_stronger_amount() {
    let mut game = Game::new();
    game.use_slot(FOLLOWUP);
    game.run(1.6);
    game.use_slot(STRIKE);
    game.run(1.6);
    game.use_slot(FOLLOWUP);
    game.run(0.1);
    assert_eq!(game.dummy_damage(), vec![50, 100, 300]);
}

#[test]
fn damage_over_time_ticks_until_it_runs_out() {
    let mut game = Game::new();
    game.use_slot(IGNITE);
    game.run(9.2);
    assert_eq!(
        game.dummy_damage(),
        vec![40, 40, 40],
        "ticks at 3, 6 and 9 seconds"
    );
    game.run(3.0);
    assert_eq!(game.dummy_damage().len(), 3, "expired at 9.5 seconds");
}

#[test]
fn heals_land_on_yourself_without_a_friendly_target_and_never_overheal() {
    let mut game = Game::with_enemies(&[("hitter", Vec3::new(0.0, 0.0, -3.0))]);
    let hitter = game.enemy("Hitter");
    game.use_on(STRIKE, Some(hitter));
    game.run(2.1); // the hitter swats back once (100 damage)
    let me = game.me();
    assert_eq!(game.health(me).current, 900);
    game.use_on(MEND, Some(hitter)); // an enemy target: the heal comes to you
    game.run(0.1);
    assert_eq!(game.health(me).current, 1000, "300 healing, capped at max");
    let healed: Vec<u32> = game
        .events
        .iter()
        .filter_map(|e| match e {
            ServerEvent::Heal { amount, .. } => Some(*amount),
            _ => None,
        })
        .collect();
    assert_eq!(
        healed,
        vec![100],
        "events report the health actually restored"
    );
}

#[test]
fn enemies_attack_whoever_angered_them() {
    let mut game = Game::with_enemies(&[("hitter", Vec3::new(0.0, 0.0, -3.0))]);
    let me = game.me();
    game.run(5.0);
    assert!(game.damage_to(me).is_empty(), "left alone, it does nothing");
    let hitter = game.enemy("Hitter");
    game.use_on(STRIKE, Some(hitter));
    game.run(4.1);
    assert_eq!(game.damage_to(me), vec![100, 100], "a swat every 2 seconds");
}

#[test]
fn damage_reduction_and_shields_protect_you() {
    let mut game = Game::with_enemies(&[("hitter", Vec3::new(0.0, 0.0, -3.0))]);
    let hitter = game.enemy("Hitter");
    game.use_on(STRIKE, Some(hitter));
    game.run(0.7);
    game.use_on(GUARD, None); // half damage taken
    game.run(0.7);
    game.use_on(BARRIER, None); // absorbs 250
    game.run(6.0); // swats at 2, 4 and 6 seconds: 50 each after Guard
    let me = game.me();
    assert_eq!(
        game.damage_to(me),
        vec![0, 0, 0],
        "the barrier soaks them all"
    );
    let shield = game.app.world().get::<Statuses>(me).unwrap().total_absorb();
    assert_eq!(shield, 100);
    assert_eq!(game.health(me).current, 1000);
}

#[test]
fn area_attacks_hit_every_enemy_in_range() {
    let mut game = Game::with_enemies(&[
        ("dummy", Vec3::new(0.0, 0.0, -3.0)),
        ("hitter", Vec3::new(3.0, 0.0, 0.0)),
    ]);
    game.send(ClientRequest::ChangeClass {
        class: "guardian".into(),
    });
    game.run(2.1);
    game.use_on(2, None); // guardian: Sweep
    game.run(0.1);
    let hitter = game.enemy("Hitter");
    assert_eq!(game.dummy_damage(), vec![10]);
    assert_eq!(game.damage_to(hitter), vec![10]);
}

// ---------- Classes ----------

#[test]
fn changing_flame_switches_class_after_a_moment() {
    let mut game = Game::new();
    game.send(ClientRequest::ChangeClass {
        class: "guardian".into(),
    });
    game.run(1.0);
    let me = game.me();
    assert_eq!(
        game.app.world().get::<CurrentClass>(me).unwrap().class,
        "fighter"
    );
    game.run(1.1);
    let world = game.app.world();
    assert_eq!(world.get::<CurrentClass>(me).unwrap().class, "guardian");
    assert_eq!(world.get::<Health>(me).unwrap().max, 2000);
    let hotbar = &world.get::<Hotbar>(me).unwrap().0;
    assert_eq!(hotbar[1].as_deref(), Some("provoke"));
    assert_eq!(
        hotbar[REKINDLE].as_deref(),
        Some("rekindle"),
        "the shared lantern abilities stay on the hotbar"
    );
    assert!(
        game.events
            .iter()
            .any(|e| matches!(e, ServerEvent::ClassChanged { .. }))
    );
}

#[test]
fn moving_cancels_a_flame_change() {
    let mut game = Game::new();
    game.send(ClientRequest::ChangeClass {
        class: "guardian".into(),
    });
    game.run(0.5);
    game.walk(Vec2::X, 0.1);
    game.run(2.0);
    let me = game.me();
    assert_eq!(
        game.app.world().get::<CurrentClass>(me).unwrap().class,
        "fighter"
    );
}

#[test]
fn cannot_change_flame_in_combat_or_to_the_same_class() {
    let mut game = Game::new();
    game.send(ClientRequest::ChangeClass {
        class: "fighter".into(),
    });
    game.run(0.1);
    game.use_slot(STRIKE);
    game.run(0.1);
    game.send(ClientRequest::ChangeClass {
        class: "guardian".into(),
    });
    game.run(0.1);
    game.send(ClientRequest::ChangeClass {
        class: "wizard".into(),
    });
    game.run(0.1);
    assert_eq!(
        game.rejections(),
        vec![
            Reject::AlreadyThatClass,
            Reject::InCombat,
            Reject::UnknownClass
        ]
    );
    game.run(6.0);
    game.send(ClientRequest::ChangeClass {
        class: "guardian".into(),
    });
    game.run(2.1);
    let me = game.me();
    assert_eq!(
        game.app.world().get::<CurrentClass>(me).unwrap().class,
        "guardian"
    );
}

#[test]
fn durable_classes_and_taunts_take_the_enemys_attention() {
    let mut game = Game::with_enemies(&[("hitter", Vec3::new(0.0, 0.0, -3.0))]);
    game.send(ClientRequest::ChangeClass {
        class: "guardian".into(),
    });
    game.run(2.1);
    let hitter = game.enemy("Hitter");
    game.use_on(1, Some(hitter)); // Provoke
    game.run(2.1);
    let me = game.me();
    assert_eq!(game.damage_to(me), vec![100]);
}

// ---------- Defeat ----------

#[test]
fn defeated_players_get_back_up() {
    let mut game = Game::with_enemies(&[("hitter", Vec3::new(0.0, 0.0, -3.0))]);
    let hitter = game.enemy("Hitter");
    game.use_on(STRIKE, Some(hitter));
    game.run(20.1); // ten swats of 100: down
    let me = game.me();
    assert!(game.app.world().get::<Defeated>(me).is_some());
    assert!(
        game.events
            .iter()
            .any(|e| matches!(e, ServerEvent::Defeated { .. }))
    );
    game.use_on(STRIKE, Some(hitter));
    game.run(0.1);
    assert!(game.rejections().contains(&Reject::Dead));
    game.run(5.0);
    assert!(game.app.world().get::<Defeated>(me).is_none());
    assert_eq!(game.health(me).current, 1000);
}

// ---------- Zones and portals (Milestone 4) ----------

#[test]
fn portals_move_you_to_another_zone() {
    let mut game = Game::new();
    let me = game.me();
    assert_eq!(game.zone_of(me), "test");
    game.send(ClientRequest::Interact);
    game.run(0.1);
    assert_eq!(
        game.rejections(),
        vec![Reject::NothingHere],
        "not standing in a portal"
    );
    game.enter_arena(ME);
    assert_eq!(game.zone_of(me), "arena");
    let position = game.app.world().get::<Motion>(me).unwrap().0.position;
    assert_eq!(position, ARENA_ENTRANCE);
}

#[test]
fn portals_cannot_be_used_in_combat() {
    let mut game = Game::new();
    game.use_slot(STRIKE);
    game.run(0.1);
    game.enter_arena(ME);
    let me = game.me();
    assert_eq!(game.zone_of(me), "test");
    assert!(game.rejections().contains(&Reject::InCombat));
}

#[test]
fn characters_in_other_zones_are_out_of_reach() {
    // A dummy in the field, right where the player will stand in the arena.
    let mut game = Game::with_enemies(&[("dummy", ARENA_ENTRANCE)]);
    game.enter_arena(ME);
    let dummy = game.dummy();
    game.use_on(STRIKE, Some(dummy));
    game.run(0.1);
    assert_eq!(game.rejections(), vec![Reject::InvalidTarget]);
}

// ---------- The trial (Milestone 4) ----------

/// Enter the arena and pull the boss with a bolt; returns the boss.
fn pull_boss(game: &mut Game) -> Entity {
    game.enter_arena(ME);
    let boss = game.enemy("Warden");
    game.use_on(BOLT, Some(boss));
    game.run(2.1);
    assert!(
        game.events
            .iter()
            .any(|e| matches!(e, ServerEvent::EncounterStarted { .. })),
        "the fight started"
    );
    boss
}

#[test]
fn standing_in_a_marker_hurts() {
    let mut game = Game::new();
    pull_boss(&mut game);
    game.run(1.1); // slam starts 1 s into the fight
    assert_eq!(game.markers(), 1, "a marker is on the ground");
    game.run(2.0);
    let me = game.me();
    assert_eq!(game.damage_from(me, "slam"), vec![300]);
    assert_eq!(game.markers(), 0, "and it is gone once it goes off");
}

#[test]
fn walking_out_of_a_marker_dodges_it() {
    let mut game = Game::new();
    pull_boss(&mut game);
    game.run(1.1);
    game.walk(Vec2::X, 1.0); // 6 m sideways: out of the 3 m circle
    game.run(1.5);
    let me = game.me();
    assert!(game.damage_from(me, "slam").is_empty());
}

#[test]
fn stack_markers_share_the_damage() {
    let mut game = Game::new();
    let friend = game.join_friend();
    game.enter_arena(FRIEND);
    pull_boss(&mut game);
    // Stand together. The stack goes off at 6 s.
    game.app
        .world_mut()
        .get_mut::<Motion>(friend)
        .unwrap()
        .0
        .position = ARENA_ENTRANCE;
    game.run(6.1);
    let me = game.me();
    assert_eq!(game.damage_from(me, "sap"), vec![300]);
    assert_eq!(game.damage_from(friend, "sap"), vec![300]);
}

#[test]
fn spread_markers_hit_everyone_inside_each_one() {
    let mut game = Game::new();
    let friend = game.join_friend();
    game.enter_arena(FRIEND);
    pull_boss(&mut game);
    // Stand together for the spreads at 9 s: both markers hit both of us.
    game.app
        .world_mut()
        .get_mut::<Motion>(friend)
        .unwrap()
        .0
        .position = ARENA_ENTRANCE;
    game.run(9.1);
    let me = game.me();
    assert_eq!(game.damage_from(me, "seeds"), vec![200, 200]);
    assert_eq!(game.damage_from(friend, "seeds"), vec![200, 200]);
}

#[test]
fn the_boss_changes_phase_and_calls_adds() {
    let mut game = Game::new();
    let boss = pull_boss(&mut game);
    game.set_health(boss, 400); // below 50%
    game.run(0.2);
    assert!(
        game.events
            .iter()
            .any(|e| matches!(e, ServerEvent::Announce { text, .. } if text == "Grow!"))
    );
    game.enemy("Sapling"); // panics if it wasn't summoned
    game.run(3.0);
    let me = game.me();
    assert_eq!(
        game.damage_from(me, "quake"),
        vec![50],
        "phase two's timeline runs"
    );
}

#[test]
fn defeating_the_boss_wins_the_fight() {
    let mut game = Game::new();
    let boss = pull_boss(&mut game);
    game.set_health(boss, 1);
    game.use_on(BURST, Some(boss));
    game.run(0.2);
    assert!(
        game.events
            .iter()
            .any(|e| matches!(e, ServerEvent::EncounterWon { .. }))
    );
}

#[test]
fn when_everyone_falls_the_fight_resets_at_the_entrance() {
    let mut game = Game::new();
    let boss = pull_boss(&mut game);
    let me = game.me();
    game.app
        .world_mut()
        .get_mut::<Motion>(me)
        .unwrap()
        .0
        .position = Vec3::new(0.0, 0.0, 4.0);
    game.set_health(me, 1);
    game.run(3.0); // the boss's slam (aimed at us) finishes us
    assert!(
        game.app.world().get::<Defeated>(me).is_some(),
        "{:#?}",
        game.events
    );
    assert!(
        game.events
            .iter()
            .any(|e| matches!(e, ServerEvent::EncounterWiped { .. }))
    );
    game.run(4.5);
    assert!(
        game.app.world().get::<Defeated>(me).is_none(),
        "back on our feet"
    );
    assert_eq!(game.health(me).current, 1000);
    assert_eq!(
        game.app.world().get::<Motion>(me).unwrap().0.position,
        ARENA_ENTRANCE
    );
    let boss_health = game.health(boss);
    assert_eq!(
        boss_health.current, boss_health.max,
        "the boss is back to full"
    );
}

#[test]
fn in_a_trial_you_wait_to_be_raised() {
    let mut game = Game::new();
    let friend = game.join_friend();
    game.enter_arena(ME);
    game.enter_arena(FRIEND);
    let me = game.me();
    game.set_health(me, 0);
    game.run(6.0);
    assert!(
        game.app.world().get::<Defeated>(me).is_some(),
        "no automatic revive here"
    );
    // The friend rekindles us.
    game.app
        .world_mut()
        .get_mut::<Motion>(friend)
        .unwrap()
        .0
        .position = ARENA_ENTRANCE;
    game.send_as(
        FRIEND,
        ClientRequest::UseAbility {
            slot: REKINDLE,
            target: Some(me),
        },
    );
    game.run(3.2);
    assert!(game.app.world().get::<Defeated>(me).is_none());
    assert_eq!(game.health(me).current, 300, "30% health");
}

#[test]
fn second_wind_heals_you() {
    let mut game = Game::new();
    let me = game.me();
    game.set_health(me, 500);
    game.use_on(SECOND_WIND, None);
    game.run(0.1);
    assert_eq!(game.health(me).current, 800);
}

#[test]
fn the_boss_has_more_health_with_more_players() {
    let mut game = Game::new();
    game.join_friend();
    game.enter_arena(FRIEND);
    let boss = pull_boss(&mut game);
    assert_eq!(game.health(boss).max, 1500, "+50% for the second player");
}
