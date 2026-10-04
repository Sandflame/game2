//! Experience, levels, gear and saving, on the rules side:
//! - defeated enemies give experience to everyone who fought them,
//! - won boss fights give experience and loot,
//! - players equip, unequip and discard items,
//! - stats follow class, level, gear and the zone's level sync,
//! - characters are loaded when they join and saved when things change.

use std::collections::HashSet;

use bevy::prelude::*;
use shared::classes::{CurrentClass, Stats};
use shared::combat::{Health, Reject};
use shared::components::{CharacterName, Motion, PlayerId, Zone};
use shared::gamedata::{GameData, Zones};
use shared::items::{
    Bag, Equipment, ItemDef, LootEntry, Slot, character_stats, discard, equip, roll_loot, unequip,
};
use shared::progression::{ClassLevels, effective_level};
use shared::protocol::{Link, ServerEvent};
use shared::threat::ThreatTable;

use crate::CombatRng;
use crate::characters::{CombatClock, Defeated};
use crate::database::{CharacterSave, Database, SavedItem};
use crate::enemies::EnemyKind;

/// Experience (and maybe loot) waiting to be handed out.
#[derive(Debug, Clone)]
pub struct Reward {
    pub players: Vec<Entity>,
    pub xp: u32,
    pub loot: Vec<LootEntry>,
}

#[derive(Resource, Default, Debug)]
pub struct PendingRewards(pub Vec<Reward>);

/// Item requests waiting to be handled this tick.
#[derive(Debug, Clone, Copy)]
pub enum GearRequest {
    Equip(u64),
    Unequip(Slot),
    Discard(u64),
}

#[derive(Resource, Default, Debug)]
pub struct PendingGear(pub Vec<(PlayerId, Entity, GearRequest)>);

/// When characters were last saved automatically.
#[derive(Resource, Default, Debug)]
pub struct LastAutosave(pub f64);

/// The gear a brand-new character starts with: everything in
/// `start_items`, worn where it fits (each class's weapon on that class).
pub fn starting_gear(data: &GameData) -> (Bag, Equipment) {
    let mut bag = Bag::default();
    let mut worn = Equipment::default();
    for item in &data.player.start_items {
        let Ok(id) = bag.add(item, data.progression.bag_size) else {
            break;
        };
        let Some(def) = data.items.get(item) else {
            continue;
        };
        let class = def.class.as_deref().unwrap_or(&data.player.start_class);
        // Starting gear is level 1, so this only fails for odd data.
        let _ = equip(&bag, &mut worn, &data.items, id, class, def.level);
    }
    (bag, worn)
}

/// Rebuild a saved character's levels, bag and worn gear. Items that no
/// longer exist in the data files are dropped.
pub fn restore(save: &CharacterSave, data: &GameData) -> (ClassLevels, Bag, Equipment) {
    let levels = ClassLevels(
        save.levels
            .iter()
            .filter(|(class, _)| data.classes.contains_key(class))
            .map(|(class, progress)| {
                let mut progress = *progress;
                progress.level = progress.level.clamp(1, data.progression.max_level);
                (class.clone(), progress)
            })
            .collect(),
    );
    let mut bag = Bag::default();
    let mut worn = Equipment::default();
    for saved in &save.items {
        let Some(def) = data.items.get(&saved.item) else {
            continue;
        };
        let Ok(id) = bag.add(&saved.item, data.progression.bag_size) else {
            break;
        };
        match saved.worn.as_deref() {
            None => {}
            Some(key) => match key.strip_prefix("weapon:") {
                Some(class) if def.slot == Slot::Weapon && def.class.as_deref() == Some(class) => {
                    worn.weapons.insert(class.to_owned(), id);
                }
                Some(_) => {}
                None => {
                    if Slot::from_key(key) == Some(def.slot) && def.slot != Slot::Weapon {
                        worn.armour.insert(def.slot, id);
                    }
                }
            },
        }
    }
    (levels, bag, worn)
}

/// What gets written to the save file for one character.
pub fn to_save(
    name: &str,
    class: &CurrentClass,
    zone: &Zone,
    motion: &Motion,
    levels: &ClassLevels,
    bag: &Bag,
    worn: &Equipment,
) -> CharacterSave {
    let mut level_list: Vec<_> = levels
        .0
        .iter()
        .map(|(class, progress)| (class.clone(), *progress))
        .collect();
    level_list.sort_by(|a, b| a.0.cmp(&b.0));
    let items = bag
        .items
        .iter()
        .map(|owned| {
            let armour = worn
                .armour
                .iter()
                .find(|(_, id)| **id == owned.id)
                .map(|(slot, _)| slot.key().to_owned());
            let weapon = worn
                .weapons
                .iter()
                .find(|(_, id)| **id == owned.id)
                .map(|(class, _)| format!("weapon:{class}"));
            SavedItem {
                item: owned.item.clone(),
                worn: armour.or(weapon),
            }
        })
        .collect();
    CharacterSave {
        name: name.to_owned(),
        class: class.class.clone(),
        zone: zone.0.clone(),
        position: motion.0.position,
        yaw: motion.0.yaw,
        levels: level_list,
        items,
    }
}

