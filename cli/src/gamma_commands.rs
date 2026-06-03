//! CLI subcommands for the Gamma API.
//!
//! Mirrors `polymarket-cli`'s `events / markets / tags / series / comments /
//! profiles / sports / search` tree, dropping subcommands whose endpoint does
//! not exist in the gamma openapi (no `teams`, no `markets list`,
//! no streaming) and adding platform-only ones (`curation events`,
//! `series summary`, `sport-types`, `public-info`, `agreements`).
//!
//! Wired into [`crate::commands::run`] via a single `Command::Gamma` arm so
//! the diff with shared CLI files stays minimal.

use anyhow::{Context, anyhow};
use chrono::DateTime;
use clap::{Args, Subcommand};
use predict_rs_clob_client::gamma::types::request::{
    CommentsByUserAddressRequest, GetSeriesRequest, ListCommentsRequest, ListCurationEventsRequest,
    ListEventCreatorsRequest, ListEventsRequest, ListSeriesRequest, ListTagsRequest,
    RelatedTagsRequest, SearchRequest,
};
use predict_rs_clob_client::gamma::types::response::{
    Agreement, Comment, CurationEvent, Event, EventCreator, Market, Profile, PublicInfo,
    PublicProfile, RelatedTag, Search, Series, SeriesSummary, SportType, Tag,
};
use predict_rs_clob_client::Client;
use tabled::Tabled;

use crate::output::{self, Format};

#[derive(Debug, Args)]
pub struct GammaArgs {
    #[command(subcommand)]
    pub command: GammaCmd,
}

#[derive(Debug, Subcommand)]
pub enum GammaCmd {
    /// `GET /health`.
    Health,
    /// `GET /public-info` — tenant brand + chain config.
    PublicInfo,
    /// `GET /agreements`.
    Agreements,
    /// `GET /config/sport-types` — sport-type catalog.
    SportTypes,
    /// `GET /public-search`.
    Search(SearchCmdArgs),
    /// Event subtree.
    Events(EventsArgs),
    /// Market subtree.
    Markets(MarketsArgs),
    /// Tag subtree.
    Tags(TagsArgs),
    /// Series subtree.
    Series(SeriesArgs),
    /// Comment subtree.
    Comments(CommentsArgs),
    /// Profile subtree.
    Profiles(ProfilesArgs),
    /// Curation subtree.
    Curation(CurationArgs),
}

// ─── events ─────────────────────────────────────────────────────────────────

#[derive(Debug, Args)]
pub struct EventsArgs {
    #[command(subcommand)]
    pub command: EventsCmd,
}

#[derive(Debug, Subcommand)]
pub enum EventsCmd {
    /// `GET /events`.
    List(ListEventsArgs),
    /// `GET /events/{id}` or `/events/slug/{slug}` (auto-detected from numeric vs slug).
    Get { id_or_slug: String },
    /// `GET /events/{id}/tags`.
    Tags { id: String },
    /// `GET /events/creators`.
    Creators(CreatorsArgs),
    /// `GET /events/creators/{id}`.
    Creator { id: String },
}

#[derive(Debug, Args)]
pub struct ListEventsArgs {
    #[arg(long)]
    pub limit: Option<i64>,
    #[arg(long)]
    pub offset: Option<i64>,
    /// Sort field: id, label, slug, created_at, updated_at.
    #[arg(long)]
    pub order: Option<String>,
    #[arg(long)]
    pub ascending: bool,
    #[arg(long)]
    pub slug: Option<String>,
    #[arg(long)]
    pub tag_id: Option<i64>,
    #[arg(long)]
    pub active: Option<bool>,
    #[arg(long)]
    pub closed: Option<bool>,
    #[arg(long)]
    pub archived: Option<bool>,
    #[arg(long)]
    pub featured: Option<bool>,
}

#[derive(Debug, Args)]
pub struct CreatorsArgs {
    #[arg(long)]
    pub limit: Option<i64>,
    #[arg(long)]
    pub offset: Option<i64>,
    #[arg(long)]
    pub order: Option<String>,
    #[arg(long)]
    pub ascending: bool,
    #[arg(long)]
    pub creator_name: Option<String>,
    #[arg(long)]
    pub creator_handle: Option<String>,
}

// ─── markets ────────────────────────────────────────────────────────────────

#[derive(Debug, Args)]
pub struct MarketsArgs {
    #[command(subcommand)]
    pub command: MarketsCmd,
}

#[derive(Debug, Subcommand)]
pub enum MarketsCmd {
    /// `GET /markets/{id}` or `/markets/slug/{slug}` (auto-detected).
    Get(GetMarketArgs),
    /// `GET /markets/{id}/tags`.
    Tags { id: String },
    /// `POST /markets/information` — bulk lookup by id / slug / clob token ids
    /// / condition ids. Pass the JSON body via `--body` or `--body-file`.
    Information(MarketsInformationArgs),
}

