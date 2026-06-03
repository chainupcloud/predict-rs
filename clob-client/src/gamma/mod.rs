//! Gamma REST API client (events, markets, tags, series, comments,
//! profiles, search, curation, sports config).
//!
//! Gamma is a separate REST service from CLOB; it lives at
//! `gamma-api.<tenant>` (e.g. `https://gamma-api.hermestrade.xyz`). Construct
//! a [`GammaClient`] from a parent [`crate::Client`] via [`crate::Client::gamma`].
//!
//! The full endpoint table and upstream-comparison live in
//! [`predict-rs/docs/gamma.md`](../../docs/gamma.md).

pub mod client;
pub mod types;

pub use client::GammaClient;
