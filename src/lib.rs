//! Generate accurate, deterministic database context that developers and AI
//! coding agents can trust.
//!
//! `SPEC.md` is the behavior contract for this crate. The public API is built
//! out over the phases described in `CLAUDE.md`; this crate currently carries
//! the canonical schema model and the configuration layer that feeds it.

pub mod analysis;
pub mod cli;
pub mod config;
pub mod database;
pub mod diff;
pub mod discovery;
pub mod error;
pub mod execution;
pub mod export;
pub mod model;
pub mod stats;
pub mod validation;

pub use error::{Error, Result};

/// The version of this crate, reported by `dbctx --version`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
