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
pub mod paths;
pub mod plan;
pub mod profile;

pub use error::{Error, Result};
