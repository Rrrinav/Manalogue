use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::process::Command;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;

use man_search::constants::FINAL_INDEX_PATH;
use man_search::index::{load_index, MmapIndex};
use man_search::search::{search, SearchResult};

// Simple token-bucket per IP: max 30 requests per 10 seconds.
const RATE_LIMIT_WINDOW_SECS: u64 = 10;
const RATE_LIMIT_MAX_REQUESTS: u32 = 30;

struct RateLimiter {
    buckets: Mutex<HashMap<String, (u32, Instant)>>,
}

impl RateLimiter {
    fn new() -> Self {
        RateLimiter {
            buckets: Mutex::new(HashMap::new()),
        }
    }

    /// Returns true if the request should be allowed.
    fn check(&self, ip: &str) -> bool {
        let mut buckets = self.buckets.lock().unwrap();
        let window = Duration::from_secs(RATE_LIMIT_WINDOW_SECS);
        let now = Instant::now();

        let entry = buckets.entry(ip.to_string()).or_insert((0, now));
        if now.duration_since(entry.1) >= window {
            *entry = (1, now);
            return true;
        }
        if entry.0 < RATE_LIMIT_MAX_REQUESTS {
            entry.0 += 1;
            return true;
        }
        false
    }
}

struct AppState {
    index: MmapIndex,
    rate_limiter: RateLimiter,
}

type SharedState = Arc<AppState>;

#[derive(Deserialize)]
struct SearchQuery {
    q: String,
}

#[derive(Deserialize)]
struct ContentQuery {
    fname: String,
}

#[derive(Serialize)]
struct ContentResponse {
    text: String,
}

/// Accepts only alphanumeric characters, hyphens, underscores, and dots.
/// Returns None if the input is empty or contains anything suspicious.
fn sanitize_fname(fname: &str) -> Option<&str> {
    if fname.is_empty() || fname.len() > 64 {
        return None;
    }
    if fname
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        Some(fname)
    } else {
        None
    }
}

/// Strips everything after the first dot and validates the result.
/// "ls.1" -> Some("ls"), "foo;rm -rf" -> None
fn sanitize_cmd(fname: &str) -> Option<String> {
    let base = fname.split('.').next().unwrap_or(fname);
    if base.is_empty() || base.len() > 64 {
        return None;
    }
    if base
        .chars()
        .all(|c| c.is_alphanumeric() || c == '-' || c == '_')
    {
        Some(base.to_string())
    } else {
        None
    }
}

/// Clamp and sanitize search queries.
fn sanitize_query(q: &str) -> Option<String> {
    let trimmed = q.trim();
    if trimmed.is_empty() || trimmed.len() > 256 {
        return None;
    }
    Some(trimmed.to_string())
}

fn escape_html(c: char, buf: &mut String) {
    match c {
        '<' => buf.push_str("&lt;"),
        '>' => buf.push_str("&gt;"),
        '&' => buf.push_str("&amp;"),
        _ => buf.push(c),
    }
}

/// Converts Unix backspace formatting (`X\x08X` = bold, `_\x08X` = underline)
/// into `<b>` / `<u>` HTML tags.
fn parse_man_formatting(raw: &str) -> String {
    let mut result = String::with_capacity(raw.len() * 2);
    let chars: Vec<char> = raw.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if i + 2 < chars.len() && chars[i + 1] == '\x08' {
            let first = chars[i];
            let second = chars[i + 2];
            if first == '_' {
                result.push_str("<u>");
                escape_html(second, &mut result);
                result.push_str("</u>");
            } else {
                result.push_str("<b>");
                escape_html(second, &mut result);
                result.push_str("</b>");
            }
            i += 3;
        } else {
            escape_html(chars[i], &mut result);
            i += 1;
        }
    }
    result
}

async fn serve_frontend() -> Html<String> {
    let html = fs::read_to_string("index.html").unwrap_or_else(|_| {
        "<h1>Error: index.html not found in the project root!</h1>".to_string()
    });
    Html(html)
}

async fn search_api(State(state): State<SharedState>, Query(params): Query<SearchQuery>,
) -> impl IntoResponse {
    // Rate limit by a fixed key (extend to real IP if behind a proxy)
    if !state.rate_limiter.check("global") {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(Vec::<SearchResult>::new()),
        )
            .into_response();
    }

    let q = match sanitize_query(&params.q) {
        Some(q) => q,
        None => return (StatusCode::BAD_REQUEST, Json(Vec::<SearchResult>::new())).into_response(),
    };

    let results: Vec<SearchResult> = search(&q, &state.index).into_iter().take(15).collect();

    Json(results).into_response()
}

async fn content_api(
    State(state): State<SharedState>,
    Query(params): Query<ContentQuery>,
) -> impl IntoResponse {
    if !state.rate_limiter.check("global") {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            Json(ContentResponse {
                text: "Rate limit exceeded.".into(),
            }),
        )
            .into_response();
    }

    // Validate fname first, then extract the command name
    let fname = match sanitize_fname(&params.fname) {
        Some(f) => f,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ContentResponse {
                    text: "Invalid filename.".into(),
                }),
            )
                .into_response();
        }
    };

    let cmd = match sanitize_cmd(fname) {
        Some(c) => c,
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(ContentResponse {
                    text: "Invalid command name.".into(),
                }),
            )
                .into_response();
        }
    };

    // Use Command::new instead of sh -c to avoid shell injection entirely.
    // Pass env vars and the command name as discrete arguments — no shell interpolation.
    let output = Command::new("man")
        .env("MANWIDTH", "120")
        .env("GROFF_NO_SGR", "1")
        .env("PAGER", "cat")
        .arg(&cmd)
        .output();

    let text = match output {
        Ok(out) if out.status.success() => {
            let raw = String::from_utf8_lossy(&out.stdout);
            parse_man_formatting(&raw)
        }
        _ => format!("Could not load man page for '{cmd}'"),
    };

    Json(ContentResponse { text }).into_response()
}

#[tokio::main]
async fn main() {
    println!("Loading memory-mapped index from {}...", FINAL_INDEX_PATH);
    let index = load_index(FINAL_INDEX_PATH).unwrap_or_else(|_| {
        eprintln!("Failed to load index. Run `cargo run --bin index` first.");
        std::process::exit(1);
    });

    let state = Arc::new(AppState {
        index,
        rate_limiter: RateLimiter::new(),
    });

    let app = Router::new()
        .route("/", get(serve_frontend))
        .route("/api/search", get(search_api))
        .route("/api/content", get(content_api))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:3000").await.unwrap();
    println!("Server running at http://127.0.0.1:3000");
    axum::serve(listener, app).await.unwrap();
}
