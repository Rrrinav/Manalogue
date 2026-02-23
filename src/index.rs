use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Write};

use crate::constants::*;
use crate::crawl::CrawlStats;
use crate::doc::doc_type_multiplier;
use crate::io_util::*;

pub struct Index {
    /// Filename of each document (indexed by doc_id).
    pub doc_map: Vec<String>,
    /// Stemmed command name for each document.
    pub cmd_names: Vec<String>,
    /// Raw NAME-section description for each document (used for semantic re-ranking).
    pub name_descs: Vec<String>,
    /// Main BM25 inverted index: term → sorted (doc_id, score) postings.
    pub inverted: HashMap<String, Vec<(u32, f32)>>,
    /// cmd_name → [doc_id] (exact command-name lookup).
    pub cmd_name_index: HashMap<String, Vec<u32>>,
    /// term → [doc_id] for terms that appear in the NAME description.
    pub desc_index: HashMap<String, Vec<u32>>,
}

#[inline]
fn bm25_term(tf: f32, dl: f32, avgdl: f32, n: f32, df: f32) -> f32 {
    if tf == 0.0 || df == 0.0 || n == 0.0 {
        return 0.0;
    }
    let idf = ((n - df + 0.5) / (df + 0.5) + 1.0).ln().max(0.0);
    let ntf = (tf * (BM25_K1 + 1.0)) / (tf + BM25_K1 * (1.0 - BM25_B + BM25_B * dl / avgdl));
    idf * ntf
}

pub fn build_index(temp_path: &str, stats: &CrawlStats) -> io::Result<Index> {
    let CrawlStats {
        total_docs,
        global_df,
        avg_desc_len,
        avg_synopsis_len,
        avg_body_len,
    } = stats;

    let n = *total_docs as f32;
    let file = File::open(temp_path)?;
    let mut reader = BufReader::new(file);

    let mut doc_map = Vec::with_capacity(*total_docs as usize);
    let mut cmd_names = Vec::with_capacity(*total_docs as usize);
    let mut name_descs = Vec::with_capacity(*total_docs as usize);
    let mut inverted: HashMap<String, Vec<(u32, f32)>> = HashMap::new();
    let mut cmd_name_index: HashMap<String, Vec<u32>> = HashMap::new();
    let mut desc_index: HashMap<String, Vec<u32>> = HashMap::new();

    for doc_id in 0..*total_docs {
        let fname = read_str(&mut reader)?;
        let cmd_name = read_str(&mut reader)?;
        let desc_len = read_u32(&mut reader)? as f32;
        let synopsis_len = read_u32(&mut reader)? as f32;
        let body_len = read_u32(&mut reader)? as f32;
        let desc_tf = read_tf_map(&mut reader)?;
        let synopsis_tf = read_tf_map(&mut reader)?;
        let body_tf = read_tf_map(&mut reader)?;
        let name_desc_raw = read_str(&mut reader)?;

        let type_mult = doc_type_multiplier(&fname);

        doc_map.push(fname);
        cmd_names.push(cmd_name.clone());
        name_descs.push(name_desc_raw);

        if !cmd_name.is_empty() {
            cmd_name_index.entry(cmd_name.clone()).or_default().push(doc_id);
        }

        // Build desc_index (term → doc_id)
        for term in desc_tf.keys() {
            desc_index.entry(term.clone()).or_default().push(doc_id);
        }

        // Collect all unique terms across all fields
        let all_terms: HashSet<String> = desc_tf
            .keys()
            .chain(synopsis_tf.keys())
            .chain(body_tf.keys())
            .chain(std::iter::once(&cmd_name))
            .cloned()
            .collect();

        for term in &all_terms {
            let df = *global_df.get(term).unwrap_or(&1) as f32;

            let cmd_score = if term == &cmd_name && !cmd_name.is_empty() {
                bm25_term(1.0, 1.0, 1.0, n, df) * WEIGHT_CMD_NAME
            } else {
                0.0
            };

            let desc_score = if desc_len > 0.0 {
                bm25_term(
                    *desc_tf.get(term).unwrap_or(&0) as f32,
                    desc_len,
                    avg_desc_len.max(1.0),
                    n,
                    df,
                ) * WEIGHT_NAME_DESC
            } else {
                0.0
            };

            let syn_score = if synopsis_len > 0.0 {
                bm25_term(
                    *synopsis_tf.get(term).unwrap_or(&0) as f32,
                    synopsis_len,
                    avg_synopsis_len.max(1.0),
                    n,
                    df,
                ) * WEIGHT_SYNOPSIS
            } else {
                0.0
            };

            let body_score = if body_len > 0.0 {
                bm25_term(
                    *body_tf.get(term).unwrap_or(&0) as f32,
                    body_len,
                    avg_body_len.max(1.0),
                    n,
                    df,
                ) * WEIGHT_BODY
            } else {
                0.0
            };

            let score = (cmd_score + desc_score + syn_score + body_score) * type_mult;
            if score > 0.0 {
                inverted.entry(term.clone()).or_default().push((doc_id, score));
            }
        }
    }

    // Sort each posting list highest-score-first
    for postings in inverted.values_mut() {
        postings.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    }

    Ok(Index {
        doc_map,
        cmd_names,
        name_descs,
        inverted,
        cmd_name_index,
        desc_index,
    })
}

