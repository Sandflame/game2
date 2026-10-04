//! Lanternflame shared crate: game rules, data definitions and (later) the
//! network protocol. Everything here is used by both the server and the
//! client, and nothing here knows about rendering.

pub mod config;
pub mod data;
pub mod level;
pub mod movement;