#[derive(Debug, Args)]
pub struct GetMarketArgs {
    pub id_or_slug: String,
    /// Embed the market's tags in the response.
    #[arg(long)]
    pub include_tag: bool,
}

#[derive(Debug, Args)]
pub struct MarketsInformationArgs {
    /// Raw JSON body. Mutually exclusive with `--body-file`.
    #[arg(long, conflicts_with = "body_file")]
    pub body: Option<String>,
    /// Path to a JSON file containing the body.
    #[arg(long)]
    pub body_file: Option<std::path::PathBuf>,
}

// ─── tags ───────────────────────────────────────────────────────────────────

#[derive(Debug, Args)]
pub struct TagsArgs {
    #[command(subcommand)]
    pub command: TagsCmd,
}

#[derive(Debug, Subcommand)]
pub enum TagsCmd {
    /// `GET /tags`.
    List(ListTagsArgs),
    /// `GET /tags/{id}` or `/tags/slug/{slug}` (auto-detected).
    Get { id_or_slug: String },
    /// `GET /tags/{id}/related-tags` (relationship rows) or
    /// `GET /tags/{id}/related-tags/tags` (full tag objects) when `--full` is set.
    Related(RelatedArgs),
}

#[derive(Debug, Args)]
pub struct ListTagsArgs {
    #[arg(long)]
    pub limit: Option<i64>,
    #[arg(long)]
    pub offset: Option<i64>,
    #[arg(long)]
    pub order: Option<String>,
    #[arg(long)]
    pub ascending: bool,
    #[arg(long)]
    pub is_carousel: Option<bool>,
}

#[derive(Debug, Args)]
pub struct RelatedArgs {
    pub id_or_slug: String,
    /// Return full Tag objects instead of relationship rows.
    #[arg(long)]
    pub full: bool,
    #[arg(long)]
    pub status: Option<String>,
    #[arg(long)]
    pub omit_empty: bool,
}

// ─── series ─────────────────────────────────────────────────────────────────

#[derive(Debug, Args)]
pub struct SeriesArgs {
    #[command(subcommand)]
    pub command: SeriesCmd,
}

#[derive(Debug, Subcommand)]
pub enum SeriesCmd {
    /// `GET /series`.
    List(ListSeriesArgs),
    /// `GET /series/{id}`.
    Get(GetSeriesArgs),
    /// `GET /series-summary/{id}` or `/series-summary/slug/{slug}`.
    Summary { id_or_slug: String },
    /// `GET /series/{id}/comments/count`.
    CommentsCount { id: String },
}

#[derive(Debug, Args)]
pub struct ListSeriesArgs {
    #[arg(long)]
    pub limit: Option<i64>,
    #[arg(long)]
    pub offset: Option<i64>,
    #[arg(long)]
    pub order: Option<String>,
    #[arg(long)]
    pub ascending: bool,
    #[arg(long)]
    pub recurrence: Option<String>,
    #[arg(long)]
    pub closed: Option<bool>,
    #[arg(long)]
    pub exclude_events: bool,
}

#[derive(Debug, Args)]
pub struct GetSeriesArgs {
    pub id: String,
    #[arg(long)]
    pub exclude_events: bool,
    #[arg(long)]
    pub include_chat: bool,
}

// ─── comments ───────────────────────────────────────────────────────────────

#[derive(Debug, Args)]
pub struct CommentsArgs {
    #[command(subcommand)]
    pub command: CommentsCmd,
}

#[derive(Debug, Subcommand)]
pub enum CommentsCmd {
    /// `GET /comments` — list with optional parent-entity filter.
    List(ListCommentsArgs),
    /// `GET /comments/{id}`.
    Get { id: String },
    /// `GET /comments/user_address/{user_address}`.
    ByUser(ByUserArgs),
}

#[derive(Debug, Args)]
pub struct ListCommentsArgs {
    /// `Event`, `Market`, or `Series` (case-sensitive on the server).
    #[arg(long)]
    pub parent_entity_type: Option<String>,
    #[arg(long)]
    pub parent_entity_id: Option<i64>,
    #[arg(long)]
    pub limit: Option<i64>,
    #[arg(long)]
    pub offset: Option<i64>,
    #[arg(long)]
    pub order: Option<String>,
    #[arg(long)]
    pub ascending: bool,
}

