//! CLOB WebSocket client — market channel (`/ws/market`, public) and user
//! channel (`/ws/user`, auth-required).
//!
//! Construct via [`crate::Client::clob_ws`]; the returned
//! [`ClobWebSocketClient`] is a sub-client analogous to
//! [`crate::gamma::GammaClient`].
//!
//! See `pm-rs/docs/ws.md` for the wire-format overview, and the AsyncAPI
//! specs at `pm-cup2026/services/clob-service/docs/asyncapi-{market,user}.json`
//! for the authoritative payload definitions.

pub mod client;
pub mod subscription;
pub mod types;

pub use client::{ClobWebSocketClient, MarketSubscribeOpts, MidpointUpdate};
pub use subscription::{MarketStream, UserStream};
pub use types::request::{MarketLevel, MarketSubscribeRequest, UserSubscribeRequest};
pub use types::response::{
    BestBidAskEvent, BookEvent, LastTradePriceEvent, MakerOrderFill, MarketEvent,
    MarketResolvedEvent, NewMarketEvent, OrderEvent, OrderLevel, OrderSide, OrderStatus,
    OrderSubType, PriceChangeEntry, PriceChangeEvent, TickSizeChangeEvent, TradeEvent, TradeStatus,
    TraderSide, UserEvent,
};
