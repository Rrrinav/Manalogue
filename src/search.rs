use std::collections::{HashMap, HashSet};

use rust_stemmers::Stemmer;

use crate::constants::*;
use crate::index::MmapIndex;
use crate::text::{edit_distance, make_stemmer, tokenize};

fn query_idf(token: &str, index: &MmapIndex, n: f32) -> f32 {
    let df = index
        .inverted_dict
        .get(token)
        .map(|&(_, len)| len)
        .unwrap_or(1) as f32;
    ((n - df + 0.5) / (df + 0.5) + 1.0).ln().max(0.01)
}

fn semantic_desc_score(
    query_tokens: &HashSet<String>,
    token_idfs: &HashMap<String, f32>,
    name_desc: &str,
    stemmer: &Stemmer,
) -> f32 {
    if name_desc.is_empty() || query_tokens.is_empty() {
        return 0.0;
    }

    let desc_tokens: HashSet<String> = tokenize(name_desc, stemmer).into_iter().collect();
    if desc_tokens.is_empty() {
        return 0.0;
    }

    let mut idfs: Vec<f32> = token_idfs.values().copied().collect();
    idfs.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median_idf = idfs.get(idfs.len() / 2).copied().unwrap_or(0.0);

    if idfs.len() > 1 {
        for (tok, &idf) in token_idfs.iter() {
            if idf >= median_idf && !desc_tokens.contains(tok) {
                return 0.0;
            }
        }
    }

    let mut idf_overlap = 0.0f32;
    let mut total_query_idf = 0.0f32;
    for qt in query_tokens {
        let idf = token_idfs.get(qt).copied().unwrap_or(0.01);
        total_query_idf += idf;
        if desc_tokens.contains(qt) {
            idf_overlap += idf;
        }
    }
    if total_query_idf == 0.0 {
        return 0.0;
    }

    let coverage = idf_overlap / total_query_idf;
    let matched = query_tokens.intersection(&desc_tokens).count() as f32;
    let precision = matched / desc_tokens.len() as f32;

    if coverage + precision == 0.0 {
        return 0.0;
    }
    let f1 = 2.0 * coverage * precision / (coverage + precision);
    f1 * f1
}

pub struct SearchResult {
    pub doc_id: u32,
    pub fname: String,
    pub name_desc: String,
    pub score: f32,
}

pub fn search(query: &str, index: &MmapIndex) -> Vec<SearchResult> {
    let stemmer = make_stemmer();
    let query_tokens_vec = tokenize(query, &stemmer);
    if query_tokens_vec.is_empty() {
        return Vec::new();
    }

    let query_token_set: HashSet<String> = query_tokens_vec.iter().cloned().collect();
    let n = index.doc_map.len() as f32;

    let token_idfs: HashMap<String, f32> = query_tokens_vec
        .iter()
        .map(|t| (t.clone(), query_idf(t, index, n)))
        .collect();
    let total_idf: f32 = token_idfs.values().sum();

    let mut doc_score: HashMap<u32, f32> = HashMap::new();
    let mut doc_matched_idf: HashMap<u32, f32> = HashMap::new();

    for (token, &tok_idf) in query_tokens_vec.iter().zip(token_idfs.values()) {
        let mut token_posts: HashMap<u32, f32> = HashMap::new();

        // Exact match via mmap
        if let Some(postings) = index.get_postings(token) {
            for (doc_id, score) in postings {
                *token_posts.entry(doc_id).or_insert(0.0) += score;
            }
        }

        // Prefix expansion
        if token.len() >= PREFIX_MIN_LEN && tok_idf > PREFIX_MIN_IDF {
            for (key, _) in &index.inverted_dict {
                if key != token && key.starts_with(token.as_str()) {
                    let penalty = (0.6f32).powf((key.len() - token.len()) as f32 + 1.0);
                    if let Some(postings) = index.get_postings(key) {
                        for (doc_id, score) in postings {
                            *token_posts.entry(doc_id).or_insert(0.0) += score * penalty;
                        }
                    }
                }
            }
        }

        // Fuzzy fallback (edit-distance <= 1)
        if token_posts.is_empty() && token.len() >= FUZZY_MIN_LEN {
            for key in index.inverted_dict.keys() {
                if key.len().abs_diff(token.len()) <= 1 && edit_distance(key, token, 1) <= 1 {
                    if let Some(postings) = index.get_postings(key) {
                        for (doc_id, score) in postings {
                            *token_posts.entry(doc_id).or_insert(0.0) += score * 0.5;
                        }
                    }
                }
            }
        }

        if let Some(desc_docs) = index.desc_index.get(token) {
            for &doc_id in desc_docs {
                token_posts.entry(doc_id).or_insert(0.0);
            }
        }

        let matched = !token_posts.is_empty();
        for (doc_id, score) in token_posts {
            *doc_score.entry(doc_id).or_insert(0.0) += score;
            if matched {
                *doc_matched_idf.entry(doc_id).or_insert(0.0) += tok_idf;
            }
        }
    }

    let and_exp = (query_tokens_vec.len() as f32 - 1.0).max(2.0);
    let mut candidates: Vec<(u32, f32)> = doc_score
        .into_iter()
        .filter_map(|(doc_id, score)| {
            let midf = *doc_matched_idf.get(&doc_id).unwrap_or(&0.0);
            if midf == 0.0 {
                return None;
            }
            let coverage = (midf / total_idf).min(1.0);
            Some((doc_id, score * coverage.powf(and_exp)))
        })
        .collect();

    candidates.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut reranked: Vec<(u32, f32)> = candidates
        .into_iter()
        .take(SEMANTIC_RERANK_N)
        .map(|(doc_id, bm25_score)| {
            let sem = semantic_desc_score(
                &query_token_set,
                &token_idfs,
                &index.name_descs[doc_id as usize],
                &stemmer,
            );
            (doc_id, bm25_score * (1.0 + SEMANTIC_WEIGHT * sem))
        })
        .collect();

    reranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let mut best_for_base: HashMap<String, (u32, f32)> = HashMap::new();
    for &(doc_id, score) in &reranked {
        let base = index.doc_map[doc_id as usize]
            .split('.')
            .next()
            .unwrap_or("")
            .to_lowercase();
        let entry = best_for_base
            .entry(base)
            .or_insert((doc_id, f32::NEG_INFINITY));
        if score > entry.1 {
            *entry = (doc_id, score);
        }
    }

    let mut deduped: Vec<(u32, f32)> = best_for_base.into_values().collect();
    deduped.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    deduped
        .into_iter()
        .map(|(doc_id, score)| SearchResult {
            doc_id,
            fname: index.doc_map[doc_id as usize].clone(),
            name_desc: index.name_descs[doc_id as usize].clone(),
            score,
        })
        .collect()
}

pub fn search_and_print(query: &str, index: &MmapIndex, top_k: usize) {
    let stemmer = make_stemmer();
    let tokens = tokenize(query, &stemmer);

    println!("\nQuery: '{query}'");
    println!("  Tokens: {tokens:?}");

    if tokens.is_empty() {
        println!("  No searchable terms.");
        return;
    }

    let results = search(query, index);

    if results.is_empty() {
        println!("  No results found.");
        return;
    }

    for r in results.iter().take(top_k) {
        let preview = if r.name_desc.is_empty() {
            String::new()
        } else {
            format!(" -> {}", r.name_desc)
        };
        println!("  [{:.3}] {}{}", r.score, r.fname, preview);
    }
}