#[derive(Debug, Args)]
pub struct ByUserArgs {
    pub address: String,
    #[arg(long)]
    pub limit: Option<i64>,
    #[arg(long)]
    pub offset: Option<i64>,
    #[arg(long)]
    pub order: Option<String>,
    #[arg(long)]
    pub ascending: bool,
}

// ─── profiles ───────────────────────────────────────────────────────────────

#[derive(Debug, Args)]
pub struct ProfilesArgs {
    #[command(subcommand)]
    pub command: ProfilesCmd,
}

#[derive(Debug, Subcommand)]
pub enum ProfilesCmd {
    /// `GET /public-profile`.
    Public { address: String },
    /// `GET /profiles/user_address/{user_address}`.
    Get { address: String },
}

// ─── curation ───────────────────────────────────────────────────────────────

#[derive(Debug, Args)]
pub struct CurationArgs {
    #[command(subcommand)]
    pub command: CurationCmd,
}

#[derive(Debug, Subcommand)]
pub enum CurationCmd {
    /// `GET /curation/events`.
    Events {
        /// Bitmask filter: 1=normal, 2=highlight, 4=hero. `0` (default) returns all.
        #[arg(long)]
        featured_level: Option<i64>,
    },
}

// ─── search ─────────────────────────────────────────────────────────────────

#[derive(Debug, Args)]
pub struct SearchCmdArgs {
    pub query: String,
    #[arg(long)]
    pub limit_per_type: Option<i64>,
    #[arg(long)]
    pub page: Option<i64>,
    #[arg(long)]
    pub search_tags: Option<bool>,
    #[arg(long)]
    pub search_profiles: Option<bool>,
}

// ─── dispatch ───────────────────────────────────────────────────────────────

pub async fn run(client: Client, fmt: Format, args: GammaArgs) -> anyhow::Result<()> {
    let gamma = client.gamma().context("construct gamma sub-client")?;

    match args.command {
        GammaCmd::Health => {
            let v = gamma.health().await?;
            match fmt {
                Format::Json => output::print_json(&v)?,
                Format::Table => {
                    println!("service  : {}", v.service);
                    println!("status   : {}", v.status);
                    println!("timestamp: {}", v.timestamp);
                }
            }
        }
        GammaCmd::PublicInfo => {
            let v = gamma.public_info().await?;
            print_public_info(&v, fmt)?;
        }
        GammaCmd::Agreements => {
            let v = gamma.agreements().await?;
            print_agreements(&v, fmt)?;
        }
        GammaCmd::SportTypes => {
            let v = gamma.sport_types().await?;
            print_sport_types(&v.types, fmt)?;
        }
        GammaCmd::Search(a) => {
            let req = SearchRequest {
                q: a.query,
                limit_per_type: a.limit_per_type,
                page: a.page,
                search_tags: a.search_tags,
                search_profiles: a.search_profiles,
                ..Default::default()
            };
            let v = gamma.search(&req).await?;
            print_search(&v, fmt)?;
        }
        GammaCmd::Events(a) => run_events(&gamma, a, fmt).await?,
        GammaCmd::Markets(a) => run_markets(&gamma, a, fmt).await?,
        GammaCmd::Tags(a) => run_tags(&gamma, a, fmt).await?,
        GammaCmd::Series(a) => run_series(&gamma, a, fmt).await?,
        GammaCmd::Comments(a) => run_comments(&gamma, a, fmt).await?,
        GammaCmd::Profiles(a) => run_profiles(&gamma, a, fmt).await?,
        GammaCmd::Curation(a) => run_curation(&gamma, a, fmt).await?,
    }
    Ok(())
}

async fn run_events(
    gamma: &predict_rs_clob_client::gamma::GammaClient,
    args: EventsArgs,
    fmt: Format,
) -> anyhow::Result<()> {
    match args.command {
        EventsCmd::List(a) => {
            let req = ListEventsRequest {
                limit: a.limit,
                offset: a.offset,
                order: a.order,
                ascending: if a.ascending { Some(true) } else { None },
                slug: a.slug,
                tag_id: a.tag_id,
                active: a.active,
                closed: a.closed,
                archived: a.archived,
                featured: a.featured,
                ..Default::default()
            };
            let events = gamma.list_events(&req).await?;
            print_events(&events, fmt)?;
        }
        EventsCmd::Get { id_or_slug } => {
            let event = if is_numeric(&id_or_slug) {
                gamma.get_event(&id_or_slug).await?
            } else {
                gamma.get_event_by_slug(&id_or_slug).await?
            };
            print_event_detail(&event, fmt)?;
        }
        EventsCmd::Tags { id } => {
            let tags = gamma.event_tags(&id).await?;
            print_tags(&tags, fmt)?;
        }
        EventsCmd::Creators(a) => {
            let req = ListEventCreatorsRequest {
                limit: a.limit,
                offset: a.offset,
                order: a.order,
                ascending: if a.ascending { Some(true) } else { None },
                creator_name: a.creator_name,
                creator_handle: a.creator_handle,
            };
            let creators = gamma.list_event_creators(&req).await?;
            print_creators(&creators, fmt)?;
        }
        EventsCmd::Creator { id } => {
            let creator = gamma.get_event_creator(&id).await?;
            match fmt {
                Format::Json => output::print_json(&creator)?,
                Format::Table => print_creators(std::slice::from_ref(&creator), fmt)?,
            }
        }
    }
    Ok(())
}

