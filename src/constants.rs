// BM25 tuning
pub const BM25_K1: f32 = 1.5;
pub const BM25_B: f32 = 0.75;

// Field weights
pub const WEIGHT_CMD_NAME: f32 = 30.0;
pub const WEIGHT_NAME_DESC: f32 = 12.0;
pub const WEIGHT_SYNOPSIS: f32 = 2.5;
pub const WEIGHT_BODY: f32 = 1.0;

// Search behaviour
pub const SEMANTIC_RERANK_N: usize = 50;
pub const SEMANTIC_WEIGHT: f32 = 15.0;

// Fuzzy / prefix search
/// Minimum token length before prefix expansion is attempted.
pub const PREFIX_MIN_LEN: usize = 4;
/// Minimum IDF before prefix expansion is attempted.
pub const PREFIX_MIN_IDF: f32 = 1.0;
/// Minimum token length before fuzzy (edit-distance) matching is attempted.
pub const FUZZY_MIN_LEN: usize = 4;

// Index file paths
pub const TEMP_INDEX_PATH: &str = "temp_index.bin";
pub const FINAL_INDEX_PATH: &str = "man.idx";

// Source directories
pub const SOURCE_DIRS: [&str; 2] = ["man-pages-6.9.1/man", "pure_coreutils_man/"];

// VIP commands (boosted in ranking)
pub const VIP_COMMANDS: &[&str] = &[
    "ls", "cp", "mv", "rm", "mkdir", "rmdir", "cd", "pwd", "cat", "echo",
    "chmod", "chown", "tar", "grep", "find", "awk", "sed", "kill", "ps",
    "top", "df", "du", "mount", "umount", "ip", "ping", "ssh", "bash", "sh",
    "sudo", "su", "apt", "pacman", "systemctl", "journalctl", "man", "info",
    "less", "more", "nano", "vim", "git", "curl", "wget", "rsync", "ln",
    "stat", "touch", "tail", "head", "sort", "uniq", "wc", "read", "gzip",
    "bzip2", "unzip", "zip", "chgrp", "date", "cal", "whoami",
];
