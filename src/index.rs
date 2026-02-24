use memmap2::MmapOptions;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{self, BufReader, BufWriter, Cursor, Read, Seek, Write};

use crate::constants::*;
use crate::crawl::CrawlStats;
use crate::doc::doc_type_multiplier;
use crate::io_util::*;

// Used during Pass 2 to build the index in RAM
pub struct Index {
    pub doc_map: Vec<String>,
    pub cmd_names: Vec<String>,
    pub name_descs: Vec<String>,
    pub inverted: HashMap<String, Vec<(u32, f32)>>,
    pub cmd_name_index: HashMap<String, Vec<u32>>,
    pub desc_index: HashMap<String, Vec<u32>>,
}

// Used during Querying to read from disk instantly
pub struct MmapIndex {
    pub doc_map: Vec<String>,
    pub cmd_names: Vec<String>,
    pub name_descs: Vec<String>,
    pub inverted_dict: HashMap<String, (u64, u32)>, // word -> (byte_offset, num_postings)
    pub cmd_name_index: HashMap<String, Vec<u32>>,
    pub desc_index: HashMap<String, Vec<u32>>,
    mmap: memmap2::Mmap,
}

impl MmapIndex {
    /// Reads a posting list directly from the memory-mapped file
    pub fn get_postings(&self, word: &str) -> Option<Vec<(u32, f32)>> {
        let &(offset, len) = self.inverted_dict.get(word)?;
        let mut postings = Vec::with_capacity(len as usize);
        let mut pos = offset as usize;

        for _ in 0..len {
            let mut doc_bytes = [0u8; 4];
            doc_bytes.copy_from_slice(&self.mmap[pos..pos + 4]);
            let doc_id = u32::from_le_bytes(doc_bytes);
            pos += 4;

            let mut score_bytes = [0u8; 4];
            score_bytes.copy_from_slice(&self.mmap[pos..pos + 4]);
            let score = f32::from_le_bytes(score_bytes);
            pos += 4;

            postings.push((doc_id, score));
        }
        Some(postings)
    }
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
            cmd_name_index
                .entry(cmd_name.clone())
                .or_default()
                .push(doc_id);
        }

        for term in desc_tf.keys() {
            desc_index.entry(term.clone()).or_default().push(doc_id);
        }

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
                inverted
                    .entry(term.clone())
                    .or_default()
                    .push((doc_id, score));
            }
        }
    }

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

    // 1. Write docs metadata
    write_u32(&mut w, index.doc_map.len() as u32)?;
    for i in 0..index.doc_map.len() {
        write_str(&mut w, &index.doc_map[i])?;
        write_str(&mut w, &index.cmd_names[i])?;
        write_str(&mut w, &index.name_descs[i])?;
    }

    // 2. Write Postings dynamically and track offsets
    let mut dict = Vec::with_capacity(index.inverted.len());
    for (word, postings) in &index.inverted {
        let offset = w.stream_position()?;
        for &(doc_id, score) in postings {
            write_u32(&mut w, doc_id)?;
            write_f32(&mut w, score)?;
        }
        dict.push((word.clone(), offset, postings.len() as u32));
    }

    // 3. Write Dictionary
    let dict_offset = w.stream_position()?;
    write_u32(&mut w, dict.len() as u32)?;
    for (word, offset, len) in dict {
        write_str(&mut w, &word)?;
        w.write_all(&offset.to_le_bytes())?;
        write_u32(&mut w, len)?;
    }

    // 4. Write Footer (8 bytes pointing to the dictionary)
    w.write_all(&dict_offset.to_le_bytes())?;

    w.flush()
}

pub fn load_index(path: &str) -> io::Result<MmapIndex> {
    let file = File::open(path)?;
    let mmap = unsafe { MmapOptions::new().map(&file)? };

    let len = mmap.len();
    if len < 8 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "File too small"));
    }

    // Read the footer to find the dictionary
    let mut footer = [0u8; 8];
    footer.copy_from_slice(&mmap[len - 8..]);
    let dict_offset = u64::from_le_bytes(footer) as usize;

    // 1. Read metadata from the start
    let mut r = Cursor::new(&mmap[..dict_offset]);
    let doc_count = read_u32(&mut r)? as usize;

    let mut doc_map = Vec::with_capacity(doc_count);
    let mut cmd_names = Vec::with_capacity(doc_count);
    let mut name_descs = Vec::with_capacity(doc_count);
    let mut cmd_name_index: HashMap<String, Vec<u32>> = HashMap::new();

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

    // 2. Read the dictionary into memory
    let mut r_dict = Cursor::new(&mmap[dict_offset..len - 8]);
    let dict_len = read_u32(&mut r_dict)?;
    let mut inverted_dict = HashMap::with_capacity(dict_len as usize);

    for _ in 0..dict_len {
        let word = read_str(&mut r_dict)?;
        let mut off_buf = [0u8; 8];
        r_dict.read_exact(&mut off_buf)?;
        let offset = u64::from_le_bytes(off_buf);
        let num_postings = read_u32(&mut r_dict)?;

        inverted_dict.insert(word, (offset, num_postings));
    }

    // 3. Rebuild desc_index
    use crate::text::make_stemmer;
    use crate::text::tokenize;
    let stemmer = make_stemmer();
    let mut desc_index: HashMap<String, Vec<u32>> = HashMap::new();
    for (doc_id, desc) in name_descs.iter().enumerate() {
        for token in tokenize(desc, &stemmer) {
            desc_index.entry(token).or_default().push(doc_id as u32);
        }
    }

    Ok(MmapIndex {
        doc_map,
        cmd_names,
        name_descs,
        inverted_dict,
        cmd_name_index,
        desc_index,
        mmap,
    })
}