async fn run_markets(
    gamma: &predict_rs_clob_client::gamma::GammaClient,
    args: MarketsArgs,
    fmt: Format,
) -> anyhow::Result<()> {
    match args.command {
        MarketsCmd::Get(a) => {
            let market = if is_numeric(&a.id_or_slug) {
                gamma.get_market(&a.id_or_slug, a.include_tag).await?
            } else {
                gamma.get_market_by_slug(&a.id_or_slug, a.include_tag).await?
            };
            print_market_detail(&market, fmt)?;
        }
        MarketsCmd::Tags { id } => {
            let tags = gamma.market_tags(&id).await?;
            print_tags(&tags, fmt)?;
        }
        MarketsCmd::Information(a) => {
            let body_text = match (a.body, a.body_file) {
                (Some(b), _) => b,
                (None, Some(p)) => std::fs::read_to_string(&p)
                    .with_context(|| format!("read --body-file {}", p.display()))?,
                (None, None) => return Err(anyhow!("pass --body or --body-file")),
            };
            let body: serde_json::Value =
                serde_json::from_str(&body_text).context("parse --body as JSON")?;
            let markets = gamma.markets_information(&body).await?;
            print_markets(&markets, fmt)?;
        }
    }
    Ok(())
}

async fn run_tags(
    gamma: &predict_rs_clob_client::gamma::GammaClient,
    args: TagsArgs,
    fmt: Format,
) -> anyhow::Result<()> {
    match args.command {
        TagsCmd::List(a) => {
            let req = ListTagsRequest {
                limit: a.limit,
                offset: a.offset,
                order: a.order,
                ascending: if a.ascending { Some(true) } else { None },
                is_carousel: a.is_carousel,
            };
            let tags = gamma.list_tags(&req).await?;
            print_tags(&tags, fmt)?;
        }
        TagsCmd::Get { id_or_slug } => {
            let tag = if is_numeric(&id_or_slug) {
                gamma.get_tag(&id_or_slug).await?
            } else {
                gamma.get_tag_by_slug(&id_or_slug).await?
            };
            match fmt {
                Format::Json => output::print_json(&tag)?,
                Format::Table => print_tags(std::slice::from_ref(&tag), fmt)?,
            }
        }
        TagsCmd::Related(a) => {
            let req = RelatedTagsRequest {
                status: a.status,
                omit_empty: if a.omit_empty { Some(true) } else { None },
            };
            let is_numeric = is_numeric(&a.id_or_slug);
            if a.full {
                let tags = if is_numeric {
                    gamma.tags_related_to_tag(&a.id_or_slug, &req).await?
                } else {
                    gamma
                        .tags_related_to_tag_by_slug(&a.id_or_slug, &req)
                        .await?
                };
                print_tags(&tags, fmt)?;
            } else {
                let rels = if is_numeric {
                    gamma.related_tags(&a.id_or_slug, &req).await?
                } else {
                    gamma.related_tags_by_slug(&a.id_or_slug, &req).await?
                };
                print_related_tags(&rels, fmt)?;
            }
        }
    }
    Ok(())
}

async fn run_series(
    gamma: &predict_rs_clob_client::gamma::GammaClient,
    args: SeriesArgs,
    fmt: Format,
) -> anyhow::Result<()> {
    match args.command {
        SeriesCmd::List(a) => {
            let req = ListSeriesRequest {
                limit: a.limit,
                offset: a.offset,
                order: a.order,
                ascending: if a.ascending { Some(true) } else { None },
                recurrence: a.recurrence,
                closed: a.closed,
                exclude_events: if a.exclude_events { Some(true) } else { None },
                ..Default::default()
            };
            let series = gamma.list_series(&req).await?;
            print_series_list(&series, fmt)?;
        }
        SeriesCmd::Get(a) => {
            let req = GetSeriesRequest {
                exclude_events: if a.exclude_events { Some(true) } else { None },
                include_chat: if a.include_chat { Some(true) } else { None },
            };
            let s = gamma.get_series(&a.id, &req).await?;
            print_series_detail(&s, fmt)?;
        }
        SeriesCmd::Summary { id_or_slug } => {
            let s = if is_numeric(&id_or_slug) {
                gamma.get_series_summary(&id_or_slug).await?
            } else {
                gamma.get_series_summary_by_slug(&id_or_slug).await?
            };
            print_series_summary(&s, fmt)?;
        }
        SeriesCmd::CommentsCount { id } => {
            let c = gamma.series_comment_count(&id).await?;
            output::print_scalar("count", c.count, fmt)?;
        }
    }
    Ok(())
}

