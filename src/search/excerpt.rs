use crate::markdown::Section;
use crate::text::{fold_for_exact, normalize, truncate_graphemes};

pub fn excerpt(section: &Section, query: &[String]) -> String {
    excerpt_with_fold(section, query, normalize)
}

pub fn exact_excerpt(section: &Section, query: &[String]) -> String {
    excerpt_with_fold(section, query, fold_for_exact)
}

fn excerpt_with_fold(section: &Section, query: &[String], fold: fn(&str) -> String) -> String {
    let lines: Vec<_> = section.text.lines().collect();
    if lines.is_empty() {
        return String::new();
    }
    let mut best = truncate_graphemes(&lines[..3.min(lines.len())].join("\n"), 400);
    let mut terms: Vec<_> = query.iter().collect();
    terms.sort_unstable();
    terms.dedup();
    if terms.is_empty() {
        return best;
    }
    let mut best_score = (0, 0);
    for start in 0..lines.len() {
        let line = fold(lines[start]);
        if !terms.iter().any(|term| line.contains(term.as_str())) {
            continue;
        }
        let text = truncate_graphemes(&lines[start..(start + 5).min(lines.len())].join("\n"), 400);
        let folded = fold(&text);
        let mut score = (0, 0);
        for term in &terms {
            let hits = folded.matches(term.as_str()).count();
            score.0 += usize::from(hits > 0);
            score.1 += hits;
        }
        if score > best_score {
            best_score = score;
            best = text;
        }
    }
    best
}
