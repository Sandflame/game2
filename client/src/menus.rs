//! The screens before the game: log in (or make an account), the character
//! list (play, make or delete characters), and character creation (name,
//! starting class, race and look, with a turning 3D preview). Everything
//! is decided by the rules half; these screens only send requests and show
//! the answers. Without a save file (demos, tests) the game skips straight
//! to playing.

use bevy::input::ButtonState;
use bevy::input::keyboard::{Key, KeyboardInput};
use bevy::prelude::*;
use shared::appearance::{Appearance, RaceDef};
use shared::classes::CurrentClass;
use shared::gamedata::GameData;
use shared::protocol::{CharacterSummary, ClientRequest, Link, ServerEvent};

use crate::camera::FollowCamera;
use crate::hud::{
    character::CharacterPanel, font, journal::QuestJournal, lantern::LanternPanel, map::WorldMap,
    options::OptionsMenu, palette,
};
use crate::models::{ModelLook, ModelPending};
use crate::net::{self, Connection};
use crate::session::{LocalPlayerId, Received, send};
use crate::settings::LastServer;
use crate::toon::{Outline, ToonAssets};
use server::AuthorityActive;

/// Which screen is showing.
#[derive(States, Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Screen {
    #[default]
    Login,
    Characters,
    Create,
    Playing,
}

pub struct MenusPlugin;

impl Plugin for MenusPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MenuState>()
            .init_resource::<PendingLogin>()
            .init_resource::<Focus>()
            .init_resource::<MenuActions>()
            .add_systems(OnEnter(Screen::Login), spawn_menu_scenery)
            .add_systems(Update, watch_connection)
            .add_systems(OnExit(Screen::Playing), dusk)
            .add_systems(OnEnter(Screen::Login), spawn_login)
            .add_systems(OnEnter(Screen::Characters), spawn_characters)
            .add_systems(OnEnter(Screen::Create), spawn_create)
            .add_systems(OnEnter(Screen::Playing), (leave_menus, show_hud))
            .add_systems(
                Update,
                (
                    hear_answers,
                    press_buttons,
                    type_text,
                    run_actions,
                    fill_lists,
                    show_fields,
                    show_message,
                    update_preview,
                    place_camera,
                    hide_hud,
                )
                    .chain()
                    .run_if(not(in_state(Screen::Playing))),
            );
    }
}

/// What the menus know and have chosen.
#[derive(Resource)]
pub struct MenuState {
    pub characters: Vec<CharacterSummary>,
    pub selected: usize,
    /// A line to show: (text, is it a problem?).
    pub message: Option<(String, bool)>,
    /// Delete was pressed once; pressing it again deletes.
    pub confirm_delete: bool,
    /// The character being made.
    pub draft: Draft,
    /// Waiting for an answer from the rules half.
    pub waiting: bool,
    /// The lists need rebuilding.
    dirty: bool,
}

impl Default for MenuState {
    fn default() -> Self {
        Self {
            characters: Vec::new(),
            selected: 0,
            message: None,
            confirm_delete: false,
            draft: Draft::default(),
            waiting: false,
            dirty: true,
        }
    }
}

#[derive(Clone, Default)]
pub struct Draft {
    pub class: String,
    pub look: Option<Appearance>,
}

