use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse},
    routing::get,
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::process::Command;
use std::sync::Arc;
use tokio::net::TcpListener;

use man_search::constants::FINAL_INDEX_PATH;
use man_search::index::{load_index, MmapIndex};
use man_search::search::{search, SearchResult};

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

type AppState = Arc<MmapIndex>;

// Safely escape HTML to prevent rendering bugs
fn escape_html(c: char, buf: &mut String) {
    match c {
        '<' => buf.push_str("&lt;"),
        '>' => buf.push_str("&gt;"),
        '&' => buf.push_str("&amp;"),
        _ => buf.push(c),
    }
}

// Converts Unix backspace formatting into modern HTML tags
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

#[tokio::main]
async fn main() {
    println!("Loading memory-mapped index from {}...", FINAL_INDEX_PATH);

    let index = load_index(FINAL_INDEX_PATH).unwrap_or_else(|_| {
        eprintln!("Failed to load index. Run `cargo run --bin index` first.");
        std::process::exit(1);
    });

    let state = Arc::new(index);

    let app = Router::new()
        .route("/", get(serve_frontend))
        .route("/api/search", get(search_api))
        .route("/api/content", get(content_api))
        .with_state(state);

    let listener = TcpListener::bind("127.0.0.1:3000").await.unwrap();
    println!("Server running at http://127.0.0.1:3000");

    axum::serve(listener, app).await.unwrap();
}

async fn serve_frontend() -> Html<String> {
    let html = fs::read_to_string("index.html").unwrap_or_else(|_| {
        "<h1>Error: index.html not found in the project root!</h1>".to_string()
    });
    Html(html)
}

async fn search_api(
    State(index): State<AppState>,
    Query(params): Query<SearchQuery>,
) -> Json<Vec<SearchResult>> {
    let results = search(&params.q, &index);
    let top_results: Vec<SearchResult> = results.into_iter().take(15).collect();
    Json(top_results)
}

async fn content_api(Query(params): Query<ContentQuery>) -> impl IntoResponse {
    let base_name = params.fname.split('.').next().unwrap_or(&params.fname);

    // Force MANWIDTH to a wide standard (120 columns) to prevent text clipping
    // GROFF_NO_SGR=1 forces classic backspace formatting instead of ANSI colors
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!(
            "env MANWIDTH=120 GROFF_NO_SGR=1 PAGER=cat man {}",
            base_name
        ))
        .output();

    let text = match output {
        Ok(out) if out.status.success() => {
            let raw_text = String::from_utf8_lossy(&out.stdout);
            parse_man_formatting(&raw_text)
        }
        _ => format!("Could not load man page for '{}'", base_name),
    };

    Json(ContentResponse { text })
}
