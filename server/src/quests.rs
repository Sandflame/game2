//! Quests on the rules side: things players do (defeating enemies, going
//! places, winning boss fights) are collected as deeds and recorded in each
//! player's [`QuestLog`]; finished quests hand out their rewards. Talking
//! to people is handled in `travel::talk`, which calls [`talked`].

use bevy::prelude::*;
use shared::components::{PlayerId, Zone};
use shared::gamedata::GameData;
use shared::protocol::{Link, ServerEvent};
use shared::quests::{Deed, Progressed, QuestLog, Talked};

use crate::progression::{PendingRewards, Reward};

/// Things players did this tick that quests may count.
#[derive(Resource, Default, Debug)]
pub struct PendingDeeds(pub Vec<(Entity, Deed)>);

/// Arriving in a zone counts for `Reach` steps.
pub fn note_arrivals(
    mut deeds: ResMut<PendingDeeds>,
    arrived: Query<(Entity, &Zone), (With<PlayerId>, Changed<Zone>)>,
) {
    for (entity, zone) in &arrived {
        deeds.0.push((entity, Deed::Reached(zone.0.clone())));
    }
}

/// Record this tick's deeds in quest logs.
pub fn record_deeds(
    data: Res<GameData>,
    mut deeds: ResMut<PendingDeeds>,
    mut rewards: ResMut<PendingRewards>,
    mut link: ResMut<Link>,
    mut players: Query<(&PlayerId, &Zone, &mut QuestLog)>,
) {
    for (entity, deed) in std::mem::take(&mut deeds.0) {
        let Ok((&player, zone, mut log)) = players.get_mut(entity) else {
            continue;
        };
        let changes = log.record(&data.quests, &deed);
        let here = zone.0.clone();
        report(
            &data,
            &mut link,
            &mut rewards,
            player,
            entity,
            &mut log,
            &here,
            changes,
        );
    }
}

/// Tell the player about quest changes and reward finished quests.
fn report(
    data: &GameData,
    link: &mut Link,
    rewards: &mut PendingRewards,
    player: PlayerId,
    entity: Entity,
    log: &mut QuestLog,
    here: &str,
    changes: Vec<Progressed>,
) {
    let mut moved_on = false;
    for change in changes {
        match change {
            Progressed::Step { quest } => {
                moved_on = true;
                link.to_client
                    .push(ServerEvent::QuestProgressed { player, quest });
            }
            Progressed::Finished { quest } => finish(data, link, rewards, player, entity, quest),
        }
    }
    // A step that is already done where the player stands counts at once
    // (e.g. "go to the forest" while in the forest).
    if moved_on {
        let more = log.record(&data.quests, &Deed::Reached(here.to_owned()));
        if !more.is_empty() {
            report(data, link, rewards, player, entity, log, here, more);
        }
    }
}

fn finish(
    data: &GameData,
    link: &mut Link,
    rewards: &mut PendingRewards,
    player: PlayerId,
    entity: Entity,
    quest: String,
) {
    if let Some(def) = data.quests.get(&quest) {
        rewards.0.push(Reward {
            players: vec![entity],
            xp: def.xp,
            loot: def.items.clone(),
        });
    }
    link.to_client
        .push(ServerEvent::QuestCompleted { player, quest });
}

/// The player talked to someone. Returns false if no quest cared, so
/// they just chat instead.
pub fn talked(
    data: &GameData,
    link: &mut Link,
    rewards: &mut PendingRewards,
    player: PlayerId,
    entity: Entity,
    here: &str,
    log: &mut QuestLog,
    npc: &str,
) -> bool {
    let Some(outcome) = log.talk(&data.quests, npc) else {
        return false;
    };
    match outcome {
        Talked::Accepted { quest, dialogue } => {
            link.to_client
                .push(ServerEvent::Dialogue { player, dialogue });
            link.to_client.push(ServerEvent::QuestAccepted {
                player,
                quest: quest.clone(),
            });
            let changes = log.record(&data.quests, &Deed::Reached(here.to_owned()));
            report(data, link, rewards, player, entity, log, here, changes);
        }
        Talked::Step {
            quest,
            dialogue,
            finished,
        } => {
            if let Some(dialogue) = dialogue {
                link.to_client
                    .push(ServerEvent::Dialogue { player, dialogue });
            }
            if finished {
                finish(data, link, rewards, player, entity, quest);
            } else {
                link.to_client
                    .push(ServerEvent::QuestProgressed { player, quest });
                let changes = log.record(&data.quests, &Deed::Reached(here.to_owned()));
                report(data, link, rewards, player, entity, log, here, changes);
            }
        }
    }
    true
}
