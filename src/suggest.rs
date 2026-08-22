//! The suggestion engine behind "did you mean" diagnostics.
//!
//! Suggestions are hedged by construction: every message carries one of
//! `HEDGE_PHRASES` and never states the developer's intent as a fact. An
//! ambiguous match — two candidates equally close, or none close enough —
//! produces nothing at all, and the caller emits its plain error with no
//! candidate named.
//!
//! The engine matches names and nothing else. Candidates and their real
//! source spans come from the caller, which is what lets a suggestion point
//! at a declaration without this file ever knowing where anything is.
//!
//! **Invariants:**
//! - Producing nothing is the correct output whenever the match is not clear.
//! - No message asserts what the programmer meant; the hedge is contractual.

/// The phrase every suggestion message must contain, keeping it hedged.
pub const HEDGE_PHRASES: [&str; 1] = ["did you mean"];

/// Vocabulary a suggestion must never contain: bandaids, not fixes.
pub const BANDAID_TERMS: [&str; 5] = ["suppress", "silence", "stub", "comment out", "ignore this"];

/// A candidate for a suggestion: the name actually in scope and the real
/// source span of the declaration that name points at.
pub struct Candidate {
    pub name: String,
    pub file: i64,
    pub start: i64,
    pub end: i64,
}

/// A hedged suggestion: a message carrying the candidate's name, and the
/// candidate declaration's source span.
pub struct Suggestion {
    pub message: String,
    pub file: i64,
    pub start: i64,
    pub end: i64,
}

/// Case- and separator-insensitive form of a name for matching.
fn normalized(text: &str) -> String {
    let mut out = String::new();
    for ch in text.chars() {
        if ch != '_' {
            out.push(ch.to_ascii_lowercase());
        }
    }
    out
}

/// Levenshtein edit distance over bytes; one edit is distance 1.
fn levenshtein(a: &str, b: &str) -> usize {
    let a_bytes = a.as_bytes();
    let b_bytes = b.as_bytes();
    let mut previous: Vec<usize> = (0..=b_bytes.len()).collect();
    let mut current: Vec<usize> = vec![0usize; b_bytes.len() + 1];
    let mut row = 1usize;
    while row <= a_bytes.len() {
        current[0] = row;
        let mut col = 1usize;
        while col <= b_bytes.len() {
            let cost = if a_bytes[row - 1] == b_bytes[col - 1] { 0usize } else { 1usize };
            let deletion = previous[col] + 1;
            let insertion = current[col - 1] + 1;
            let substitution = previous[col - 1] + cost;
            let mut best = deletion;
            if insertion < best {
                best = insertion;
            }
            if substitution < best {
                best = substitution;
            }
            current[col] = best;
            col += 1;
        }
        std::mem::swap(&mut previous, &mut current);
        row += 1;
    }
    previous[b_bytes.len()]
}

/// The unique closest candidate within tolerance, or `None` when nothing is
/// close enough or two candidates tie.
pub fn best_match(misspelled: &str, candidates: &[String]) -> Option<String> {
    let target = normalized(misspelled);
    // Short names tolerate 1 edit, longer names 2.
    let limit = if target.len() >= 5 { 2usize } else { 1usize };
    let mut best_distance = usize::MAX;
    let mut best_name: Option<String> = None;
    let mut tied = 0usize;
    let mut idx = 0usize;
    while idx < candidates.len() {
        match candidates.get(idx) {
            Some(candidate) => {
                let distance = levenshtein(&target, &normalized(candidate));
                if distance <= limit {
                    if distance < best_distance {
                        best_distance = distance;
                        best_name = Some(candidate.clone());
                        tied = 1;
                    } else if distance == best_distance {
                        tied += 1;
                    }
                }
            }
            None => break,
        }
        idx += 1;
    }
    if tied == 1 {
        best_name
    } else {
        None
    }
}

/// A full hedged suggestion for `misspelled`, or `None` when ambiguous.
pub fn suggest(misspelled: &str, candidates: &[Candidate]) -> Option<Suggestion> {
    let names: Vec<String> = {
        let mut collected: Vec<String> = Vec::new();
        let mut idx = 0usize;
        while idx < candidates.len() {
            match candidates.get(idx) {
                Some(candidate) => collected.push(candidate.name.clone()),
                None => break,
            }
            idx += 1;
        }
        collected
    };
    let winner = best_match(misspelled, &names)?;
    let mut idx = 0usize;
    while idx < candidates.len() {
        match candidates.get(idx) {
            Some(candidate) => {
                if candidate.name == winner {
                    return Some(Suggestion {
                        message: format!("did you mean '{}'?", winner),
                        file: candidate.file,
                        start: candidate.start,
                        end: candidate.end,
                    });
                }
            }
            None => break,
        }
        idx += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|text| text.to_string()).collect()
    }

    #[test]
    fn unique_close_match_is_suggested() {
        let winner = best_match("cheksum", &names(&["checksum", "length", "value"]));
        assert_eq!(winner, Some("checksum".to_string()));
    }

    #[test]
    fn case_and_underscores_do_not_block_a_match() {
        let winner = best_match("checkmagic", &names(&["check_magic", "verify_magic"]));
        assert_eq!(winner, Some("check_magic".to_string()));
    }

    #[test]
    fn a_tie_is_neutral_and_names_nothing() {
        let winner = best_match("port", &names(&["post", "fort"]));
        assert_eq!(winner, None);
    }

    #[test]
    fn nothing_close_is_neutral() {
        let winner = best_match("zebra", &names(&["length", "value"]));
        assert_eq!(winner, None);
    }

    #[test]
    fn every_suggestion_is_hedged_and_never_a_bandaid() {
        let corpus = [
            ("cheksum", &["checksum", "check_range_workflow"][..]),
            ("vec_pus", &["vec_push", "vec_pop"][..]),
            ("string_lengh", &["string_len", "slice_len"][..]),
        ];
        let mut case = 0usize;
        while case < corpus.len() {
            let (misspelled, candidates) = corpus[case];
            let candidates: Vec<Candidate> = names(candidates)
                .into_iter()
                .map(|name| Candidate { name, file: 0, start: 0, end: 1 })
                .collect();
            match suggest(misspelled, &candidates) {
                Some(suggestion) => {
                    let hedged = HEDGE_PHRASES.iter().any(|phrase| suggestion.message.contains(phrase));
                    assert!(hedged, "suggestion '{}' is not hedged", suggestion.message);
                    let bandaid = BANDAID_TERMS.iter().any(|term| suggestion.message.to_lowercase().contains(term));
                    assert!(!bandaid, "suggestion '{}' names a bandaid", suggestion.message);
                }
                None => assert!(false, "{} should have produced a suggestion", misspelled),
            }
            case += 1;
        }
    }

    #[test]
    fn ambiguous_case_produces_no_candidate() {
        let candidates: Vec<Candidate> = names(&["post", "fort"])
            .into_iter()
            .map(|name| Candidate { name, file: 0, start: 0, end: 1 })
            .collect();
        assert!(suggest("port", &candidates).is_none());
    }
}