/// A player's stats from their class, level (after the zone's level sync)
/// and the gear their current class has on.
pub fn player_stats(
    data: &GameData,
    zones: &Zones,
    class: &str,
    zone: &str,
    levels: &ClassLevels,
    bag: &Bag,
    worn: &Equipment,
) -> Option<Stats> {
    let def = data.classes.get(class)?;
    let sync = zones.get(zone).and_then(|z| z.level_sync);
    let level = effective_level(levels.get(class).level, sync);
    let items: Vec<&ItemDef> = worn
        .worn_by(class)
        .filter_map(|id| bag.get(id))
        .filter_map(|owned| data.items.get(&owned.item))
        .collect();
    Some(character_stats(
        def,
        level,
        &items,
        &data.progression,
        data.config.combat.crit_chance,
    ))
}

/// Defeated enemies reward everyone on their threat list (anyone who hit
/// them, healed against them, or was attacked by them) in the same zone.
pub fn kill_rewards(
    data: Res<GameData>,
    mut rewards: ResMut<PendingRewards>,
    fallen: Query<(&EnemyKind, &ThreatTable, &Zone), Added<Defeated>>,
    players: Query<&Zone, With<PlayerId>>,
) {
    for (kind, threat, zone) in &fallen {
        let xp = data.enemies.get(&kind.0).map_or(0, |e| e.xp);
        if xp == 0 {
            continue;
        }
        let mut fought: Vec<Entity> = threat
            .0
            .keys()
            .copied()
            .filter(|e| players.get(*e).is_ok_and(|z| z == zone))
            .collect();
        fought.sort();
        if !fought.is_empty() {
            rewards.0.push(Reward {
                players: fought,
                xp,
                loot: Vec::new(),
            });
        }
    }
}

/// Hand out experience and loot.
pub fn grant_rewards(
    data: Res<GameData>,
    mut rewards: ResMut<PendingRewards>,
    mut rng: ResMut<CombatRng>,
    mut link: ResMut<Link>,
    mut players: Query<(
        &PlayerId,
        &CurrentClass,
        &mut ClassLevels,
        &mut Bag,
        &mut Health,
    )>,
) {
    for reward in std::mem::take(&mut rewards.0) {
        for entity in reward.players {
            let Ok((&player, class, mut levels, mut bag, mut health)) = players.get_mut(entity)
            else {
                continue;
            };
            if reward.xp > 0 {
                link.to_client.push(ServerEvent::XpGained {
                    entity,
                    class: class.class.clone(),
                    amount: reward.xp,
                });
                if let Some(level) = levels.add_xp(&class.class, reward.xp, &data.progression) {
                    // A new level also restores health (unless defeated).
                    if !health.is_dead() {
                        health.current = health.max;
                    }
                    link.to_client.push(ServerEvent::LevelUp {
                        entity,
                        class: class.class.clone(),
                        level,
                    });
                }
            }
            for item in roll_loot(&reward.loot, &mut rng.0, true) {
                match bag.add(&item, data.progression.bag_size) {
                    Ok(_) => link
                        .to_client
                        .push(ServerEvent::ItemReceived { entity, item }),
                    Err(reason) => link
                        .to_client
                        .push(ServerEvent::Rejected { player, reason }),
                }
            }
        }
    }
}

/// Equip, unequip and discard requests. Gear can't be changed in combat.
pub fn handle_gear(
    time: Res<Time>,
    data: Res<GameData>,
    mut pending: ResMut<PendingGear>,
    mut link: ResMut<Link>,
    mut players: Query<(
        &CurrentClass,
        &ClassLevels,
        &mut Bag,
        &mut Equipment,
        &CombatClock,
        Has<Defeated>,
    )>,
) {
    let now = time.elapsed_secs_f64();
    for (player, entity, request) in std::mem::take(&mut pending.0) {
        let Ok((class, levels, mut bag, mut worn, clock, defeated)) = players.get_mut(entity)
        else {
            continue;
        };
        let result = if defeated {
            Err(Reject::Dead)
        } else if clock.in_combat(now, data.config.combat.combat_timeout) {
            Err(Reject::InCombat)
        } else {
            match request {
                GearRequest::Equip(id) => {
                    let level = levels.get(&class.class).level;
                    equip(&bag, &mut worn, &data.items, id, &class.class, level)
                }
                GearRequest::Unequip(slot) => {
                    unequip(&mut worn, slot, &class.class);
                    Ok(())
                }
                GearRequest::Discard(id) => discard(&mut bag, &mut worn, id),
            }
        };
        if let Err(reason) = result {
            link.to_client
                .push(ServerEvent::Rejected { player, reason });
        }
    }
}