/// Something a menu button (or the demo script) asks for.
#[derive(Component, Clone, Debug, PartialEq)]
pub enum MenuAction {
    Login,
    Register,
    Logout,
    Select(usize),
    Play,
    NewCharacter,
    Delete,
    Create,
    Back,
    Class(String),
    Race(String),
    /// Step a choice by -1 or +1.
    Step(Choice, i32),
    Focus(FieldId),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Choice {
    Face,
    Skin,
    Hair,
    Feature,
    FeatureColor,
    Height,
}

/// Actions waiting to run (from buttons or the demo script).
#[derive(Resource, Default)]
pub struct MenuActions(pub Vec<MenuAction>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FieldId {
    Server,
    Account,
    Password,
    Name,
}

/// A box you can type in.
#[derive(Component)]
pub struct TextField {
    pub id: FieldId,
    pub value: String,
    pub secret: bool,
    pub max: usize,
}

#[derive(Component)]
struct FieldText(FieldId);

/// Which box keys go to.
#[derive(Resource, Default)]
pub struct Focus(pub Option<FieldId>);

/// Everything belonging to the menus (removed when the game starts).
#[derive(Component)]
struct MenuScenery;

/// The character shown on the menus.
#[derive(Component)]
struct Preview;

#[derive(Component)]
struct MessageLine;

/// Where the character list / creation choices are built.
#[derive(Component)]
struct ListArea;

/// HUD roots hidden while the menus show.
#[derive(Component)]
struct HiddenByMenu;

/// The preview sways this far either side of facing the camera (radians),
/// this fast; arrow keys turn it (radians per second).
const PREVIEW_SWAY: f32 = 0.5;
const PREVIEW_SWAY_SPEED: f32 = 0.6;
const PREVIEW_TURN: f32 = 2.0;
/// Behind the menus: a dusky sky.
const MENU_SKY: Color = Color::srgb(0.16, 0.17, 0.27);
/// Where the camera stands to look at the preview (it stands to the right
/// of the panels).
const CAMERA_AT: Vec3 = Vec3::new(-1.1, 1.25, 4.3);
const CAMERA_LOOKS_AT: Vec3 = Vec3::new(-1.1, 0.95, 0.0);
const HEIGHT_STEP: f32 = 0.25;

fn root(screen: Screen) -> impl Bundle {
    (
        MenuScenery,
        DespawnOnExit(screen),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            padding: UiRect::all(px(28)),
            column_gap: px(20),
            ..default()
        },
        GlobalZIndex(20),
    )
}

fn panel(width: f32) -> impl Bundle {
    (
        Node {
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(px(16)),
            row_gap: px(9),
            border: UiRect::all(px(2)),
            width: px(width),
            align_self: AlignSelf::FlexStart,
            ..default()
        },
        BackgroundColor(palette::PANEL.with_alpha(0.9)),
        BorderColor::all(palette::PANEL_BORDER),
    )
}

fn button(parent: &mut ChildSpawnerCommands, action: MenuAction, label: &str, size: f32) {
    parent
        .spawn((
            Button,
            action,
            Node {
                padding: UiRect::axes(px(12), px(6)),
                border: UiRect::all(px(1)),
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
            BorderColor::all(palette::PANEL_BORDER),
        ))
        .with_child((Text::new(label), font(size), TextColor(palette::TEXT)));
}

fn text_field(
    parent: &mut ChildSpawnerCommands,
    id: FieldId,
    label: &str,
    secret: bool,
    max: usize,
) {
    text_field_with(parent, id, label, secret, max, "");
}

fn text_field_with(
    parent: &mut ChildSpawnerCommands,
    id: FieldId,
    label: &str,
    secret: bool,
    max: usize,
    value: &str,
) {
    parent.spawn((Text::new(label), font(13.0), TextColor(palette::TEXT_DIM)));
    parent
        .spawn((
            Button,
            MenuAction::Focus(id),
            TextField {
                id,
                value: value.to_owned(),
                secret,
                max,
            },
            Node {
                padding: UiRect::axes(px(10), px(6)),
                border: UiRect::all(px(1)),
                min_height: px(32),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.5)),
            BorderColor::all(palette::PANEL_BORDER),
        ))
        .with_child((
            FieldText(id),
            Text::new(""),
            font(16.0),
            TextColor(palette::TEXT),
        ));
}

fn message_line(parent: &mut ChildSpawnerCommands) {
    parent.spawn((
        MessageLine,
        Text::new(""),
        font(13.0),
        TextColor(palette::TEXT_DIM),
    ));
}

fn spawn_login(mut commands: Commands, mut focus: ResMut<Focus>, server: Res<LastServer>) {
    focus.0 = Some(FieldId::Account);
    // A message from before (a lost connection) stays.
    commands.spawn(root(Screen::Login)).with_children(|row| {
        row.spawn(panel(400.0)).with_children(|p| {
            p.spawn((
                Text::new("Lanternflame"),
                font(34.0),
                TextColor(palette::BANNER),
            ));
            p.spawn((
                Text::new("Log in to your account, or make a new one."),
                font(13.0),
                TextColor(palette::TEXT_DIM),
            ));
            text_field_with(
                p,
                FieldId::Server,
                "Server (leave empty to play on this computer)",
                false,
                64,
                &server.0,
            );
            text_field(p, FieldId::Account, "Account name", false, 16);
            text_field(p, FieldId::Password, "Password", true, 64);
            p.spawn(Node {
                column_gap: px(10),
                margin: UiRect::top(px(6)),
                ..default()
            })
            .with_children(|buttons| {
                button(buttons, MenuAction::Login, "Log in", 16.0);
                button(buttons, MenuAction::Register, "Make account", 16.0);
            });
            message_line(p);
            p.spawn((
                Text::new("Tab: next box    Enter: log in"),
                font(11.0),
                TextColor(palette::TEXT_DIM),
            ));
        });
    });
}

fn spawn_characters(
    mut commands: Commands,
    mut state: ResMut<MenuState>,
    mut focus: ResMut<Focus>,
) {
    focus.0 = None;
    state.dirty = true;
    state.confirm_delete = false;
    commands
        .spawn(root(Screen::Characters))
        .with_children(|row| {
            row.spawn(panel(360.0)).with_children(|p| {
                p.spawn((
                    Text::new("Your characters"),
                    font(22.0),
                    TextColor(palette::BANNER),
                ));
                p.spawn((
                    ListArea,
                    Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: px(6),
                        ..default()
                    },
                ));
                p.spawn(Node {
                    column_gap: px(8),
                    margin: UiRect::top(px(8)),
                    flex_wrap: FlexWrap::Wrap,
                    row_gap: px(8),
                    ..default()
                })
                .with_children(|buttons| {
                    button(buttons, MenuAction::Play, "Play", 18.0);
                    button(buttons, MenuAction::NewCharacter, "New character", 15.0);
                    button(buttons, MenuAction::Delete, "Delete", 15.0);
                    button(buttons, MenuAction::Logout, "Log out", 15.0);
                });
                message_line(p);
            });
        });
}