async fn run_comments(
    gamma: &predict_rs_clob_client::gamma::GammaClient,
    args: CommentsArgs,
    fmt: Format,
) -> anyhow::Result<()> {
    match args.command {
        CommentsCmd::List(a) => {
            let req = ListCommentsRequest {
                limit: a.limit,
                offset: a.offset,
                order: a.order,
                ascending: if a.ascending { Some(true) } else { None },
                parent_entity_type: a.parent_entity_type,
                parent_entity_id: a.parent_entity_id,
            };
            let comments = gamma.list_comments(&req).await?;
            print_comments(&comments, fmt)?;
        }
        CommentsCmd::Get { id } => {
            let comments = gamma.get_comment(&id).await?;
            print_comments(&comments, fmt)?;
        }
        CommentsCmd::ByUser(a) => {
            let req = CommentsByUserAddressRequest {
                limit: a.limit,
                offset: a.offset,
                order: a.order,
                ascending: if a.ascending { Some(true) } else { None },
            };
            let comments = gamma.comments_by_user(&a.address, &req).await?;
            print_comments(&comments, fmt)?;
        }
    }
    Ok(())
}

async fn run_profiles(
    gamma: &predict_rs_clob_client::gamma::GammaClient,
    args: ProfilesArgs,
    fmt: Format,
) -> anyhow::Result<()> {
    match args.command {
        ProfilesCmd::Public { address } => {
            let p = gamma.get_public_profile(&address).await?;
            print_public_profile(&p, fmt)?;
        }
        ProfilesCmd::Get { address } => {
            let p = gamma.get_profile_by_address(&address).await?;
            print_profile(&p, fmt)?;
        }
    }
    Ok(())
}

async fn run_curation(
    gamma: &predict_rs_clob_client::gamma::GammaClient,
    args: CurationArgs,
    fmt: Format,
) -> anyhow::Result<()> {
    match args.command {
        CurationCmd::Events { featured_level } => {
            let req = ListCurationEventsRequest { featured_level };
            let events = gamma.list_curation_events(&req).await?;
            print_curation_events(&events, fmt)?;
        }
    }
    Ok(())
}

// ─── output helpers ─────────────────────────────────────────────────────────

fn is_numeric(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

fn opt<T: ToString>(v: &Option<T>) -> String {
    v.as_ref().map(ToString::to_string).unwrap_or_default()
}

fn iso(v: &Option<DateTime<chrono::Utc>>) -> String {
    v.as_ref().map(|d| d.to_rfc3339()).unwrap_or_default()
}

#[derive(Tabled)]
struct EventRow {
    id: String,
    slug: String,
    title: String,
    active: String,
    closed: String,
    end_date: String,
    markets: usize,
}

fn print_events(events: &[Event], fmt: Format) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(&events)?,
        Format::Table => {
            let rows: Vec<EventRow> = events
                .iter()
                .map(|e| EventRow {
                    id: e.id.clone(),
                    slug: e.slug.clone().unwrap_or_default(),
                    title: e.title.clone().unwrap_or_default(),
                    active: opt(&e.active),
                    closed: opt(&e.closed),
                    end_date: iso(&e.end_date),
                    markets: e.markets.len(),
                })
                .collect();
            if rows.is_empty() {
                println!("(no events)");
            } else {
                output::print_table(rows);
            }
        }
    }
    Ok(())
}

fn print_event_detail(e: &Event, fmt: Format) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(e)?,
        Format::Table => {
            println!("id          : {}", e.id);
            println!("slug        : {}", opt(&e.slug));
            println!("title       : {}", opt(&e.title));
            println!("description : {}", opt(&e.description));
            println!("category    : {}", opt(&e.category));
            println!("active      : {}", opt(&e.active));
            println!("closed      : {}", opt(&e.closed));
            println!("archived    : {}", opt(&e.archived));
            println!("featured    : {}", opt(&e.featured));
            println!("liquidity   : {}", opt(&e.liquidity));
            println!("volume      : {}", opt(&e.volume));
            println!("volume24h   : {}", opt(&e.volume24hr));
            println!("start_date  : {}", iso(&e.start_date));
            println!("end_date    : {}", iso(&e.end_date));
            println!("num_markets : {}", opt(&e.num_markets));
            println!("markets     :");
            for m in &e.markets {
                println!(
                    "  - id={} slug={} q='{}' last={} bid={} ask={}",
                    m.id,
                    opt(&m.slug),
                    opt(&m.question),
                    opt(&m.last_trade_price),
                    opt(&m.best_bid),
                    opt(&m.best_ask),
                );
            }
            if !e.tags.is_empty() {
                let labels: Vec<String> =
                    e.tags.iter().filter_map(|t| t.label.clone()).collect();
                println!("tags        : {}", labels.join(", "));
            }
        }
    }
    Ok(())
}

