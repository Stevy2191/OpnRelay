// SPDX-License-Identifier: AGPL-3.0-only
//! Shared domain model for OpnRelay: guides, steps, actions and annotations.
//!
//! This crate has no I/O. It is used by the server, the desktop app, the
//! export engine and (via JSON) the browser extension.

pub mod model;
pub mod title;
pub mod validate;

pub use model::*;