fn spawn_create(mut commands: Commands, mut state: ResMut<MenuState>, mut focus: ResMut<Focus>) {
    focus.0 = Some(FieldId::Name);
    state.dirty = true;
    state.message = None;
    commands.spawn(root(Screen::Create)).with_children(|row| {
        row.spawn(panel(480.0)).with_children(|p| {
            p.spawn((
                Text::new("A new character"),
                font(22.0),
                TextColor(palette::BANNER),
            ));
            text_field(p, FieldId::Name, "Name", false, 16);
            p.spawn((
                ListArea,
                Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: px(6),
                    ..default()
                },
            ));
            p.spawn(Node {
                column_gap: px(10),
                margin: UiRect::top(px(6)),
                ..default()
            })
            .with_children(|buttons| {
                button(buttons, MenuAction::Create, "Create", 18.0);
                button(buttons, MenuAction::Back, "Back", 15.0);
            });
            message_line(p);
        });
    });
}

/// A stone platform for the preview, and the preview character.
fn spawn_menu_scenery(
    mut commands: Commands,
    mut toon: ToonAssets,
    data: Res<GameData>,
    existing: Query<(), With<Preview>>,
) {
    if !existing.is_empty() {
        return;
    }
    commands.insert_resource(ClearColor(MENU_SKY));
    let stone = toon.material(Color::srgb(0.55, 0.52, 0.58));
    let rim = toon.glowing(Color::srgb(1.0, 0.8, 0.4), LinearRgba::rgb(2.0, 1.4, 0.5));
    let base = commands
        .spawn((MenuScenery, Transform::default(), Visibility::default()))
        .id();
    toon.spawn_part(
        &mut commands,
        base,
        Cylinder::new(1.4, 0.2),
        stone,
        Outline::Cylinder,
        Transform::from_xyz(0.0, -0.1, 0.0),
    );
    toon.spawn_part(
        &mut commands,
        base,
        Torus::new(1.38, 1.46),
        rim,
        Outline::None,
        Transform::from_xyz(0.0, 0.0, 0.0),
    );
    let class = data.player.start_class.clone();
    let spec = data
        .classes
        .get(&class)
        .map(|c| c.default_spec.clone())
        .unwrap_or_default();
    commands.spawn((
        MenuScenery,
        Preview,
        ModelLook::Player,
        ModelPending,
        CurrentClass { class, spec },
        Appearance::first(&data.races),
        Transform::from_rotation(Quat::from_rotation_y(std::f32::consts::PI)),
        Visibility::default(),
    ));
}

/// A lost connection goes back to the login screen with a message.
fn watch_connection(
    mut commands: Commands,
    mut connection: ResMut<Connection>,
    mut state: ResMut<MenuState>,
    mut pending: ResMut<PendingLogin>,
    mut next: ResMut<NextState<Screen>>,
    remote: Query<Entity, With<lightyear::prelude::client::Remote>>,
) {
    let Connection::Lost(why) = &*connection else {
        return;
    };
    state.message = Some((why.clone(), true));
    state.waiting = false;
    state.characters.clear();
    pending.0 = None;
    // What the server sent is gone with it.
    for entity in &remote {
        commands.entity(entity).despawn();
    }
    *connection = Connection::Local;
    next.set(Screen::Login);
}

fn dusk(mut sky: ResMut<ClearColor>) {
    sky.0 = MENU_SKY;
}

fn leave_menus(
    mut commands: Commands,
    scenery: Query<Entity, With<MenuScenery>>,
    mut focus: ResMut<Focus>,
) {
    focus.0 = None;
    for e in &scenery {
        commands.entity(e).despawn();
    }
}

/// The game's HUD stays hidden behind the menus.
fn hide_hud(
    mut commands: Commands,
    roots: Query<(Entity, &mut Visibility), (With<Node>, Without<ChildOf>, Without<MenuScenery>)>,
    mut panels: (
        ResMut<LanternPanel>,
        ResMut<OptionsMenu>,
        ResMut<CharacterPanel>,
        ResMut<WorldMap>,
        ResMut<QuestJournal>,
    ),
) {
    for (e, mut visibility) in roots {
        *visibility = Visibility::Hidden;
        commands.entity(e).insert(HiddenByMenu);
    }
    // Keys typed into boxes must not open game panels.
    panels.0.open = false;
    panels.1.open = false;
    panels.2.open = false;
    panels.3.open = false;
    panels.4.open = false;
}