pub fn save_index(path: &str, index: &Index) -> io::Result<()> {
    let mut w = BufWriter::new(File::create(path)?);

    write_u32(&mut w, index.doc_map.len() as u32)?;
    for i in 0..index.doc_map.len() {
        write_str(&mut w, &index.doc_map[i])?;
        write_str(&mut w, &index.cmd_names[i])?;
        write_str(&mut w, &index.name_descs[i])?;
    }

    write_u32(&mut w, index.inverted.len() as u32)?;
    for (word, postings) in &index.inverted {
        write_str(&mut w, word)?;
        write_u32(&mut w, postings.len() as u32)?;
        for &(doc_id, score) in postings {
            write_u32(&mut w, doc_id)?;
            write_f32(&mut w, score)?;
        }
    }

    w.flush()
}

/// Load a previously saved index from disk.
pub fn load_index(path: &str) -> io::Result<Index> {
    let mut r = BufReader::new(File::open(path)?);

    let doc_count = read_u32(&mut r)? as usize;
    let mut doc_map = Vec::with_capacity(doc_count);
    let mut cmd_names = Vec::with_capacity(doc_count);
    let mut name_descs = Vec::with_capacity(doc_count);
    let mut cmd_name_index: HashMap<String, Vec<u32>> = HashMap::new();
    let mut desc_index: HashMap<String, Vec<u32>> = HashMap::new();

    for doc_id in 0..doc_count {
        let fname = read_str(&mut r)?;
        let cmd_name = read_str(&mut r)?;
        let name_desc = read_str(&mut r)?;

        if !cmd_name.is_empty() {
            cmd_name_index
                .entry(cmd_name.clone())
                .or_default()
                .push(doc_id as u32);
        }

        doc_map.push(fname);
        cmd_names.push(cmd_name);
        name_descs.push(name_desc);
    }

    let term_count = read_u32(&mut r)? as usize;
    let mut inverted: HashMap<String, Vec<(u32, f32)>> = HashMap::with_capacity(term_count);

    for _ in 0..term_count {
        let word = read_str(&mut r)?;
        let n = read_u32(&mut r)? as usize;
        let mut postings = Vec::with_capacity(n);
        for _ in 0..n {
            let doc_id = read_u32(&mut r)?;
            let score = read_f32(&mut r)?;
            postings.push((doc_id, score));
        }
        inverted.insert(word, postings);
    }

    // Re-build desc_index from the loaded inverted index + name_descs
    // (we re-derive it rather than storing it to keep the file format simpler)
    use crate::text::make_stemmer;
    use crate::text::tokenize;
    let stemmer = make_stemmer();
    for (doc_id, desc) in name_descs.iter().enumerate() {
        for token in tokenize(desc, &stemmer) {
            desc_index
                .entry(token)
                .or_default()
                .push(doc_id as u32);
        }
    }

    Ok(Index {
        doc_map,
        cmd_names,
        name_descs,
        inverted,
        cmd_name_index,
        desc_index,
    })
}

