use rust_stemmers::Stemmer;
use std::collections::HashMap;
use std::io;
use std::path::Path;
use std::process::Command;

use crate::constants::VIP_COMMANDS;
use crate::text::tokenize;

#[derive(Clone, Copy, PartialEq)]
pub enum Section {
    Name,
    Synopsis,
    Body,
}

struct SectionState {
    current: Section,
    name_lines_seen: u32,
}

impl SectionState {
    fn new() -> Self {
        SectionState {
            current: Section::Body,
            name_lines_seen: 0,
        }
    }

    fn advance(&mut self, line: &str) -> Section {
        let trimmed = line.trim();

        let is_header = trimmed.len() >= 2
            && trimmed
                .chars()
                .all(|c| c.is_uppercase() || c.is_whitespace() || c == '_' || c == '-')
            && trimmed.chars().any(|c| c.is_uppercase());

        if is_header {
            self.name_lines_seen = 0;
            self.current = if trimmed == "NAME" {
                Section::Name
            } else if trimmed.starts_with("SYNOPSIS") {
                Section::Synopsis
            } else {
                Section::Body
            };
            return self.current;
        }

        if self.current == Section::Name {
            if trimmed.is_empty() {
                return Section::Name;
            }
            self.name_lines_seen += 1;
            if self.name_lines_seen > 1 {
                return Section::Body;
            }
        }

        self.current
    }
}

pub struct DocFields {
    pub fname: String,
    pub cmd_name: String,
    pub name_desc_raw: String,
    pub name_desc_tf: HashMap<String, u32>,
    pub name_desc_len: u32,
    pub synopsis_tf: HashMap<String, u32>,
    pub synopsis_len: u32,
    pub body_tf: HashMap<String, u32>,
    pub body_len: u32,
}

/// Document-type score multiplier derived from the filename / section number.
pub fn doc_type_multiplier(fname: &str) -> f32 {
    // Skip index / heading files
    if fname.ends_with("const") || fname.ends_with("type") || fname.ends_with("head") {
        return 0.1;
    }

    let section_mult = match fname
        .rsplit('.')
        .next()
        .and_then(|s| s.chars().next())
        .and_then(|c| c.to_digit(10))
    {
        Some(1) => 4.0,           // User commands
        Some(8) => 2.5,           // Sysadmin commands
        Some(5) => 1.2,           // Config files
        Some(2) | Some(3) => 0.8, // Dev libs / syscalls
        Some(4) | Some(6) | Some(7) => 0.6,
        _ => 0.8,
    };

    let base = fname.split('.').next().unwrap_or("").to_lowercase();
    let vip_mult = if VIP_COMMANDS.contains(&base.as_str()) {
        5.0
    } else {
        1.0
    };

    section_mult * vip_mult
}

fn render_man_page(path: &Path) -> io::Result<String> {
    let output = Command::new("sh")
        .arg("-c")
        .arg(format!("man {} | col -b", path.to_string_lossy()))
        .output()?;

    if !output.status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("man command failed for {:?}", path),
        ));
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Parse the NAME line into (command names, description).
fn parse_name_line(line: &str) -> (Vec<String>, String) {
    let dash_pos = line.find(" - ").or_else(|| line.find(" \u{2013} "));

    if let Some(pos) = dash_pos {
        let names_part = &line[..pos];
        let desc_part = line[pos..]
            .trim_start_matches([' ', '-', '\u{2013}'])
            .trim();
        let names = names_part
            .split([',', ';'])
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        (names, desc_part.to_string())
    } else {
        let names = line
            .split([',', ';'])
            .map(|s| s.trim().to_lowercase())
            .filter(|s| !s.is_empty())
            .collect();
        (names, String::new())
    }
}

/// Parse a man-page at `path` into structured `DocFields`, or `None` if empty.
pub fn parse_doc(path: &Path, fname: &str, stemmer: &Stemmer) -> Option<DocFields> {
    let content = render_man_page(path).ok()?;

    let mut name_desc_tf: HashMap<String, u32> = HashMap::new();
    let mut synopsis_tf: HashMap<String, u32> = HashMap::new();
    let mut body_tf: HashMap<String, u32> = HashMap::new();
    let mut name_desc_len = 0u32;
    let mut synopsis_len = 0u32;
    let mut body_len = 0u32;

    let mut state = SectionState::new();
    let mut cmd_name = String::new();
    let mut name_desc_raw = String::new();
    let mut found_name_line = false;

    for line in content.lines() {
        let effective_section = state.advance(line);
        let trimmed = line.trim();

        // Capture the canonical NAME line (first non-empty line in NAME section)
        if effective_section == Section::Name
            && !trimmed.is_empty()
            && state.name_lines_seen == 1
            && !found_name_line
        {
            let (names, desc) = parse_name_line(trimmed);
            if let Some(first) = names.first() {
                cmd_name = stemmer.stem(first).into_owned();
            }
            name_desc_raw = desc.clone();
            let tokens = tokenize(&desc, stemmer);
            name_desc_len += tokens.len() as u32;
            for t in tokens {
                *name_desc_tf.entry(t).or_insert(0) += 1;
            }
            found_name_line = true;
            continue;
        }

        // All other lines go into their respective buckets
        let tokens = tokenize(line, stemmer);
        let count = tokens.len() as u32;
        match effective_section {
            Section::Synopsis => {
                synopsis_len += count;
                for t in tokens {
                    *synopsis_tf.entry(t).or_insert(0) += 1;
                }
            }
            // Name (after the first line) and Body both go into body
            Section::Name | Section::Body => {
                body_len += count;
                for t in tokens {
                    *body_tf.entry(t).or_insert(0) += 1;
                }
            }
        }
    }

    // Fall back to filename when no NAME section was found
    if cmd_name.is_empty() {
        let base = fname.split('.').next().unwrap_or("").to_lowercase();
        if base.len() > 1 {
            cmd_name = stemmer.stem(&base).into_owned();
        }
    }

    if name_desc_len + synopsis_len + body_len == 0 {
        return None;
    }

    Some(DocFields {
        fname: fname.to_string(),
        cmd_name,
        name_desc_raw,
        name_desc_tf,
        name_desc_len,
        synopsis_tf,
        synopsis_len,
        body_tf,
        body_len,
    })
}