fn show_hud(
    mut commands: Commands,
    mut hidden: Query<(Entity, &mut Visibility), With<HiddenByMenu>>,
) {
    for (e, mut visibility) in &mut hidden {
        *visibility = Visibility::Inherited;
        commands.entity(e).remove::<HiddenByMenu>();
    }
}

fn hear_answers(
    (mut pending, mut link): (ResMut<PendingLogin>, ResMut<Link>),
    mut received: MessageReader<Received>,
    me: Res<LocalPlayerId>,
    data: Res<GameData>,
    mut state: ResMut<MenuState>,
    screen: Res<State<Screen>>,
    mut next: ResMut<NextState<Screen>>,
) {
    for Received(event) in received.read() {
        match event {
            ServerEvent::LoggedIn { player, account } if *player == me.0 => {
                state.message = Some((format!("Welcome, {account}."), false));
                state.waiting = false;
                next.set(Screen::Characters);
            }
            ServerEvent::Characters { player, characters } if *player == me.0 => {
                let made = characters.len() > state.characters.len();
                state.characters = characters.clone();
                if state.waiting {
                    state.message = None;
                }
                state.waiting = false;
                state.dirty = true;
                if *screen.get() == Screen::Create && made {
                    // Select the new character.
                    state.selected = state.characters.len().saturating_sub(1);
                    next.set(Screen::Characters);
                }
                state.selected = state.selected.min(state.characters.len().saturating_sub(1));
            }
            ServerEvent::AccountError { player, message } if *player == me.0 => {
                state.message = Some((message.clone(), true));
                state.waiting = false;
            }
            ServerEvent::Joined { player, .. } if *player == me.0 => {
                next.set(Screen::Playing);
            }
            ServerEvent::Welcome { player } => {
                if let Some(request) = pending.0.take() {
                    link.to_authority.push((*player, request));
                    state.message = Some(("Just a moment...".into(), false));
                }
            }
            _ => {}
        }
    }
    if state.draft.look.is_none() {
        state.draft.look = Some(Appearance::first(&data.races));
        state.draft.class = data.player.start_class.clone();
    }
}

fn press_buttons(
    buttons: Query<(&Interaction, &MenuAction), Changed<Interaction>>,
    mut colours: Query<(&Interaction, &mut BackgroundColor), With<MenuAction>>,
    mut actions: ResMut<MenuActions>,
) {
    for (interaction, action) in &buttons {
        if *interaction == Interaction::Pressed {
            actions.0.push(action.clone());
        }
    }
    for (interaction, mut background) in &mut colours {
        background.0 = match interaction {
            Interaction::Hovered | Interaction::Pressed => Color::srgba(1.0, 1.0, 1.0, 0.1),
            Interaction::None => Color::srgba(0.0, 0.0, 0.0, 0.35),
        };
    }
}

/// Keys go to the focused box. Tab moves on; Enter logs in or creates.
fn type_text(
    mut keys: MessageReader<KeyboardInput>,
    held: Res<ButtonInput<KeyCode>>,
    mut focus: ResMut<Focus>,
    mut fields: Query<&mut TextField>,
    screen: Res<State<Screen>>,
    mut actions: ResMut<MenuActions>,
) {
    let ctrl = held.any_pressed([KeyCode::ControlLeft, KeyCode::ControlRight]);
    for key in keys.read() {
        if key.state != ButtonState::Pressed {
            continue;
        }
        match &key.logical_key {
            Key::Tab => {
                focus.0 = match (*screen.get(), focus.0) {
                    (Screen::Login, Some(FieldId::Server)) => Some(FieldId::Account),
                    (Screen::Login, Some(FieldId::Account)) => Some(FieldId::Password),
                    (Screen::Login, _) => Some(FieldId::Server),
                    (Screen::Create, _) => Some(FieldId::Name),
                    _ => None,
                };
            }
            Key::Enter => match screen.get() {
                Screen::Login => actions.0.push(MenuAction::Login),
                Screen::Create => actions.0.push(MenuAction::Create),
                Screen::Characters => actions.0.push(MenuAction::Play),
                Screen::Playing => {}
            },
            Key::Backspace => {
                for mut field in &mut fields {
                    if Some(field.id) == focus.0 {
                        field.value.pop();
                    }
                }
            }
            Key::Character(text) if !ctrl => {
                for mut field in &mut fields {
                    if Some(field.id) == focus.0 && field.value.chars().count() < field.max {
                        field.value.push_str(text);
                    }
                }
            }
            Key::Space => {
                for mut field in &mut fields {
                    if Some(field.id) == focus.0
                        && !field.secret
                        && field.value.chars().count() < field.max
                    {
                        field.value.push(' ');
                    }
                }
            }
            _ => {}
        }
    }
}

fn field_value(fields: &Query<&mut TextField>, id: FieldId) -> String {
    fields
        .iter()
        .find(|f| f.id == id)
        .map(|f| f.value.clone())
        .unwrap_or_default()
}

