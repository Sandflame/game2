//! Threat ("aggro"): each enemy remembers how much each character has
//! bothered it, and attacks whoever is on top.

use std::collections::HashMap;

use bevy::prelude::{Component, Entity};

#[derive(Component, Debug, Clone, Default)]
pub struct ThreatTable(pub HashMap<Entity, f32>);

impl ThreatTable {
    pub fn add(&mut self, who: Entity, amount: f32) {
        *self.0.entry(who).or_insert(0.0) += amount.max(0.0);
    }

    /// The character with the most threat (ties broken by entity order so
    /// the result never flickers).
    pub fn top(&self) -> Option<Entity> {
        self.0
            .iter()
            .max_by(|a, b| a.1.total_cmp(b.1).then(b.0.cmp(a.0)))
            .map(|(e, _)| *e)
    }

    /// Put `who` at the top of the list (just above the current leader).
    pub fn taunt(&mut self, who: Entity) {
        let highest = self.0.values().copied().fold(0.0, f32::max);
        self.0.insert(who, highest + 1.0);
    }

    pub fn forget(&mut self, who: Entity) {
        self.0.remove(&who);
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entity(n: u32) -> Entity {
        Entity::from_raw_u32(n).unwrap()
    }

    #[test]
    fn most_threat_is_on_top() {
        let mut t = ThreatTable::default();
        assert_eq!(t.top(), None);
        t.add(entity(1), 100.0);
        t.add(entity(2), 300.0);
        t.add(entity(1), 150.0);
        assert_eq!(t.top(), Some(entity(2)));
    }

    #[test]
    fn taunt_takes_the_top_spot() {
        let mut t = ThreatTable::default();
        t.add(entity(1), 5000.0);
        t.taunt(entity(2));
        assert_eq!(t.top(), Some(entity(2)));
    }

    #[test]
    fn forgetting_removes_from_the_list() {
        let mut t = ThreatTable::default();
        t.add(entity(1), 10.0);
        t.add(entity(2), 5.0);
        t.forget(entity(1));
        assert_eq!(t.top(), Some(entity(2)));
        t.clear();
        assert!(t.is_empty());
    }
}
