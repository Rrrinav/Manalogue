use rust_stemmers::{Algorithm, Stemmer};

pub fn make_stemmer() -> Stemmer {
    Stemmer::create(Algorithm::English)
}

pub fn is_stop_word(w: &str) -> bool {
    matches!(
        w,
        "a" | "an" | "the" | "and" | "or" | "but" | "if" | "then" | "else"
        | "when" | "while" | "where" | "why" | "how" | "of" | "to" | "in"
        | "on" | "at" | "by" | "for" | "with" | "about" | "from" | "into"
        | "over" | "after" | "before" | "does" | "between" | "through"
        | "during" | "without" | "within" | "is" | "are" | "was" | "were"
        | "be" | "been" | "being" | "do" | "will" | "did" | "doing" | "have"
        | "has" | "had" | "having" | "can" | "could" | "should" | "would"
        | "may" | "might" | "must" | "such" | "shall" | "as" | "it" | "its"
        | "it's" | "this" | "that" | "these" | "those" | "he" | "she" | "they"
        | "them" | "yes" | "their" | "there" | "here" | "we" | "you" | "your"
        | "i" | "me" | "my" | "our" | "us" | "not" | "no" | "use" | "than"
        | "too" | "very" | "also" | "just" | "only" | "even" | "more" | "most"
        | "some" | "any" | "each" | "other" | "used" | "call" | "called"
        | "return" | "returns" | "value" | "set" | "get" | "new" | "see"
    )
}

/// Split `text` into stemmed tokens, removing stop-words and very short words.
pub fn tokenize(text: &str, stemmer: &Stemmer) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric() && c != '-' && c != '_')
        .map(|w| w.to_lowercase())
        .filter(|w| {
            !is_stop_word(w)
                && if w.starts_with('-') {
                    w.len() >= 2
                } else {
                    w.len() > 2
                }
        })
        .map(|w| stemmer.stem(&w).into_owned())
        .collect()
}

/// Classic Levenshtein distance, bailing out early when `max_dist` is exceeded.
pub fn edit_distance(a: &str, b: &str, max_dist: usize) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (m, n) = (a.len(), b.len());

    if m.abs_diff(n) > max_dist {
        return max_dist + 1;
    }

    let mut prev: Vec<usize> = (0..=n).collect();
    let mut curr = vec![0usize; n + 1];

    for i in 1..=m {
        curr[0] = i;
        let mut row_min = i;
        for j in 1..=n {
            curr[j] = if a[i - 1] == b[j - 1] {
                prev[j - 1]
            } else {
                1 + prev[j - 1].min(prev[j]).min(curr[j - 1])
            };
            row_min = row_min.min(curr[j]);
        }
        if row_min > max_dist {
            return max_dist + 1;
        }
        std::mem::swap(&mut prev, &mut curr);
    }
    prev[n]
}