/// Where the login screen's request goes.
#[derive(Debug, PartialEq)]
pub enum Where {
    /// The rules half in this game.
    Here,
    /// The server we are already connected to.
    Connected,
    /// Connect to the server first.
    Connect,
}

/// Decide where to log in, given the server box (`""` = this computer),
/// the connection, and whether the rules have already run in this game.
pub fn where_to_play(
    server: &str,
    connection: &Connection,
    ran_here: bool,
) -> Result<Where, String> {
    match (server.is_empty(), connection) {
        (_, Connection::Connecting(_)) => Err("Still connecting, just a moment...".into()),
        (true, Connection::Online(_)) => {
            Err("You are connected to a server. Restart the game to play on this computer.".into())
        }
        (true, _) => Ok(Where::Here),
        (false, Connection::Online(address)) if address == server => Ok(Where::Connected),
        (false, Connection::Online(_)) => {
            Err("Restart the game to switch to another server.".into())
        }
        (false, _) if ran_here => {
            Err("You played on this computer. Restart the game to join a server.".into())
        }
        (false, _) => Ok(Where::Connect),
    }
}

/// A login waiting for the connection to the server.
#[derive(Resource, Default)]
pub struct PendingLogin(pub Option<ClientRequest>);

/// Step through a list of `count` choices, wrapping round.
fn step(index: usize, count: usize, by: i32) -> usize {
    if count == 0 {
        return 0;
    }
    (index as i32 + by).rem_euclid(count as i32) as usize
}

/// Change one choice of a look (wrapping round each list).
pub fn change_look(look: &mut Appearance, race: &RaceDef, choice: Choice, by: i32) {
    match choice {
        Choice::Face => look.face = step(look.face, race.faces.len(), by),
        Choice::Skin => look.skin = step(look.skin, race.skins.len(), by),
        Choice::Hair => look.hair = step(look.hair, race.hair.len(), by),
        Choice::Feature => look.feature = step(look.feature, race.features.len(), by),
        Choice::FeatureColor => {
            look.feature_color = step(look.feature_color, race.feature_colors.len(), by)
        }
        Choice::Height => look.height = (look.height + HEIGHT_STEP * by as f32).clamp(0.0, 1.0),
    }
}

fn run_actions(
    mut actions: ResMut<MenuActions>,
    mut state: ResMut<MenuState>,
    mut focus: ResMut<Focus>,
    mut fields: Query<&mut TextField>,
    data: Res<GameData>,
    me: Res<LocalPlayerId>,
    mut link: ResMut<Link>,
    mut next: ResMut<NextState<Screen>>,
    mut commands: Commands,
    (mut connection, mut authority, mut last_server, mut pending): (
        ResMut<Connection>,
        ResMut<AuthorityActive>,
        ResMut<LastServer>,
        ResMut<PendingLogin>,
    ),
) {
    for action in std::mem::take(&mut actions.0) {
        if !matches!(action, MenuAction::Delete) {
            state.confirm_delete = false;
        }
        match action {
            MenuAction::Focus(id) => focus.0 = Some(id),
            MenuAction::Login | MenuAction::Register => {
                let account = field_value(&fields, FieldId::Account);
                let password = field_value(&fields, FieldId::Password);
                let request = if action == MenuAction::Login {
                    ClientRequest::Login { account, password }
                } else {
                    ClientRequest::Register { account, password }
                };
                let server = field_value(&fields, FieldId::Server).trim().to_owned();
                last_server.0 = server.clone();
                match where_to_play(&server, &connection, authority.0) {
                    Ok(Where::Here) => {
                        // The rules run in this game.
                        authority.0 = true;
                        send(&mut link, *me, request);
                        state.waiting = true;
                        state.message = Some(("Just a moment...".into(), false));
                    }
                    Ok(Where::Connected) => {
                        send(&mut link, *me, request);
                        state.waiting = true;
                        state.message = Some(("Just a moment...".into(), false));
                    }
                    Ok(Where::Connect) => {
                        let started = net::parse_address(&server, data.config.network.port)
                            .and_then(|address| net::connect(&mut commands, address, &data));
                        match started {
                            Ok(()) => {
                                *connection = Connection::Connecting(server.clone());
                                pending.0 = Some(request);
                                state.waiting = true;
                                state.message = Some((format!("Connecting to {server}..."), false));
                            }
                            Err(why) => state.message = Some((why, true)),
                        }
                    }
                    Err(why) => state.message = Some((why, true)),
                }
            }
            MenuAction::Logout => {
                send(&mut link, *me, ClientRequest::Logout);
                state.characters.clear();
                state.message = None;
                for mut field in &mut fields {
                    if field.id == FieldId::Password {
                        field.value.clear();
                    }
                }
                next.set(Screen::Login);
            }
            MenuAction::Select(index) => {
                state.selected = index;
                state.dirty = true;
            }
            MenuAction::Play => {
                if let Some(character) = state.characters.get(state.selected) {
                    send(
                        &mut link,
                        *me,
                        ClientRequest::Join {
                            name: character.name.clone(),
                        },
                    );
                    state.message = Some((
                        format!("Entering the world as {}...", character.name),
                        false,
                    ));
                } else {
                    state.message = Some(("Make a character first.".into(), true));
                }
            }
            MenuAction::NewCharacter => {
                if state.characters.len() >= data.accounts.max_characters {
                    state.message = Some((
                        format!(
                            "You have the most characters an account can have ({}).",
                            data.accounts.max_characters
                        ),
                        true,
                    ));
                } else {
                    for mut field in &mut fields {
                        if field.id == FieldId::Name {
                            field.value.clear();
                        }
                    }
                    next.set(Screen::Create);
                }
            }
            MenuAction::Delete => {
                let Some(character) = state.characters.get(state.selected).cloned() else {
                    continue;
                };
                if state.confirm_delete {
                    send(
                        &mut link,
                        *me,
                        ClientRequest::DeleteCharacter {
                            name: character.name.clone(),
                        },
                    );
                    state.confirm_delete = false;
                    state.message = Some((format!("{} was deleted.", character.name), false));
                } else {
                    state.confirm_delete = true;
                    state.message = Some((
                        format!(
                            "Delete {} for good? Press Delete again to confirm.",
                            character.name
                        ),
                        true,
                    ));
                }
            }
            MenuAction::Create => {
                let name = field_value(&fields, FieldId::Name);
                let Some(look) = state.draft.look.clone() else {
                    continue;
                };
                send(
                    &mut link,
                    *me,
                    ClientRequest::CreateCharacter {
                        name,
                        class: state.draft.class.clone(),
                        appearance: look,
                    },
                );
                state.waiting = true;
                state.message = Some(("Just a moment...".into(), false));
            }
            MenuAction::Back => {
                state.message = None;
                next.set(Screen::Characters);
            }
            MenuAction::Class(class) => {
                state.draft.class = class;
                state.dirty = true;
            }
            MenuAction::Race(race) => {
                if let Some(look) = &mut state.draft.look {
                    *look = Appearance {
                        race,
                        height: look.height,
                        ..Appearance::first(&data.races)
                    };
                }
                state.dirty = true;
            }
            MenuAction::Step(choice, by) => {
                let race = state
                    .draft
                    .look
                    .as_ref()
                    .and_then(|l| data.races.get(&l.race))
                    .cloned();
                if let (Some(look), Some(race)) = (&mut state.draft.look, race) {
                    change_look(look, &race, choice, by);
                }
                state.dirty = true;
            }
        }
    }
}

