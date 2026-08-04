//! Generate accurate, deterministic database context that developers and AI
//! coding agents can trust.
//!
//! `SPEC.md` is the behavior contract for this crate. The public API is built
//! out over the phases described in `CLAUDE.md`; this crate currently carries
//! the repository foundation only.

/// The version of this crate, reported by `dbctx --version`.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
