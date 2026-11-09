pub mod exact;
pub mod excerpt;
pub mod hints;
pub mod output;
pub mod rank;

use crate::markdown::Section;
use crate::text::{document_counts, normalize};
use rayon::prelude::*;
use std::collections::HashMap;

pub struct Document {
    pub section: Section,
    pub frequencies: HashMap<String, usize>,
    pub length: usize,
}

pub fn body(section: &Section) -> &str {
    if section.level == 0 {
        &section.text
    } else {
        section.text.split_once('\n').map_or("", |(_, rest)| rest)
    }
}

pub fn tokenize(sections: Vec<Section>, query: &[String]) -> (Vec<Document>, usize, f64) {
    let n = sections.len();
    let filter = crate::text::tokenize::Filter::new(query.iter());
    let docs: Vec<_> = sections
        .into_par_iter()
        .map(|section| {
            let heading = if section.level == 0 {
                ""
            } else {
                &section.heading_path
            };
            let path = section.path.strip_suffix(".md").unwrap_or(&section.path);
            let weighted = normalize(&format!(
                "{path} {path} {path} {heading} {heading} {heading} {}",
                body(&section)
            ));
            let (frequencies, length) = document_counts(&weighted, &filter);
            Document {
                section,
                frequencies,
                length,
            }
        })
        .collect();
    let total: f64 = docs.iter().map(|d| d.length as f64).sum();
    (
        docs,
        n,
        if n == 0 {
            1.0
        } else {
            (total / n as f64).max(f64::EPSILON)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_and_headings_are_weighted_three_times_with_body_length_retained() {
        let query = crate::text::query_terms("route title body");
        let sections = crate::markdown::split("route.md", "# title\nbody\n# blank\n \t\n");
        let (docs, n, avgdl) = tokenize(sections, &query);
        assert_eq!(n, 2);
        assert_eq!(docs[0].frequencies["route"], 3);
        assert_eq!(docs[0].frequencies["title"], 3);
        assert_eq!(docs[0].frequencies["body"], 1);
        assert_eq!(docs[0].length, 7);
        assert_eq!(avgdl, 6.5);
        let ranked = rank::rank(&docs, &query, n, avgdl);
        assert_eq!(ranked.len(), 1);
        let idf = (1.0_f64 + 0.5 / 2.5).ln();
        let other_idf = (1.0_f64 + 1.5 / 1.5).ln();
        let norm = 1.5 * (0.8 + 0.2 * 7.0 / avgdl);
        let expected = idf * 3.0 * 2.5 / (3.0 + norm)
            + other_idf * 3.0 * 2.5 / (3.0 + norm)
            + other_idf * 2.5 / (1.0 + norm);
        assert!((ranked[0].1 - expected).abs() < 1e-12);
    }
}