fn class_name(data: &GameData, id: &str) -> String {
    data.classes
        .get(id)
        .map_or_else(|| id.to_owned(), |c| c.name.clone())
}

/// Rebuild the character list or the creation choices when they change.
fn fill_lists(
    mut commands: Commands,
    mut state: ResMut<MenuState>,
    data: Res<GameData>,
    screen: Res<State<Screen>>,
    area: Option<Single<Entity, With<ListArea>>>,
) {
    let Some(area) = area else { return };
    if !state.dirty {
        return;
    }
    state.dirty = false;
    let area = *area;
    commands.entity(area).despawn_children();
    match screen.get() {
        Screen::Characters => {
            let max = data.accounts.max_characters;
            commands.entity(area).with_children(|list| {
                list.spawn((
                    Text::new(format!("{} of {max} characters", state.characters.len())),
                    font(12.0),
                    TextColor(palette::TEXT_DIM),
                ));
                if state.characters.is_empty() {
                    list.spawn((
                        Text::new("No characters yet. Make one with \"New character\"."),
                        font(14.0),
                        TextColor(palette::TEXT),
                    ));
                }
                for (i, c) in state.characters.iter().enumerate() {
                    let race = c
                        .appearance
                        .as_ref()
                        .and_then(|a| data.races.get(&a.race))
                        .map_or("Human".to_owned(), |r| r.name.clone());
                    let chosen = i == state.selected;
                    list.spawn((
                        Button,
                        MenuAction::Select(i),
                        Node {
                            flex_direction: FlexDirection::Column,
                            padding: UiRect::axes(px(10), px(6)),
                            border: UiRect::all(px(if chosen { 2 } else { 1 })),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
                        BorderColor::all(if chosen {
                            palette::BANNER
                        } else {
                            palette::PANEL_BORDER
                        }),
                        children![
                            (
                                Text::new(c.name.clone()),
                                font(17.0),
                                TextColor(if chosen {
                                    palette::BANNER
                                } else {
                                    palette::TEXT
                                })
                            ),
                            (
                                Text::new(format!(
                                    "Level {} {} - {race}",
                                    c.level,
                                    class_name(&data, &c.class)
                                )),
                                font(12.0),
                                TextColor(palette::TEXT_DIM),
                            ),
                        ],
                    ));
                }
            });
        }
        Screen::Create => {
            let Some(look) = state.draft.look.clone() else {
                return;
            };
            let Some(race) = data.races.get(&look.race) else {
                return;
            };
            let mut classes: Vec<_> = data.classes.iter().collect();
            classes.sort_by(|a, b| a.1.name.cmp(&b.1.name));
            commands.entity(area).with_children(|list| {
                list.spawn((
                    Text::new("Starting class"),
                    font(13.0),
                    TextColor(palette::TEXT_DIM),
                ));
                list.spawn(Node {
                    column_gap: px(6),
                    flex_wrap: FlexWrap::Wrap,
                    row_gap: px(6),
                    ..default()
                })
                .with_children(|row| {
                    for (id, class) in &classes {
                        chip(
                            row,
                            MenuAction::Class((*id).clone()),
                            &class.name,
                            **id == state.draft.class,
                        );
                    }
                });
                if let Some(class) = data.classes.get(&state.draft.class) {
                    list.spawn((
                        Text::new(class.description.clone()),
                        font(11.0),
                        TextColor(palette::TEXT_DIM),
                    ));
                }
                list.spawn((Text::new("Race"), font(13.0), TextColor(palette::TEXT_DIM)));
                list.spawn(Node {
                    column_gap: px(6),
                    flex_wrap: FlexWrap::Wrap,
                    row_gap: px(6),
                    ..default()
                })
                .with_children(|row| {
                    for r in &data.races.0 {
                        chip(
                            row,
                            MenuAction::Race(r.id.clone()),
                            &r.name,
                            r.id == look.race,
                        );
                    }
                });
                list.spawn((
                    Text::new(race.description.clone()),
                    font(11.0),
                    TextColor(palette::TEXT_DIM),
                ));
                let name = |list: &[shared::appearance::Choice], i: usize| {
                    list.get(i).map(|c| c.name.clone()).unwrap_or_default()
                };
                let colour = |list: &[shared::appearance::Swatch], i: usize| {
                    list.get(i).map(|c| c.name.clone()).unwrap_or_default()
                };
                stepper(
                    list,
                    "Face and hair",
                    Choice::Face,
                    &name(&race.faces, look.face),
                );
                stepper(list, "Skin", Choice::Skin, &colour(&race.skins, look.skin));
                stepper(
                    list,
                    "Hair colour",
                    Choice::Hair,
                    &colour(&race.hair, look.hair),
                );
                if !race.features.is_empty() {
                    stepper(
                        list,
                        "Feature",
                        Choice::Feature,
                        &name(&race.features, look.feature),
                    );
                }
                if !race.feature_colors.is_empty() {
                    stepper(
                        list,
                        "Feature colour",
                        Choice::FeatureColor,
                        &colour(&race.feature_colors, look.feature_color),
                    );
                }
                let height = match (look.height * 4.0).round() as i32 {
                    0 => "Shortest",
                    1 => "Short",
                    2 => "Average",
                    3 => "Tall",
                    _ => "Tallest",
                };
                stepper(list, "Height", Choice::Height, height);
            });
        }
        _ => {}
    }
}

fn chip(parent: &mut ChildSpawnerCommands, action: MenuAction, label: &str, chosen: bool) {
    parent
        .spawn((
            Button,
            action,
            Node {
                padding: UiRect::axes(px(10), px(5)),
                border: UiRect::all(px(if chosen { 2 } else { 1 })),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
            BorderColor::all(if chosen {
                palette::BANNER
            } else {
                palette::PANEL_BORDER
            }),
        ))
        .with_child((
            Text::new(label),
            font(14.0),
            TextColor(if chosen {
                palette::BANNER
            } else {
                palette::TEXT
            }),
        ));
}

/// "Label   <  value  >"
fn stepper(parent: &mut ChildSpawnerCommands, label: &str, choice: Choice, value: &str) {
    parent
        .spawn(Node {
            align_items: AlignItems::Center,
            column_gap: px(8),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new(label),
                font(13.0),
                TextColor(palette::TEXT_DIM),
                Node {
                    width: px(120),
                    ..default()
                },
            ));
            button(row, MenuAction::Step(choice, -1), "<", 14.0);
            row.spawn((
                Text::new(value),
                font(14.0),
                TextColor(palette::TEXT),
                TextLayout::justify(Justify::Center),
                Node {
                    width: px(190),
                    ..default()
                },
            ));
            button(row, MenuAction::Step(choice, 1), ">", 14.0);
        });
}

