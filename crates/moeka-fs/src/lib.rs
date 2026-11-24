//! Shared filesystem execution primitives. Callers schedule blocking operations;
//! this crate neither starts an executor nor owns application/UI state.

pub mod backend;
pub mod content;
pub mod local;
pub mod path;
mod peer;
pub mod protocol;
pub mod search;
mod search_snapshot;
pub mod service;
pub mod trash;
