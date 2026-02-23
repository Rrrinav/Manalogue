//! `man_search search`
//!
//! Loads the pre-built index once, then answers queries instantly.
//!
//! Usage (single query):
//!   cargo run --bin search -- "copy file"
//!
//! Usage (interactive REPL):
//!   cargo run --bin search
//!
//! Use a custom index:
//!   cargo run --bin search -- --index custom.idx "copy file"

use std::io::{self, BufRead, Write};

use man_search::constants::FINAL_INDEX_PATH;
use man_search::index::load_index;
use man_search::search::search_and_print;

const DEFAULT_TOP_K: usize = 10;

fn main() -> io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();

    // Parse optional --index <path> flag
    let (index_path, query_args): (&str, Vec<&str>) = {
        let mut idx = FINAL_INDEX_PATH;
        let mut rest: Vec<&str> = Vec::new();
        let mut skip = false;
        for (i, arg) in args.iter().enumerate() {
            if skip {
                skip = false;
                continue;
            }
            if arg == "--index" {
                idx = args.get(i + 1).map(|s| s.as_str()).unwrap_or(idx);
                skip = true;
            } else {
                rest.push(arg.as_str());
            }
        }
        (idx, rest)
    };

    // ── Load index ──────────────────────────────────────────────────────────
    eprint!("Loading index '{index_path}'… ");
    let index = load_index(index_path).map_err(|e| {
        eprintln!("\nFailed to load index: {e}");
        eprintln!("Have you run `cargo run --bin index` first?");
        e
    })?;
    eprintln!("OK ({} docs)", index.doc_map.len());

    // ── Single query from CLI args ──────────────────────────────────────────
    if !query_args.is_empty() {
        let query = query_args.join(" ");
        search_and_print(&query, &index, DEFAULT_TOP_K);
        return Ok(());
    }

    // ── Interactive REPL ────────────────────────────────────────────────────
    println!("Type a query and press Enter. Ctrl-D / empty line to exit.");
    let stdin = io::stdin();
    loop {
        print!("> ");
        io::stdout().flush()?;

        let mut line = String::new();
        if stdin.lock().read_line(&mut line)? == 0 || line.trim().is_empty() {
            break;
        }
        search_and_print(line.trim(), &index, DEFAULT_TOP_K);
    }

    Ok(())
}
