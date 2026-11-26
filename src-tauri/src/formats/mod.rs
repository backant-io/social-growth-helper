//! Viral format detection (Phase 5): finds posts that over-perform, distils them into reusable format
//! blueprints, and keeps a deduplicated library. Additive: it reads and writes only its own tables and is
//! fed by whichever component sees posts (Scout, a watchlist scan, or a manual add).

pub mod blueprint;
pub mod cluster;
pub mod commands;
pub mod detect;
pub mod extract;
pub mod llm;
pub mod pipeline;
pub mod repo;
pub mod router_llm;
pub mod scan;
