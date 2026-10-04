//! Lanternflame shared crate: game rules, data definitions and the
//! messages between the rules half and the screen half. Everything here is
//! used by both, and nothing here knows about rendering.

pub mod abilities;
pub mod classes;
pub mod combat;
pub mod components;
pub mod config;
pub mod data;
pub mod describe;
pub mod formulas;
pub mod gamedata;
pub mod level;
pub mod movement;
pub mod protocol;
pub mod statuses;
pub mod targeting;
pub mod threat;
