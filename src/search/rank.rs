use super::Document;
use crate::text::tokenize::{contains_single, expands_single};
use std::collections::HashSet;

pub fn rank(docs: &[Document], query: &[String], n: usize, avgdl: f64) -> Vec<(usize, f64)> {
    let unique: HashSet<&String> = query.iter().collect();
    let mut idfs: Vec<_> = unique
        .into_iter()
        .map(|term| {
            let df = docs.iter().filter(|d| term_frequency(d, term) > 0).count() as f64;
            (term, (1.0 + (n as f64 - df + 0.5) / (df + 0.5)).ln())
        })
        .collect();
    // Stable term order also fixes floating point summation order between processes.
    idfs.sort_by(|a, b| a.0.cmp(b.0));
    let mut scores: Vec<_> = docs
        .iter()
        .enumerate()
        .filter_map(|(i, doc)| {
            let score: f64 = idfs
                .iter()
                .map(|(term, idf)| {
                    let tf = term_frequency(doc, term) as f64;
                    idf * tf * 2.5 / (tf + 1.5 * (0.8 + 0.2 * doc.length as f64 / avgdl))
                })
                .sum();
            (score > 0.0).then_some((i, score))
        })
        .collect();
    scores.sort_by(|(a, sa), (b, sb)| {
        sb.total_cmp(sa)
            .then_with(|| docs[*a].section.path.cmp(&docs[*b].section.path))
            .then_with(|| {
                docs[*a]
                    .section
                    .start_line
                    .cmp(&docs[*b].section.start_line)
            })
    });
    let nonempty: Vec<_> = scores
        .iter()
        .copied()
        .filter(|(i, _)| has_body(super::body(&docs[*i].section)))
        .collect();
    if nonempty.is_empty() {
        scores
    } else {
        nonempty
    }
}

fn has_body(mut body: &str) -> bool {
    let mut visible = String::with_capacity(body.len());
    while let Some((before, comment)) = body.split_once("<!--") {
        visible.push_str(before);
        body = comment.split_once("-->").map_or("", |(_, rest)| rest);
    }
    visible.push_str(body);
    visible.lines().any(|line| {
        if line.trim().is_empty() {
            return false;
        }
        let content = line.trim_start_matches(' ');
        if line.len() - content.len() > 3 {
            return true;
        }
        let marker = content.as_bytes()[0];
        !(matches!(marker, b'-' | b'*' | b'_')
            && content
                .bytes()
                .all(|byte| byte == marker || matches!(byte, b' ' | b'\t'))
            && content.bytes().filter(|&byte| byte == marker).count() >= 3)
    })
}

fn term_frequency(doc: &Document, term: &str) -> usize {
    if expands_single(term) {
        doc.frequencies
            .iter()
            .filter(|(token, _)| contains_single(token, term))
            .map(|(_, count)| count)
            .sum()
    } else {
        *doc.frequencies.get(term).unwrap_or(&0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(path: &str, tokens: &[(&str, usize)]) -> Document {
        Document {
            section: crate::markdown::split(path, "# Heading\nbody").remove(0),
            frequencies: tokens.iter().map(|(t, n)| (t.to_string(), *n)).collect(),
            length: 10,
        }
    }

    #[test]
    fn single_character_tf_sums_containing_bigrams_once() {
        let d = doc(
            "a.md",
            &[
                ("\u{4e2d}\u{6587}", 2),
                ("\u{6587}\u{66f8}", 3),
                ("\u{6587}", 4),
                ("\u{6587}\u{5b57}\u{5217}", 50),
            ],
        );
        assert_eq!(term_frequency(&d, "\u{6587}"), 9);
        assert_eq!(term_frequency(&d, "\u{4e2d}\u{6587}"), 2);
        let d = doc("c.md", &[("\u{d55c}\u{ad6d}", 2), ("\u{ad6d}\u{c5b4}", 3)]);
        assert_eq!(term_frequency(&d, "\u{ad6d}"), 5);
        let d = doc("d.md", &[("a", 1), ("at", 20)]);
        assert_eq!(term_frequency(&d, "a"), 1);
    }
}
