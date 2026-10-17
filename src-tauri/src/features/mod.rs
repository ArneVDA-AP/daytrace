//! Features added for Dayflow parity. Each module owns its tables (`SCHEMA`, applied at startup
//! with CREATE ... IF NOT EXISTS) and its Tauri commands, so features stay independent.

pub mod chat;
pub mod insights;
pub mod journal;
pub mod platform_extras;

/// Schemas of all feature modules, applied by `Db::open` after the core schema.
pub const SCHEMAS: &[&str] = &[insights::SCHEMA, chat::SCHEMA, journal::SCHEMA, platform_extras::SCHEMA];