fn show_fields(
    focus: Res<Focus>,
    time: Res<Time>,
    fields: Query<(&TextField, &mut BorderColor)>,
    mut texts: Query<(&FieldText, &mut Text)>,
) {
    let blink = (time.elapsed_secs() * 2.0) as i32 % 2 == 0;
    for (field, mut border) in fields {
        let focused = focus.0 == Some(field.id);
        *border = BorderColor::all(if focused {
            palette::BANNER
        } else {
            palette::PANEL_BORDER
        });
        let shown = if field.secret {
            "*".repeat(field.value.chars().count())
        } else {
            field.value.clone()
        };
        let caret = if focused && blink { "|" } else { "" };
        for (of, mut text) in &mut texts {
            if of.0 == field.id {
                let wanted = format!("{shown}{caret}");
                if text.0 != wanted {
                    text.0 = wanted;
                }
            }
        }
    }
}

fn show_message(
    state: Res<MenuState>,
    lines: Query<(&mut Text, &mut TextColor), With<MessageLine>>,
) {
    for (mut text, mut colour) in lines {
        let (wanted, problem) = state.message.clone().unwrap_or_default();
        if text.0 != wanted {
            text.0 = wanted;
        }
        colour.0 = if problem {
            palette::WARNING
        } else {
            palette::TEXT_DIM
        };
    }
}