#[derive(Tabled)]
struct MarketRow {
    id: String,
    question: String,
    last: String,
    bid: String,
    ask: String,
    volume24h: String,
    liquidity: String,
    status: &'static str,
}

fn market_status(m: &Market) -> &'static str {
    if m.closed.unwrap_or(false) {
        "closed"
    } else if m.archived.unwrap_or(false) {
        "archived"
    } else if m.accepting_orders.unwrap_or(false) {
        "open"
    } else if m.active.unwrap_or(false) {
        "active"
    } else {
        "—"
    }
}

fn print_markets(markets: &[Market], fmt: Format) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(&markets)?,
        Format::Table => {
            let rows: Vec<MarketRow> = markets
                .iter()
                .map(|m| MarketRow {
                    id: m.id.clone(),
                    question: m.question.clone().unwrap_or_default(),
                    last: opt(&m.last_trade_price),
                    bid: opt(&m.best_bid),
                    ask: opt(&m.best_ask),
                    volume24h: opt(&m.volume24hr),
                    liquidity: opt(&m.liquidity),
                    status: market_status(m),
                })
                .collect();
            if rows.is_empty() {
                println!("(no markets)");
            } else {
                output::print_table(rows);
            }
        }
    }
    Ok(())
}

fn print_market_detail(m: &Market, fmt: Format) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(m)?,
        Format::Table => {
            println!("id            : {}", m.id);
            println!("question      : {}", opt(&m.question));
            println!("slug          : {}", opt(&m.slug));
            println!("condition_id  : {}", m.condition_id);
            println!("status        : {}", market_status(m));
            println!("last_trade    : {}", opt(&m.last_trade_price));
            println!("best_bid      : {}", opt(&m.best_bid));
            println!("best_ask      : {}", opt(&m.best_ask));
            println!("volume        : {}", opt(&m.volume));
            println!("volume24h     : {}", opt(&m.volume24hr));
            println!("liquidity     : {}", opt(&m.liquidity));
            println!("tick_size     : {}", opt(&m.order_price_min_tick_size));
            println!("min_size      : {}", opt(&m.order_min_size));
            println!("max_size      : {}", opt(&m.order_max_size));
            println!("end_date      : {}", iso(&m.end_date));
            let ids = m.parsed_clob_token_ids();
            if !ids.is_empty() {
                println!("token_ids     : {}", ids.join(", "));
            }
            if let Some(adj) = m.adjudication.as_ref() {
                println!("adjudication  : {} ({})", adj.status, adj.current_phase);
            }
        }
    }
    Ok(())
}

#[derive(Tabled)]
struct TagRow {
    id: String,
    label: String,
    slug: String,
    tag_type: String,
}

fn print_tags(tags: &[Tag], fmt: Format) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(tags)?,
        Format::Table => {
            let rows: Vec<TagRow> = tags
                .iter()
                .map(|t| TagRow {
                    id: t.id.clone(),
                    label: t.label.clone().unwrap_or_default(),
                    slug: t.slug.clone().unwrap_or_default(),
                    tag_type: t.tag_type.clone().unwrap_or_default(),
                })
                .collect();
            if rows.is_empty() {
                println!("(no tags)");
            } else {
                output::print_table(rows);
            }
        }
    }
    Ok(())
}

#[derive(Tabled)]
struct RelatedRow {
    id: String,
    tag_id: String,
    related_tag_id: String,
    rank: String,
}

fn print_related_tags(rels: &[RelatedTag], fmt: Format) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(rels)?,
        Format::Table => {
            let rows: Vec<RelatedRow> = rels
                .iter()
                .map(|r| RelatedRow {
                    id: r.id.clone(),
                    tag_id: opt(&r.tag_id),
                    related_tag_id: opt(&r.related_tag_id),
                    rank: opt(&r.rank),
                })
                .collect();
            if rows.is_empty() {
                println!("(no related tags)");
            } else {
                output::print_table(rows);
            }
        }
    }
    Ok(())
}

