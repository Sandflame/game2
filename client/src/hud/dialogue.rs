//! Conversations: when the rules half says a dialogue plays, its lines are
//! shown one at a time in a box above the hotbar. E or a click shows the
//! next line; Esc skips the rest. (Quest progress was already recorded, so
//! skipping never loses anything.)

use std::collections::VecDeque;

use bevy::prelude::*;
use shared::gamedata::GameData;
use shared::protocol::ServerEvent;
use shared::quests::Line;

use super::{font, palette, text_shadow};
use crate::session::{LocalPlayerId, Received};

pub struct DialoguePlugin;

impl Plugin for DialoguePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Conversation>()
            .add_systems(Startup, spawn_box)
            .add_systems(
                Update,
                (start_dialogue, advance_dialogue, show_dialogue).chain(),
            );
    }
}

/// The lines being shown, and any conversations waiting after them.
#[derive(Resource, Default)]
pub struct Conversation {
    lines: VecDeque<Line>,
    waiting: VecDeque<Vec<Line>>,
}

impl Conversation {
    /// Is a conversation on screen? (E then shows the next line instead of
    /// interacting.)
    pub fn open(&self) -> bool {
        !self.lines.is_empty()
    }

    fn next(&mut self) {
        self.lines.pop_front();
        if self.lines.is_empty()
            && let Some(more) = self.waiting.pop_front()
        {
            self.lines = more.into();
        }
    }

    /// Close the conversation (and any waiting after it).
    pub fn skip(&mut self) {
        self.lines.clear();
        self.waiting.clear();
    }
}

#[derive(Component)]
pub(super) struct DialogueBox;
#[derive(Component)]
struct Speaker;
#[derive(Component)]
struct Words;
#[derive(Component)]
struct Remaining;

fn spawn_box(mut commands: Commands) {
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: px(170),
            width: percent(100),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                DialogueBox,
                Button,
                Node {
                    width: px(620),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(px(14)),
                    row_gap: px(8),
                    border: UiRect::all(px(2)),
                    ..default()
                },
                BackgroundColor(palette::PANEL.with_alpha(0.93)),
                BorderColor::all(palette::PANEL_BORDER),
                Visibility::Hidden,
                GlobalZIndex(8),
            ))
            .with_children(|panel| {
                panel.spawn((
                    Speaker,
                    Text::new(""),
                    font(17.0),
                    TextColor(palette::BANNER),
                    text_shadow(),
                ));
                panel.spawn((Words, Text::new(""), font(16.0), TextColor(palette::TEXT)));
                panel.spawn((
                    Remaining,
                    Text::new(""),
                    font(12.0),
                    TextColor(palette::TEXT_DIM),
                    Node {
                        align_self: AlignSelf::FlexEnd,
                        ..default()
                    },
                ));
            });
        });
}

fn start_dialogue(
    mut received: MessageReader<Received>,
    me: Res<LocalPlayerId>,
    data: Res<GameData>,
    mut conversation: ResMut<Conversation>,
) {
    for Received(event) in received.read() {
        let ServerEvent::Dialogue { player, dialogue } = event else {
            continue;
        };
        let Some(def) = data.dialogues.get(dialogue).filter(|_| *player == me.0) else {
            continue;
        };
        if conversation.open() {
            conversation.waiting.push_back(def.0.clone());
        } else {
            conversation.lines = def.0.iter().cloned().collect();
        }
    }
}

/// E or a click: next line. Esc: skip it all.
pub(super) fn advance_dialogue(
    keys: Res<ButtonInput<KeyCode>>,
    clicked: Query<&Interaction, (Changed<Interaction>, With<DialogueBox>)>,
    mut conversation: ResMut<Conversation>,
) {
    if !conversation.open() {
        return;
    }
    if keys.just_pressed(KeyCode::Escape) {
        conversation.skip();
        return;
    }
    let click = clicked.iter().any(|i| *i == Interaction::Pressed);
    if keys.just_pressed(KeyCode::KeyE) || click {
        conversation.next();
    }
}

fn show_dialogue(
    conversation: Res<Conversation>,
    mut panel: Single<&mut Visibility, With<DialogueBox>>,
    mut texts: ParamSet<(
        Single<&mut Text, With<Speaker>>,
        Single<&mut Text, With<Words>>,
        Single<&mut Text, With<Remaining>>,
    )>,
) {
    if !conversation.is_changed() {
        return;
    }
    let Some(line) = conversation.lines.front() else {
        **panel = Visibility::Hidden;
        return;
    };
    **panel = Visibility::Visible;
    texts.p0().0 = line.who.clone();
    texts.p1().0 = line.says.clone();
    let left =
        conversation.lines.len() - 1 + conversation.waiting.iter().map(Vec::len).sum::<usize>();
    texts.p2().0 = if left == 0 {
        "[E] or click: close".to_owned()
    } else {
        format!("[E] or click: next ({left} more)     [Esc]: skip")
    };
}
