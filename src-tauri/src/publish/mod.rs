//! Publish: the only code that changes anything on X. Every entry point needs an [`gate::Approval`], a value that
//! only `gate` can create and only an explicit user click (a Tauri command in `commands`) asks it to.

pub mod actions;
pub mod commands;
pub mod executor;
pub mod gate;
pub mod pacing;
pub mod queue;
pub mod repo;

#[cfg(test)]
mod fake;
