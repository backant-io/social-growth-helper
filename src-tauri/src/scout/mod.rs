//! Scout: reads the user's own X feed in the visible window, scores posts for reply potential and fills
//! the reply queue. Read-only against X; it can never act (see `crate::guard`).

pub mod commands;
pub mod filter;
pub mod model;
pub mod outlier;
pub mod repo;
pub mod score;
pub mod session;