#[derive(Tabled)]
struct CreatorRow {
    id: String,
    handle: String,
    name: String,
}

fn print_creators(creators: &[EventCreator], fmt: Format) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(creators)?,
        Format::Table => {
            let rows: Vec<CreatorRow> = creators
                .iter()
                .map(|c| CreatorRow {
                    id: c.id.clone(),
                    handle: c.creator_handle.clone().unwrap_or_default(),
                    name: c.creator_name.clone().unwrap_or_default(),
                })
                .collect();
            if rows.is_empty() {
                println!("(no creators)");
            } else {
                output::print_table(rows);
            }
        }
    }
    Ok(())
}

#[derive(Tabled)]
struct SeriesRow {
    id: String,
    slug: String,
    title: String,
    recurrence: String,
    closed: String,
    events: usize,
}

fn print_series_list(series: &[Series], fmt: Format) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(series)?,
        Format::Table => {
            let rows: Vec<SeriesRow> = series
                .iter()
                .map(|s| SeriesRow {
                    id: s.id.clone(),
                    slug: s.slug.clone().unwrap_or_default(),
                    title: s.title.clone().unwrap_or_default(),
                    recurrence: s.recurrence.clone().unwrap_or_default(),
                    closed: opt(&s.closed),
                    events: s.events.len(),
                })
                .collect();
            if rows.is_empty() {
                println!("(no series)");
            } else {
                output::print_table(rows);
            }
        }
    }
    Ok(())
}

fn print_series_detail(s: &Series, fmt: Format) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(s)?,
        Format::Table => {
            println!("id          : {}", s.id);
            println!("slug        : {}", opt(&s.slug));
            println!("title       : {}", opt(&s.title));
            println!("recurrence  : {}", opt(&s.recurrence));
            println!("active      : {}", opt(&s.active));
            println!("closed      : {}", opt(&s.closed));
            println!("featured    : {}", opt(&s.featured));
            println!("volume      : {}", opt(&s.volume));
            println!("volume24h   : {}", opt(&s.volume24hr));
            println!("liquidity   : {}", opt(&s.liquidity));
            println!("start_date  : {}", iso(&s.start_date));
            println!("events      : {} attached", s.events.len());
        }
    }
    Ok(())
}

fn print_series_summary(s: &SeriesSummary, fmt: Format) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(s)?,
        Format::Table => {
            println!("id                  : {}", s.id);
            println!("title               : {}", opt(&s.title));
            println!("slug                : {}", opt(&s.slug));
            println!("event_dates         : {}", s.event_dates.join(", "));
            println!(
                "event_weeks         : {}",
                s.event_weeks
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            println!("earliest_open_week  : {}", opt(&s.earliest_open_week));
            println!("earliest_open_date  : {}", opt(&s.earliest_open_date));
        }
    }
    Ok(())
}

#[derive(Tabled)]
struct CommentRow {
    id: String,
    parent: String,
    user: String,
    body: String,
    reactions: String,
    created: String,
}

fn print_comments(comments: &[Comment], fmt: Format) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(comments)?,
        Format::Table => {
            let rows: Vec<CommentRow> = comments
                .iter()
                .map(|c| CommentRow {
                    id: c.id.clone(),
                    parent: format!(
                        "{}#{}",
                        c.parent_entity_type.clone().unwrap_or_default(),
                        opt(&c.parent_entity_id),
                    ),
                    user: c.user_address.clone().unwrap_or_default(),
                    body: c.body.clone().unwrap_or_default(),
                    reactions: opt(&c.reaction_count),
                    created: iso(&c.created_at),
                })
                .collect();
            if rows.is_empty() {
                println!("(no comments)");
            } else {
                output::print_table(rows);
            }
        }
    }
    Ok(())
}

fn print_public_profile(p: &PublicProfile, fmt: Format) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(p)?,
        Format::Table => {
            println!("name             : {}", opt(&p.name));
            println!("pseudonym        : {}", opt(&p.pseudonym));
            println!("eoa_address      : {}", opt(&p.eoa_address));
            println!("proxy_wallet     : {}", opt(&p.proxy_wallet));
            println!("bio              : {}", opt(&p.bio));
            println!("verified_badge   : {}", opt(&p.verified_badge));
            println!("x_username       : {}", opt(&p.x_username));
            println!("created_at       : {}", iso(&p.created_at));
        }
    }
    Ok(())
}

