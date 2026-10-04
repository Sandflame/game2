//! The 10-slot hotbar: keys 1–0 or clicking use an ability on the current
//! target. Slots show the GCD sweep, cooldowns, range and queued actions,
//! and a tooltip on hover.

use bevy::prelude::*;
use shared::combat::{AbilityDef, ActionState, TargetKind, in_range};
use shared::components::{HOTBAR_SLOTS, HitRadius, Hotbar, Motion};
use shared::gamedata::GameData;
use shared::protocol::{ClientRequest, Link};

use super::{font, game_now, palette};
use crate::characters::LocalPlayer;
use crate::session::{LocalPlayerId, send};
use crate::targeting::CurrentTarget;

const SLOT_SIZE: f32 = 58.0;
const KEYS: [KeyCode; HOTBAR_SLOTS] = [
    KeyCode::Digit1,
    KeyCode::Digit2,
    KeyCode::Digit3,
    KeyCode::Digit4,
    KeyCode::Digit5,
    KeyCode::Digit6,
    KeyCode::Digit7,
    KeyCode::Digit8,
    KeyCode::Digit9,
    KeyCode::Digit0,
];
const KEY_LABELS: [&str; HOTBAR_SLOTS] = ["1", "2", "3", "4", "5", "6", "7", "8", "9", "0"];

pub struct HotbarPlugin;

impl Plugin for HotbarPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, spawn_hotbar)
            .add_systems(Update, (press_slots, update_slots, update_tooltip).chain());
    }
}

#[derive(Component)]
struct Slot {
    index: usize,
    /// Brief highlight after pressing.
    flash: f32,
}
#[derive(Component)]
struct SlotName(usize);
#[derive(Component)]
struct SlotShade(usize);
#[derive(Component)]
struct SlotTimer(usize);
#[derive(Component)]
struct Tooltip;
#[derive(Component)]
struct TooltipText;

fn spawn_hotbar(mut commands: Commands) {
    commands
        .spawn(Node {
            position_type: PositionType::Absolute,
            bottom: px(20),
            width: percent(100),
            justify_content: JustifyContent::Center,
            column_gap: px(6),
            ..default()
        })
        .with_children(|bar| {
            for (index, key_label) in KEY_LABELS.into_iter().enumerate() {
                bar.spawn((
                    Button,
                    Slot { index, flash: 0.0 },
                    Node {
                        width: px(SLOT_SIZE),
                        height: px(SLOT_SIZE),
                        border: UiRect::all(px(2)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    BackgroundColor(palette::PANEL),
                    BorderColor::all(palette::PANEL_BORDER),
                ))
                .with_children(|slot| {
                    // Cooldown shade: a dark panel that shrinks as the cooldown runs out.
                    slot.spawn((
                        SlotShade(index),
                        Node {
                            position_type: PositionType::Absolute,
                            bottom: px(0),
                            left: px(0),
                            width: percent(100),
                            height: percent(0),
                            ..default()
                        },
                        BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.65)),
                    ));
                    slot.spawn((
                        SlotName(index),
                        Text::new(""),
                        font(11.0),
                        TextColor(palette::TEXT),
                        TextLayout::justify(Justify::Center),
                    ));
                    slot.spawn((
                        SlotTimer(index),
                        Text::new(""),
                        font(22.0),
                        TextColor(palette::TEXT),
                        super::text_shadow(),
                        Node {
                            position_type: PositionType::Absolute,
                            ..default()
                        },
                    ));
                    slot.spawn((
                        Text::new(key_label),
                        font(11.0),
                        TextColor(palette::TEXT_DIM),
                        Node {
                            position_type: PositionType::Absolute,
                            top: px(1),
                            left: px(3),
                            ..default()
                        },
                    ));
                });
            }
        });

    commands
        .spawn((
            Tooltip,
            Node {
                position_type: PositionType::Absolute,
                bottom: px(SLOT_SIZE + 80.0),
                right: px(20),
                max_width: px(300),
                padding: UiRect::all(px(8)),
                border: UiRect::all(px(1)),
                ..default()
            },
            BackgroundColor(palette::PANEL),
            BorderColor::all(palette::PANEL_BORDER),
            Visibility::Hidden,
        ))
        .with_child((
            TooltipText,
            Text::new(""),
            font(13.0),
            TextColor(palette::TEXT),
        ));
}

