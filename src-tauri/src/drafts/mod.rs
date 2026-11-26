//! Reply drafts: variants, editing, bundles and pre-publish checks. Nothing here can publish; the
//! only way to post is `publish::gate`, which needs an approval created by an explicit user click.

pub mod bundle;
pub mod commands;
pub mod edit;
pub mod generate;
pub mod repo;
