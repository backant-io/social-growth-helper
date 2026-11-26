//! Phase 4: X Analytics ingestor, read-only widget data sandbox, starter dashboard, and reprompt builder.
//!
//! Designed as a self-contained additive module so Phase 2 (`llm/`, `style/`) and Phase 3
//! (`scout/`, `draft/`, `publish/`) merge cleanly without shared-file churn.

pub mod builder;
pub mod commands;
pub mod csv;
pub mod dashboard;
pub mod extract;
pub mod health;
pub mod ingest;
pub mod sandbox;
pub mod schedule;
pub mod spec;
pub mod starter;
pub mod views;
