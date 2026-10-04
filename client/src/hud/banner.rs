//! Big centred messages for important moments: boss speech, a fight
//! starting, victory, wipes, entering a zone, being defeated in a trial.
//! Also the "E: Enter …" prompt when standing in a portal.

use bevy::prelude::*;
use shared::components::{Motion, Zone};
use shared::gamedata::Zones;
use shared::protocol::ServerEvent;

use super::{font, palette, text_shadow};
use crate::characters::LocalPlayer;
use crate::session::Received;
use crate::world::CurrentZone;

/// Matches the shadow alpha set by `text_shadow()`.
const SHADOW_ALPHA: f32 = 0.85;

pub struct BannerPlugin;

impl Plugin for BannerPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, (spawn_banner, spawn_prompt))
            .add_systems(Update, (show_banners, fade_banner, update_prompt).chain());
    }
}

#[derive(Component)]
struct Banner {
    age: f32,
    life: f32,
}

#[derive(Component)]
struct Prompt;

fn spawn_banner(mut commands: Commands) {
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            top: percent(20),
            width: percent(100),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_child((
            Banner {
                age: 1.0,
                life: 0.0,
            },
            Text::new(""),
            font(30.0),
            TextColor(palette::BANNER),
            TextLayout::justify(Justify::Center),
            text_shadow(),
            Visibility::Hidden,
        ));
}

fn spawn_prompt(mut commands: Commands) {
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: px(140),
            width: percent(100),
            justify_content: JustifyContent::Center,
            ..default()
        })
        .with_child((
            Prompt,
            Text::new(""),
            font(18.0),
            TextColor(palette::TEXT),
            text_shadow(),
            Visibility::Hidden,
        ));
}

/// Seconds a banner stays up.
const SHORT: f32 = 3.0;
const LONG: f32 = 5.0;

fn show_banners(
    mut received: MessageReader<Received>,
    current: Res<CurrentZone>,
    zones: Res<Zones>,
    me: Option<Single<Entity, With<LocalPlayer>>>,
    mut banner: Single<(&mut Text, &mut TextColor, &mut Banner)>,
) {
    let me = me.map(|e| *e);
    let here = |zone: &String| current.0.as_ref() == Some(zone);
    for Received(event) in received.read() {
        let shown: Option<(String, Color, f32)> = match event {
            ServerEvent::Announce { zone, text } if here(zone) => {
                Some((text.clone(), palette::BANNER, LONG))
            }
            ServerEvent::EncounterStarted { zone, name } if here(zone) => {
                Some((name.clone(), palette::WARNING, SHORT))
            }
            ServerEvent::EncounterWon {
                zone,
                name,
                seconds,
            } if here(zone) => {
                let total = seconds.round() as u32;
                Some((
                    format!(
                        "Victory!\n{name} defeated in {}:{:02}",
                        total / 60,
                        total % 60
                    ),
                    palette::BANNER,
                    LONG + 2.0,
                ))
            }
            ServerEvent::EncounterWiped { zone, .. } if here(zone) => Some((
                "Everyone has fallen...\nReturning to the entrance.".to_owned(),
                palette::WARNING,
                LONG,
            )),
            ServerEvent::ZoneChanged { entity, zone } if Some(*entity) == me => zones
                .get(zone)
                .map(|level| (level.name.clone(), palette::TEXT, SHORT)),
            ServerEvent::Defeated { entity } if Some(*entity) == me => {
                let revives_here = current
                    .0
                    .as_ref()
                    .and_then(|z| zones.get(z))
                    .is_none_or(|level| level.revive_in_place);
                (!revives_here).then(|| {
                    (
                        "You have fallen.\nA friend can Rekindle you.".to_owned(),
                        palette::WARNING,
                        LONG,
                    )
                })
            }
            _ => None,
        };
        if let Some((text, color, life)) = shown {
            let (banner_text, banner_color, state) = &mut *banner;
            banner_text.0 = text;
            banner_color.0 = color;
            state.age = 0.0;
            state.life = life;
        }
    }
}

fn fade_banner(
    time: Res<Time>,
    mut banner: Single<(
        &mut TextColor,
        &mut TextShadow,
        &mut Visibility,
        &mut Banner,
    )>,
) {
    let (color, shadow, visibility, state) = &mut *banner;
    state.age += time.delta_secs();
    let alpha = banner_alpha(state.age, state.life);
    color.0.set_alpha(alpha);
    shadow.color.set_alpha(alpha * SHADOW_ALPHA);
    **visibility = if alpha > 0.0 {
        Visibility::Inherited
    } else {
        Visibility::Hidden
    };
}

/// Fades in quickly, holds, then fades out over the last second.
fn banner_alpha(age: f32, life: f32) -> f32 {
    if age >= life {
        return 0.0;
    }
    let fade_in = (age / 0.25).min(1.0);
    let fade_out = ((life - age) / 1.0).min(1.0);
    fade_in.min(fade_out).clamp(0.0, 1.0)
}

/// "E  Enter the Rootwarden's Hollow" while standing in a portal.
fn update_prompt(
    zones: Res<Zones>,
    player: Option<Single<(&Motion, &Zone), With<LocalPlayer>>>,
    mut prompt: Single<(&mut Text, &mut Visibility), With<Prompt>>,
) {
    let label = player.and_then(|player| {
        let (motion, zone) = *player;
        zones
            .get(&zone.0)?
            .portal_at(motion.0.position)
            .map(|portal| format!("[E]  {}", portal.label))
    });
    let (text, visibility) = &mut *prompt;
    match label {
        Some(label) => {
            if text.0 != label {
                text.0 = label;
            }
            **visibility = Visibility::Inherited;
        }
        None => **visibility = Visibility::Hidden,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn banners_fade_in_hold_and_fade_out() {
        assert_eq!(banner_alpha(0.0, 4.0), 0.0);
        assert_eq!(banner_alpha(1.0, 4.0), 1.0);
        assert!(banner_alpha(3.5, 4.0) < 1.0);
        assert_eq!(banner_alpha(4.0, 4.0), 0.0);
    }
}
