//! Speech: what townsfolk (and closed gates) say to you, in a box above
//! the hotbar that fades after a few seconds. Also the short fade to
//! black when you move to another zone.

use bevy::prelude::*;
use shared::protocol::ServerEvent;

use super::{font, palette};
use crate::session::{LocalPlayerId, Received};
use crate::world::CurrentZone;

pub struct SpeechPlugin;

impl Plugin for SpeechPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (spawn_speech_box, spawn_fade))
            .add_systems(Update, (show_speech, fade_speech, fade_between_zones));
    }
}

/// Seconds a line stays up: a base plus a little per character.
const BASE_LIFE: f32 = 3.0;
const LIFE_PER_CHARACTER: f32 = 0.05;
/// Seconds to fade in and out.
const FADE: f32 = 0.4;
/// Seconds the screen takes to clear after arriving in a zone.
const ZONE_FADE: f32 = 0.8;

#[derive(Component)]
struct SpeechBox {
    age: f32,
    life: f32,
}
#[derive(Component)]
struct Speaker;
#[derive(Component)]
struct Line;
#[derive(Component)]
struct ZoneFade(f32);

fn spawn_speech_box(mut commands: Commands) {
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: px(200),
            width: percent(100),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_child((
            SpeechBox {
                age: 0.0,
                life: 0.0,
            },
            Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(px(14), px(10)),
                row_gap: px(4),
                max_width: px(620),
                border: UiRect::all(px(2)),
                ..default()
            },
            BackgroundColor(palette::PANEL.with_alpha(0.0)),
            BorderColor::all(palette::PANEL_BORDER.with_alpha(0.0)),
            Visibility::Hidden,
            children![
                (
                    Speaker,
                    Text::new(""),
                    font(15.0),
                    TextColor(palette::BANNER)
                ),
                (Line, Text::new(""), font(15.0), TextColor(palette::TEXT)),
            ],
        ));
}

fn spawn_fade(mut commands: Commands) {
    commands.spawn((
        ZoneFade(ZONE_FADE),
        Node {
            position_type: PositionType::Absolute,
            width: percent(100),
            height: percent(100),
            ..default()
        },
        BackgroundColor(Color::BLACK.with_alpha(0.0)),
        GlobalZIndex(50),
        // The fade never blocks the mouse.
        bevy::ui::FocusPolicy::Pass,
    ));
}

fn show_speech(
    mut received: MessageReader<Received>,
    me: Res<LocalPlayerId>,
    mut speech: Single<(&mut SpeechBox, &mut Visibility)>,
    mut speaker: Single<&mut Text, (With<Speaker>, Without<Line>)>,
    mut line: Single<&mut Text, (With<Line>, Without<Speaker>)>,
) {
    for Received(event) in received.read() {
        let ServerEvent::Speech {
            player,
            speaker: name,
            text,
        } = event
        else {
            continue;
        };
        if *player != me.0 {
            continue;
        }
        speaker.0 = name.clone();
        line.0 = text.clone();
        let (state, visibility) = &mut *speech;
        state.age = 0.0;
        state.life = BASE_LIFE + text.chars().count() as f32 * LIFE_PER_CHARACTER;
        **visibility = Visibility::Inherited;
    }
}

/// How visible the box is `age` seconds into a `life`-second line.
fn speech_alpha(age: f32, life: f32) -> f32 {
    if age >= life {
        return 0.0;
    }
    (age / FADE).min(1.0).min((life - age) / FADE)
}

fn fade_speech(
    time: Res<Time>,
    mut speech: Single<(
        &mut SpeechBox,
        &mut Visibility,
        &mut BackgroundColor,
        &mut BorderColor,
    )>,
    mut texts: Query<(&mut TextColor, Has<Speaker>), Or<(With<Speaker>, With<Line>)>>,
) {
    let (state, visibility, background, border) = &mut *speech;
    if state.life <= 0.0 {
        return;
    }
    state.age += time.delta_secs();
    let alpha = speech_alpha(state.age, state.life);
    if alpha <= 0.0 {
        **visibility = Visibility::Hidden;
        state.life = 0.0;
        return;
    }
    background.0 = palette::PANEL.with_alpha(0.9 * alpha);
    *border.as_mut() = BorderColor::all(palette::PANEL_BORDER.with_alpha(alpha));
    for (mut color, is_speaker) in &mut texts {
        let base = if is_speaker {
            palette::BANNER
        } else {
            palette::TEXT
        };
        color.0 = base.with_alpha(alpha);
    }
}

/// Arriving in a new zone: start black and clear, hiding the scenery
/// being built.
fn fade_between_zones(
    time: Res<Time>,
    current: Res<CurrentZone>,
    mut fade: Single<(&mut ZoneFade, &mut BackgroundColor)>,
) {
    let (state, color) = &mut *fade;
    if current.is_changed() {
        state.0 = 0.0;
    }
    state.0 = (state.0 + time.delta_secs()).min(ZONE_FADE);
    let alpha = 1.0 - state.0 / ZONE_FADE;
    color.0 = Color::BLACK.with_alpha(alpha * alpha);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speech_fades_in_holds_and_fades_out() {
        assert_eq!(speech_alpha(0.0, 5.0), 0.0);
        assert_eq!(speech_alpha(2.0, 5.0), 1.0);
        assert!(speech_alpha(4.8, 5.0) < 1.0);
        assert_eq!(speech_alpha(5.0, 5.0), 0.0);
    }
}
