//! GitMate core library.
//!
//! Profiles, effective Git identity inspection, safe configuration changes
//! with preview and rollback, and diagnostics. Git itself is the source of
//! truth for configuration precedence; this crate never parses Git config
//! files.

pub mod config;
pub mod error;
pub mod fsutil;
pub mod git;
pub mod identity;
pub mod includes;
pub mod journal;
pub mod paths;
pub mod pending;
pub mod plan;
pub mod profile;
pub mod remote;
pub mod repo;
pub mod rules;
pub mod ssh;

pub use error::{Error, Result};
