//! Wire types for the chainup CLOB WebSocket channels.
//!
//! Authoritative schemas live in
//! `pm-cup2026/services/clob-service/docs/asyncapi-{market,user}.json`. The
//! `request` module covers subscribe / unsubscribe envelopes; `response`
//! covers every event the server may push.

pub mod request;
pub mod response;
