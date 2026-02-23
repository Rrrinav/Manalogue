use std::fs;
use std::io;

use man_search::constants::{FINAL_INDEX_PATH, SOURCE_DIRS, TEMP_INDEX_PATH};
use man_search::crawl::crawl;
use man_search::index::{build_index, save_index};

fn main() -> io::Result<()> {
    println!("[1/3] Crawling {} source directories…", SOURCE_DIRS.len());
    let stats = crawl(&SOURCE_DIRS, TEMP_INDEX_PATH)?;
    println!(
        "      {} docs  |  avg desc={:.1}  synopsis={:.1}  body={:.1}",
        stats.total_docs, stats.avg_desc_len, stats.avg_synopsis_len, stats.avg_body_len
    );

    println!("[2/3] Building BM25 + semantic index…");
    let index = build_index(TEMP_INDEX_PATH, &stats)?;
    println!(
        "      {} index terms  |  {} cmd names  |  {} desc terms",
        index.inverted.len(),
        index.cmd_name_index.len(),
        index.desc_index.len()
    );

    println!("[3/3] Saving index to '{FINAL_INDEX_PATH}'…");
    save_index(FINAL_INDEX_PATH, &index)?;

    let _ = fs::remove_file(TEMP_INDEX_PATH);
    println!("Done.  Run `cargo run --bin search -- <query>` to search.");
    Ok(())
}
