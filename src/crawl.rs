use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::PathBuf;

use crate::doc::parse_doc;
use crate::io_util::{write_str, write_tf_map, write_u32};
use crate::text::make_stemmer;

pub struct CrawlStats {
    pub total_docs: u32,
    pub global_df: HashMap<String, u32>,
    pub avg_desc_len: f32,
    pub avg_synopsis_len: f32,
    pub avg_body_len: f32,
}

/// Walk `source_dirs`, parse every man-page found, and stream raw
/// per-document data to `out_path`.  Returns aggregate statistics
/// needed for BM25 normalisation in Pass 2.
pub fn crawl(source_dirs: &[&str], out_path: &str) -> io::Result<CrawlStats> {
    let stemmer = make_stemmer();
    let file = File::create(out_path)?;
    let mut writer = BufWriter::new(file);

    let mut global_df: HashMap<String, u32> = HashMap::new();
    let mut total_docs: u32 = 0;
    let mut sum_desc = 0u64;
    let mut sum_synopsis = 0u64;
    let mut sum_body = 0u64;

    // Iterative DFS over all source directories
    let mut dirs: Vec<PathBuf> = source_dirs.iter().map(PathBuf::from).collect();

    while let Some(dir) = dirs.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
                continue;
            }
            if !path.is_file() {
                continue;
            }

            let fname = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();

            let Some(doc) = parse_doc(&path, &fname, &stemmer) else {
                continue;
            };

            // Update global document-frequency counts
            let mut seen: HashSet<&String> = HashSet::new();
            for w in doc
                .name_desc_tf
                .keys()
                .chain(doc.synopsis_tf.keys())
                .chain(doc.body_tf.keys())
            {
                if seen.insert(w) {
                    *global_df.entry(w.clone()).or_insert(0) += 1;
                }
            }
            if !doc.cmd_name.is_empty() && seen.insert(&doc.cmd_name) {
                *global_df.entry(doc.cmd_name.clone()).or_insert(0) += 1;
            }

            sum_desc += doc.name_desc_len as u64;
            sum_synopsis += doc.synopsis_len as u64;
            sum_body += doc.body_len as u64;

            // Serialise document to temp file
            write_str(&mut writer, &doc.fname)?;
            write_str(&mut writer, &doc.cmd_name)?;
            write_u32(&mut writer, doc.name_desc_len)?;
            write_u32(&mut writer, doc.synopsis_len)?;
            write_u32(&mut writer, doc.body_len)?;
            write_tf_map(&mut writer, &doc.name_desc_tf)?;
            write_tf_map(&mut writer, &doc.synopsis_tf)?;
            write_tf_map(&mut writer, &doc.body_tf)?;
            write_str(&mut writer, &doc.name_desc_raw)?;

            total_docs += 1;
            print!("\rIndexed: {total_docs}");
            io::stdout().flush().unwrap();
        }
    }

    println!();
    writer.flush()?;

    let n = total_docs.max(1) as f64;
    Ok(CrawlStats {
        total_docs,
        global_df,
        avg_desc_len: (sum_desc as f64 / n) as f32,
        avg_synopsis_len: (sum_synopsis as f64 / n) as f32,
        avg_body_len: (sum_body as f64 / n) as f32,
    })
}
