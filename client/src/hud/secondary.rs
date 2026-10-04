//! The secondary flame picker, inside the lantern panel (L): choose a
//! second class for your current one, and two of its abilities to borrow
//! (hotbar slots 9 and 0). Each class remembers its own choice.

use bevy::prelude::*;
use shared::classes::{CurrentClass, Secondaries, SecondaryChoice};
use shared::gamedata::GameData;
use shared::progression::ClassLevels;
use shared::protocol::{ClientRequest, Link};

use super::lantern::{LanternPanel, SecondarySection};
use super::{font, palette};
use crate::characters::{LocalPlayer, flame_look};
use crate::session::{LocalPlayerId, send};

pub struct SecondaryPlugin;

impl Plugin for SecondaryPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Update, (rebuild_picker, click_picker).chain());
    }
}

#[derive(Component, Clone)]
enum PickerButton {
    /// Borrow from this class (`None`: no secondary class).
    Class(Option<String>),
    /// Put this ability in (or take it out of) the two secondary slots.
    Ability(String),
}

fn button(
    parent: &mut ChildSpawnerCommands,
    kind: PickerButton,
    label: String,
    color: Color,
    lit: bool,
) {
    parent
        .spawn((
            Button,
            kind,
            Node {
                padding: UiRect::axes(px(6), px(3)),
                border: UiRect::all(px(if lit { 2 } else { 1 })),
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.35)),
            BorderColor::all(if lit {
                color
            } else {
                palette::PANEL_BORDER.with_alpha(0.5)
            }),
        ))
        .with_child((Text::new(label), font(12.0), TextColor(color)));
}

fn row(parent: &mut ChildSpawnerCommands, build: impl FnOnce(&mut ChildSpawnerCommands)) {
    parent
        .spawn(Node {
            flex_wrap: FlexWrap::Wrap,
            column_gap: px(6),
            row_gap: px(4),
            ..default()
        })
        .with_children(build);
}

/// Redraw the picker when the panel opens or the choice changes.
fn rebuild_picker(
    mut commands: Commands,
    panel: Res<LanternPanel>,
    data: Res<GameData>,
    player: Option<
        Single<(Ref<CurrentClass>, Ref<ClassLevels>, Ref<Secondaries>), With<LocalPlayer>>,
    >,
    section: Single<Entity, With<SecondarySection>>,
    mut was_open: Local<bool>,
) {
    let opened = panel.open && !*was_open;
    *was_open = panel.open;
    let Some(player) = player else {
        return;
    };
    let (class, levels, secondaries) = player.into_inner();
    let changed = class.is_changed() || levels.is_changed() || secondaries.is_changed();
    if !panel.open || !(opened || changed) {
        return;
    }
    let main = class.class.as_str();
    let main_name = data.classes.get(main).map_or(main, |c| c.name.as_str());
    let choice = secondaries.0.get(main);
    commands.entity(*section).despawn_children();
    commands.entity(*section).with_children(|section| {
        section.spawn((
            Text::new(format!("Secondary flame for {main_name}")),
            font(16.0),
            TextColor(palette::TEXT),
        ));
        section.spawn((
            Text::new(
                "Borrow two abilities from another class (hotbar 9 and 0). \
                 Its level also adds a little health and power.",
            ),
            font(11.0),
            TextColor(palette::TEXT_DIM),
        ));
        row(section, |r| {
            button(
                r,
                PickerButton::Class(None),
                "None".to_owned(),
                palette::TEXT,
                choice.is_none(),
            );
            for (id, def) in data.class_list() {
                if id == main {
                    continue;
                }
                let (color, _) = flame_look(&def.flame);
                let lit = choice.is_some_and(|c| &c.class == id);
                let label = format!("{}  (level {})", def.name, levels.get(id).level);
                button(r, PickerButton::Class(Some(id.clone())), label, color, lit);
            }
        });
        let Some((lender_id, lender)) = choice.and_then(|c| {
            data.classes
                .get(&c.class)
                .map(|def| (c.class.as_str(), def))
        }) else {
            return;
        };
        let lender_level = levels.get(lender_id).level;
        row(section, |r| {
            for lend in &lender.lendable {
                let name = data
                    .abilities
                    .get(&lend.ability)
                    .map_or(lend.ability.as_str(), |a| a.name.as_str());
                let chosen = choice
                    .is_some_and(|c| c.abilities.iter().flatten().any(|a| *a == lend.ability));
                let (label, color) = if lend.level > lender_level {
                    (
                        format!("{name}  (needs {} level {})", lender.name, lend.level),
                        palette::TEXT_DIM,
                    )
                } else {
                    (name.to_owned(), palette::HEAL)
                };
                button(
                    r,
                    PickerButton::Ability(lend.ability.clone()),
                    label,
                    color,
                    chosen,
                );
            }
        });
    });
}

fn click_picker(
    buttons: Query<(&Interaction, &PickerButton), Changed<Interaction>>,
    player: Option<Single<(&CurrentClass, &Secondaries), With<LocalPlayer>>>,
    me: Res<LocalPlayerId>,
    mut link: ResMut<Link>,
) {
    let Some(player) = player else {
        return;
    };
    let (class, secondaries) = *player;
    for (interaction, button) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let current = secondaries.0.get(&class.class);
        let choice = match button {
            PickerButton::Class(None) => None,
            PickerButton::Class(Some(lender)) => {
                // Picking the same class again keeps its abilities.
                match current.filter(|c| &c.class == lender) {
                    Some(same) => Some(same.clone()),
                    None => Some(SecondaryChoice {
                        class: lender.clone(),
                        abilities: [None, None],
                    }),
                }
            }
            PickerButton::Ability(ability) => {
                let Some(mut choice) = current.cloned() else {
                    continue;
                };
                toggle(&mut choice, ability);
                Some(choice)
            }
        };
        send(&mut link, *me, ClientRequest::SetSecondary { choice });
    }
}

/// Take an ability out if it's chosen; otherwise put it in the first free
/// slot (or replace the second one when both are full).
fn toggle(choice: &mut SecondaryChoice, ability: &str) {
    if let Some(slot) = choice
        .abilities
        .iter_mut()
        .find(|a| a.as_deref() == Some(ability))
    {
        *slot = None;
        return;
    }
    let slot = choice
        .abilities
        .iter()
        .position(Option::is_none)
        .unwrap_or(choice.abilities.len() - 1);
    choice.abilities[slot] = Some(ability.to_owned());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggling_fills_then_replaces_then_removes() {
        let mut choice = SecondaryChoice {
            class: "x".into(),
            abilities: [None, None],
        };
        toggle(&mut choice, "a");
        toggle(&mut choice, "b");
        assert_eq!(choice.abilities, [Some("a".into()), Some("b".into())]);
        toggle(&mut choice, "c");
        assert_eq!(choice.abilities, [Some("a".into()), Some("c".into())]);
        toggle(&mut choice, "a");
        assert_eq!(choice.abilities, [None, Some("c".into())]);
    }
}