/// Number keys or clicks send "use the ability in this slot" requests.
fn press_slots(
    keys: Res<ButtonInput<KeyCode>>,
    mut slots: Query<(Ref<Interaction>, &mut Slot)>,
    target: Res<CurrentTarget>,
    me: Res<LocalPlayerId>,
    mut link: ResMut<Link>,
) {
    let mut pressed: Vec<usize> = KEYS
        .iter()
        .enumerate()
        .filter(|(_, key)| keys.just_pressed(**key))
        .map(|(index, _)| index)
        .collect();
    for (interaction, slot) in &slots {
        if interaction.is_changed() && *interaction == Interaction::Pressed {
            pressed.push(slot.index);
        }
    }
    for index in pressed {
        send(
            &mut link,
            *me,
            ClientRequest::UseAbility {
                slot: index,
                target: target.0,
            },
        );
        for (_, mut slot) in &mut slots {
            if slot.index == index {
                slot.flash = 1.0;
            }
        }
    }
}

fn update_slots(
    time: Res<Time>,
    fixed: Res<Time<Fixed>>,
    data: Res<GameData>,
    target: Res<CurrentTarget>,
    player: Option<Single<(&Hotbar, &ActionState, &Motion), With<LocalPlayer>>>,
    targets: Query<(&Motion, &HitRadius)>,
    mut slots: Query<(&mut Slot, &mut BorderColor)>,
    mut names: Query<(&SlotName, &mut Text, &mut TextColor), Without<SlotTimer>>,
    mut shades: Query<(&SlotShade, &mut Node)>,
    mut timers: Query<(&SlotTimer, &mut Text), Without<SlotName>>,
) {
    let Some(player) = player else {
        return;
    };
    let (hotbar, actions, motion) = *player;
    let now = game_now(&fixed);
    let ability_in = |index: usize| -> Option<&AbilityDef> {
        hotbar
            .0
            .get(index)?
            .as_ref()
            .and_then(|id| data.abilities.get(id))
    };
    let target_place = target.0.and_then(|t| targets.get(t).ok());

    for (mut slot, mut border) in &mut slots {
        slot.flash = (slot.flash - time.delta_secs() * 5.0).max(0.0);
        let queued = ability_in(slot.index)
            .is_some_and(|a| actions.queued.as_ref().is_some_and(|q| q.ability == a.id));
        let base = if queued {
            palette::QUEUED
        } else {
            palette::PANEL_BORDER
        };
        *border = BorderColor::all(base.mix(&Color::WHITE, slot.flash));
    }

    for (name, mut text, mut color) in &mut names {
        let ability = ability_in(name.0);
        text.0 = ability.map(|a| a.name.clone()).unwrap_or_default();
        let out_of_range = ability.is_some_and(|a| {
            a.target == TargetKind::Enemy
                && target_place.is_some_and(|(target_motion, radius)| {
                    !in_range(
                        motion.0.position,
                        target_motion.0.position,
                        radius.0,
                        a.range,
                    )
                })
        });
        color.0 = if out_of_range {
            palette::WARNING
        } else {
            palette::TEXT
        };
    }

    for (shade, mut node) in &mut shades {
        let fraction = ability_in(shade.0).map_or(0.0, |a| {
            let gcd = if a.on_gcd {
                actions.gcd_remaining_fraction(now).unwrap_or(0.0)
            } else {
                0.0
            };
            let own = actions
                .cooldown_remaining_fraction(&a.id, now)
                .unwrap_or(0.0);
            gcd.max(own)
        });
        node.height = percent(fraction * 100.0);
    }

    for (timer, mut text) in &mut timers {
        text.0 = ability_in(timer.0)
            .and_then(|a| actions.cooldown_remaining(&a.id, now))
            .map(|seconds| format!("{}", seconds.ceil() as u32))
            .unwrap_or_default();
    }
}

/// Show the hovered ability's details.
fn update_tooltip(
    data: Res<GameData>,
    player: Option<Single<&Hotbar, With<LocalPlayer>>>,
    slots: Query<(&Slot, &Interaction)>,
    mut tooltip: Single<&mut Visibility, With<Tooltip>>,
    mut text: Single<&mut Text, With<TooltipText>>,
) {
    let hovered = slots
        .iter()
        .find(|(_, interaction)| **interaction != Interaction::None)
        .map(|(slot, _)| slot.index);
    let ability = player.and_then(|hotbar| {
        hovered
            .and_then(|index| hotbar.0.get(index).cloned().flatten())
            .and_then(|id| data.abilities.get(&id))
    });
    match ability {
        Some(a) => {
            **tooltip = Visibility::Visible;
            let cast = if a.is_instant() {
                "Instant".to_owned()
            } else {
                format!("Cast {:.1}s", a.cast_time)
            };
            let recast = if a.on_gcd {
                "Global cooldown".to_owned()
            } else {
                format!("Cooldown {:.0}s", a.cooldown)
            };
            text.0 = format!(
                "{}\n{} · {} · Range {:.0}m · Potency {}\n\n{}",
                a.name, cast, recast, a.range, a.potency, a.description
            );
        }
        None => **tooltip = Visibility::Hidden,
    }
}
