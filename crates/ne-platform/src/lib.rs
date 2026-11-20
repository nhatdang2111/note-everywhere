//! OS integration: secure credential storage, idle and sleep signals, app directories.
//!
//! Small helpers with no `unsafe` of their own that compile on macOS and
//! Windows. Platform specifics live in the crates this one wraps.
//!
//! * [`activity`]: [`activity::ActivityMonitor`] polls idle time and detects
//!   system sleep, emitting the events the vault's auto-lock consumes. Screen
//!   lock detection is not implemented (it needs unsafe platform calls); the
//!   module documents the gap.
//! * [`dirs`]: where config, data, cache and models go on each OS.
//! * [`secure_store`]: session token and account root key in the macOS login
//!   keychain / Windows Credential Manager. Never the vault key.

#![deny(missing_docs)]

pub mod activity;
pub mod dirs;
pub mod secure_store;