/// The preview shows the selected character (or the one being made) and
/// slowly turns.
fn update_preview(
    mut commands: Commands,
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    state: Res<MenuState>,
    data: Res<GameData>,
    screen: Res<State<Screen>>,
    preview: Option<
        Single<
            (
                Entity,
                &CurrentClass,
                &Appearance,
                &mut Transform,
                &mut Visibility,
            ),
            With<Preview>,
        >,
    >,
    mut turned: Local<f32>,
) {
    let Some(preview) = preview else { return };
    let (entity, class, look, mut transform, mut visibility) = preview.into_inner();
    let wanted = match screen.get() {
        Screen::Characters => state.characters.get(state.selected).map(|c| {
            (
                c.class.clone(),
                c.appearance
                    .clone()
                    .unwrap_or_else(|| Appearance::first(&data.races)),
            )
        }),
        Screen::Create => state
            .draft
            .look
            .clone()
            .map(|l| (state.draft.class.clone(), l)),
        _ => Some((class.class.clone(), look.clone())),
    };
    *visibility = if wanted.is_some() {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
    if let Some((wanted_class, wanted_look)) = wanted {
        if wanted_class != class.class {
            let spec = data
                .classes
                .get(&wanted_class)
                .map(|c| c.default_spec.clone())
                .unwrap_or_default();
            commands.entity(entity).insert(CurrentClass {
                class: wanted_class,
                spec,
            });
        }
        if &wanted_look != look {
            commands.entity(entity).insert(wanted_look);
        }
    }
    if keys.pressed(KeyCode::ArrowLeft) {
        *turned -= PREVIEW_TURN * time.delta_secs();
    } else if keys.pressed(KeyCode::ArrowRight) {
        *turned += PREVIEW_TURN * time.delta_secs();
    }
    // Facing the camera is half a turn from the model's own facing.
    let sway = PREVIEW_SWAY * (time.elapsed_secs() * PREVIEW_SWAY_SPEED).sin();
    transform.rotation = Quat::from_rotation_y(std::f32::consts::PI + *turned + sway);
}

fn place_camera(mut camera: Single<&mut Transform, With<FollowCamera>>) {
    **camera = Transform::from_translation(CAMERA_AT).looking_at(CAMERA_LOOKS_AT, Vec3::Y);
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::data::find_assets_dir;

    #[test]
    fn choices_wrap_round_and_height_stays_in_range() {
        let data = GameData::load(&find_assets_dir().unwrap()).unwrap();
        let race = data.races.get("drake").unwrap();
        let mut look = Appearance {
            race: "drake".into(),
            ..Appearance::first(&data.races)
        };
        change_look(&mut look, race, Choice::Face, -1);
        assert_eq!(look.face, race.faces.len() - 1);
        change_look(&mut look, race, Choice::Face, 1);
        assert_eq!(look.face, 0);
        for _ in 0..10 {
            change_look(&mut look, race, Choice::Height, 1);
        }
        assert_eq!(look.height, 1.0);
        assert_eq!(look.check(&data.races), Ok(()));
        // A race without features stays at 0.
        let human = data.races.get("human").unwrap();
        let mut plain = Appearance::first(&data.races);
        change_look(&mut plain, human, Choice::Feature, 1);
        assert_eq!(plain.feature, 0);
    }

    #[test]
    fn the_server_box_decides_where_to_play() {
        let online = Connection::Online("1.2.3.4".into());
        assert_eq!(
            where_to_play("", &Connection::Local, false),
            Ok(Where::Here)
        );
        assert_eq!(where_to_play("", &Connection::Local, true), Ok(Where::Here));
        assert_eq!(
            where_to_play("1.2.3.4", &Connection::Local, false),
            Ok(Where::Connect)
        );
        assert_eq!(
            where_to_play("1.2.3.4", &online, false),
            Ok(Where::Connected)
        );
        assert!(where_to_play("1.2.3.4", &Connection::Local, true).is_err());
        assert!(where_to_play("", &online, false).is_err());
        assert!(where_to_play("5.6.7.8", &online, false).is_err());
        assert!(where_to_play("1.2.3.4", &Connection::Connecting("x".into()), false).is_err());
    }
}
