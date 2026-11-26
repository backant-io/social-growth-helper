//! Phase 6: the closed learning loop plus data export and wipe.
//!
//! Additive by design: this module owns its tables (`learning_runs`, `learning_changelog`, extra columns on
//! `learning_insights`) and reads other phases' tables only through plain SQL. Nothing here writes to X.

pub mod analyzers;
pub mod commands;
pub mod outcomes;
pub mod patch;
pub mod reflection;
pub mod store;
pub mod textdiff;
pub mod transfer;
pub mod versions;
pub mod wipe;