/// Keep players' stats in step with their class, level, gear and zone.
pub fn refresh_stats(
    data: Res<GameData>,
    zones: Res<Zones>,
    mut players: Query<
        (
            &CurrentClass,
            &Zone,
            &ClassLevels,
            &Bag,
            &Equipment,
            &mut Stats,
            &mut Health,
        ),
        (
            With<PlayerId>,
            Or<(
                Changed<CurrentClass>,
                Changed<Zone>,
                Changed<ClassLevels>,
                Changed<Bag>,
                Changed<Equipment>,
            )>,
        ),
    >,
) {
    for (class, zone, levels, bag, worn, mut stats, mut health) in &mut players {
        let Some(new) = player_stats(&data, &zones, &class.class, &zone.0, levels, bag, worn)
        else {
            continue;
        };
        if *stats != new {
            *stats = new;
            health.set_max(new.max_health);
        }
    }
}

/// Save characters whose progress changed (new level, loot, gear, class,
/// zone), and everyone every so often (for their position).
pub fn save_players(
    time: Res<Time>,
    data: Res<GameData>,
    database: Option<Res<Database>>,
    mut last: ResMut<LastAutosave>,
    changed: Query<
        Entity,
        (
            With<PlayerId>,
            Or<(
                Changed<CurrentClass>,
                Changed<Zone>,
                Changed<ClassLevels>,
                Changed<Bag>,
                Changed<Equipment>,
            )>,
        ),
    >,
    players: Query<
        (
            Entity,
            &CharacterName,
            &CurrentClass,
            &Zone,
            &Motion,
            &ClassLevels,
            &Bag,
            &Equipment,
        ),
        With<PlayerId>,
    >,
) {
    let Some(database) = database else {
        return;
    };
    let now = time.elapsed_secs_f64();
    let everyone = now - last.0 >= f64::from(data.config.simulation.autosave_every);
    if everyone {
        last.0 = now;
    }
    let changed: HashSet<Entity> = changed.iter().collect();
    for (entity, name, class, zone, motion, levels, bag, worn) in &players {
        if everyone || changed.contains(&entity) {
            database.save(to_save(&name.0, class, zone, motion, levels, bag, worn));
        }
    }
}

/// When the game closes, save everyone and wait for the file to be written.
pub fn save_on_exit(
    mut exits: MessageReader<AppExit>,
    database: Option<Res<Database>>,
    players: Query<
        (
            &CharacterName,
            &CurrentClass,
            &Zone,
            &Motion,
            &ClassLevels,
            &Bag,
            &Equipment,
        ),
        With<PlayerId>,
    >,
) {
    if exits.read().next().is_none() {
        return;
    }
    let Some(database) = database else {
        return;
    };
    for (name, class, zone, motion, levels, bag, worn) in &players {
        database.save(to_save(&name.0, class, zone, motion, levels, bag, worn));
    }
    database.flush();
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::movement::MoveState;
    use shared::progression::ClassProgress;

    fn data() -> GameData {
        let assets = shared::data::find_assets_dir().unwrap();
        GameData::load(&assets).unwrap()
    }

    #[test]
    fn new_characters_wear_their_starting_gear() {
        let data = data();
        let (bag, worn) = starting_gear(&data);
        assert_eq!(bag.items.len(), data.player.start_items.len());
        // Every class has its own starting weapon on.
        for class in data.classes.keys() {
            assert!(
                worn.in_slot(Slot::Weapon, class).is_some(),
                "{class} has no weapon"
            );
        }
        assert!(worn.in_slot(Slot::Body, "priest").is_some());
    }

    #[test]
    fn saves_restore_the_same_character() {
        let data = data();
        let (bag, worn) = starting_gear(&data);
        let mut levels = ClassLevels::default();
        levels
            .0
            .insert("priest".into(), ClassProgress { level: 6, xp: 40 });
        let class = CurrentClass {
            class: "priest".into(),
            spec: "mender".into(),
        };
        let save = to_save(
            "Ari",
            &class,
            &Zone("sandbox".into()),
            &Motion(MoveState::spawn_at(Vec3::new(3.0, 0.0, 4.0))),
            &levels,
            &bag,
            &worn,
        );
        let (levels_back, bag_back, worn_back) = restore(&save, &data);
        assert_eq!(levels_back, levels);
        assert_eq!(bag_back.items.len(), bag.items.len());
        // Worn gear comes back on the same items.
        for class in data.classes.keys() {
            let before = worn
                .in_slot(Slot::Weapon, class)
                .map(|id| &bag.get(id).unwrap().item);
            let after = worn_back
                .in_slot(Slot::Weapon, class)
                .map(|id| &bag_back.get(id).unwrap().item);
            assert_eq!(before, after);
        }
    }

    #[test]
    fn unknown_saved_items_are_dropped() {
        let data = data();
        let save = CharacterSave {
            name: "Ari".into(),
            class: "priest".into(),
            zone: "sandbox".into(),
            position: Vec3::ZERO,
            yaw: 0.0,
            levels: vec![("no_such_class".into(), ClassProgress::default())],
            items: vec![
                SavedItem {
                    item: "no_such_item".into(),
                    worn: Some("head".into()),
                },
                SavedItem {
                    item: "bark_helm".into(),
                    worn: Some("feet".into()),
                },
            ],
        };
        let (levels, bag, worn) = restore(&save, &data);
        assert!(levels.0.is_empty());
        assert_eq!(bag.items.len(), 1);
        // A helm saved as worn on the feet is put back in the bag.
        assert!(worn.armour.is_empty());
    }
}
