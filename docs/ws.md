# WebSocket subscriptions

`predict-rs-clob-client` ships a typed client for the `clob-service` WebSocket
service. The server lives on port `:8082` of the same process that serves the
REST API on `:8080`, but at distinct paths:

| Path | Auth | Subscribes by |
|------|------|---------------|
| `/ws/market` | none | `asset_ids` (token IDs) |
| `/ws/user`   | L2 (API key + passphrase, in the first WS frame) | `markets` (condition IDs) |

The authoritative wire spec is the platform repo's `services/clob-service/docs/asyncapi-{market,user}.json`.
Go-side counterpart: `pm-sdk-go/pkg/ws/`.

## Quick start (SDK)

```rust,no_run
use futures::StreamExt as _;
use predict_rs_clob_client::{Client, MarketSubscribeOpts};

# async fn run() -> predict_rs_clob_client::Result<()> {
let client = Client::builder().tenant("hermestrade.xyz")?.build()?;
let ws = client.clob_ws()?;

let mut stream = ws
    .subscribe_market(
        vec!["1234".into(), "5678".into()],
        MarketSubscribeOpts::default().with_initial_dump(true),
    )
    .await?;

while let Some(event) = stream.next().await {
    println!("{:?}", event?);
}
# Ok(())
# }
```

## Wire format

### Outbound

| Channel | First frame | Fields |
|---------|-------------|--------|
| `/ws/market` | `{"assets_ids": [...], "type": "market", "initial_dump"?: bool, "level"?: 1\|2\|3, "custom_feature_enabled"?: bool}` | `assets_ids` required, all others optional. |
| `/ws/user`   | `{"auth": {"apiKey": "...", "passphrase": "...", "secret"?: "..."}, "type": "user", "markets": [...]}` | `auth.apiKey` + `auth.passphrase` required. Empty `markets` = subscribe to all markets owned by the API key. |

Runtime subscribe / unsubscribe:

```json
{"operation": "subscribe", "assets_ids": ["..."]}         // market channel
{"operation": "unsubscribe", "markets": ["0xcid"]}        // user channel
```

### Heartbeat

The client sends the **text** frame `"PING"` every 10 s (the SDK default; see
[`WsConfig::ping_interval`]). The server replies with the text frame `"PONG"`,
which the connection task swallows internally — application streams never see
keep-alives.

This is the server convention; protocol-level WebSocket Ping/Pong frames are
*also* honored, but the server doesn't use them.

### Inbound events

| `event_type` | Variant | Channel | Notes |
|--------------|---------|---------|-------|
| `book` | `MarketEvent::Book(BookEvent)` | `/ws/market` | Initial dump + on-demand snapshots. |
| `price_change` | `MarketEvent::PriceChange(PriceChangeEvent)` | `/ws/market` | Deltas; `size = "0"` means level removed. |
| `last_trade_price` | `MarketEvent::LastTradePrice(LastTradePriceEvent)` | `/ws/market` | `transaction_hash` empty for synthetic trades. |
| `tick_size_change` | `MarketEvent::TickSizeChange(...)` | `/ws/market` | |
| `best_bid_ask` | `MarketEvent::BestBidAsk(...)` | `/ws/market` | Requires `custom_feature_enabled = true`. |
| `new_market` | `MarketEvent::NewMarket(...)` | `/ws/market` | Requires `custom_feature_enabled = true`. |
| `market_resolved` | `MarketEvent::MarketResolved(...)` | `/ws/market` | Requires `custom_feature_enabled = true`. |
| `order` | `UserEvent::Order(OrderEvent)` | `/ws/user`   | Owner-scoped order lifecycle. |
| `trade` | `UserEvent::Trade(TradeEvent)` | `/ws/user`   | Owner-scoped trade execution + settlement. |

Timestamps deserialize from both JSON numbers and quoted strings; the SDK uses
the `Timestamp` newtype to bridge both forms.

## Reconnect behaviour

[`WsConnection`] reconnects automatically with exponential backoff (1 s → 2 s
→ … → 30 s cap). On every successful re-connection it re-sends the active
subscribe envelope so the server's view of the subscription matches the
client's local state.

The reconnect loop **never** swallows auth failures:

- An HTTP error on the upgrade (`401`/`403`) surfaces as
  [`WsError::Auth`] and terminates the stream.
- A user-channel `{"error":"authentication failed"}` envelope (the
  server's response to a bad apiKey + passphrase) surfaces as
  [`WsError::UserAuthRejected`] and terminates the stream.

## CLI

```bash
# Health check — connect, PING, expect PONG, disconnect.
predict-cli ws ping --tenant hermestrade.xyz

# Subscribe to one or more asset ids, print N events, then exit.
predict-cli ws book --tenant hermestrade.xyz 1234 5678 --level 2 --count 5

# Live ticker — prints best-of-book updates per frame; Ctrl-C to exit.
predict-cli ws book-watch --tenant hermestrade.xyz 1234

# JSON-per-line mode for piping to jq.
predict-cli ws book-watch --tenant hermestrade.xyz 1234 --print-as-json

# User channel — auto-derives credentials via /auth/derive-api-key.
predict-cli ws user \
  --tenant hermestrade.xyz \
  --chain-id 11155420 \
  --private-key 0x<key> \
  --market 0xcondition_a \
  --market 0xcondition_b
```

## Runtime subscribe / unsubscribe

Both streams expose `subscribe(...)` / `unsubscribe(...)` async methods that
emit the `subscriptionRequestUpdate` envelope on the live socket. The SDK
also tracks the *current* subscription locally so that any reconnect restores
the full set — there is no need to re-subscribe by hand after a transient
network blip.

[`WsConnection`]: ../clob-client/src/ws/connection.rs
[`WsError::Auth`]: ../clob-client/src/ws/error.rs
[`WsError::UserAuthRejected`]: ../clob-client/src/ws/error.rs
[`WsConfig::ping_interval`]: ../clob-client/src/ws/config.rs