fn print_profile(p: &Profile, fmt: Format) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(p)?,
        Format::Table => {
            println!("id              : {}", p.id);
            println!("name            : {}", opt(&p.name));
            println!("pseudonym       : {}", opt(&p.pseudonym));
            println!("eoa_address     : {}", opt(&p.eoa_address));
            println!("proxy_wallet    : {}", opt(&p.proxy_wallet));
            println!("bio             : {}", opt(&p.bio));
            println!("created_at      : {}", iso(&p.created_at));
            println!("updated_at      : {}", iso(&p.updated_at));
        }
    }
    Ok(())
}

#[derive(Tabled)]
struct CurationRow {
    id: String,
    slug: String,
    title: String,
    featured_level: String,
    hero: String,
    highlight: String,
    normal: String,
}

fn print_curation_events(events: &[CurationEvent], fmt: Format) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(events)?,
        Format::Table => {
            let rows: Vec<CurationRow> = events
                .iter()
                .map(|c| CurationRow {
                    id: c.event.id.clone(),
                    slug: c.event.slug.clone().unwrap_or_default(),
                    title: c.event.title.clone().unwrap_or_default(),
                    featured_level: c.featured_level.to_string(),
                    hero: opt(&c.featured_order_hero),
                    highlight: opt(&c.featured_order_highlight),
                    normal: opt(&c.featured_order_normal),
                })
                .collect();
            if rows.is_empty() {
                println!("(no curated events)");
            } else {
                output::print_table(rows);
            }
        }
    }
    Ok(())
}

fn print_public_info(p: &PublicInfo, fmt: Format) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(p)?,
        Format::Table => {
            println!("brand.title           : {}", p.brand.title);
            println!("brand.logo            : {}", p.brand.logo);
            println!("contracts.exchange    : {}", p.contracts.exchange_address);
            println!(
                "contracts.neg_risk    : {}",
                p.contracts.neg_risk_exchange_address
            );
            println!("contracts.ctf         : {}", p.contracts.ctf_address);
            println!("contracts.collateral  : {}", p.contracts.collateral_token);
            println!(
                "wallet_connect_project: {}",
                p.wallet_connect_project_id
            );
            println!("login_statement       : {}", p.login_statement);
            println!("chain                 : {}", p.chain);
            if let Some(app) = &p.app {
                println!("app.terms_url         : {}", app.terms_url);
            }
            println!("agreements            : {} entries", p.agreements.len());
        }
    }
    Ok(())
}

#[derive(Tabled)]
struct AgreementRow {
    r#type: String,
    version: String,
    title: String,
    required: String,
    sort: String,
}

fn print_agreements(agreements: &[Agreement], fmt: Format) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(agreements)?,
        Format::Table => {
            let rows: Vec<AgreementRow> = agreements
                .iter()
                .map(|a| AgreementRow {
                    r#type: a.r#type.clone(),
                    version: a.version.clone(),
                    title: a.title_translation.clone(),
                    required: a.required.to_string(),
                    sort: a.sort_order.to_string(),
                })
                .collect();
            if rows.is_empty() {
                println!("(no agreements)");
            } else {
                output::print_table(rows);
            }
        }
    }
    Ok(())
}

#[derive(Tabled)]
struct SportTypeRow {
    value: String,
    labels: String,
    stages: String,
}

fn print_sport_types(types: &[SportType], fmt: Format) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(types)?,
        Format::Table => {
            let rows: Vec<SportTypeRow> = types
                .iter()
                .map(|t| {
                    let labels: Vec<String> = t
                        .label
                        .iter()
                        .map(|(k, v)| format!("{k}={v}"))
                        .collect();
                    let stages: Vec<String> =
                        t.stages.iter().map(|s| s.value.clone()).collect();
                    SportTypeRow {
                        value: t.value.clone(),
                        labels: labels.join(", "),
                        stages: stages.join(", "),
                    }
                })
                .collect();
            if rows.is_empty() {
                println!("(no sport types)");
            } else {
                output::print_table(rows);
            }
        }
    }
    Ok(())
}

fn print_search(s: &Search, fmt: Format) -> anyhow::Result<()> {
    match fmt {
        Format::Json => output::print_json(s)?,
        Format::Table => {
            println!("events ({}):", s.events.len());
            print_events(&s.events, Format::Table)?;
            println!("\ntags ({}):", s.tags.len());
            for t in &s.tags {
                println!("  {} | {} | {} | events={}", t.id, t.label, t.slug, t.event_count);
            }
            println!("\nprofiles ({}):", s.profiles.len());
            for p in &s.profiles {
                println!(
                    "  id={} name={} pseudonym={}",
                    p.id,
                    p.name.clone().unwrap_or_default(),
                    p.pseudonym.clone().unwrap_or_default()
                );
            }
            println!(
                "\npagination: hasMore={} total={}",
                s.pagination.has_more, s.pagination.total_results
            );
        }
    }
    Ok(())
}
