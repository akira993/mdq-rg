use mdq_rg::text::tokenize::Filter;
use mdq_rg::text::{document_counts, normalize, query_terms};
use std::collections::HashMap;
use unicode_segmentation::UnicodeSegmentation;

#[path = "common/text.rs"]
mod text;
use text::document_tokens;

// Deliberately independent of the production single-character helpers.
fn reference_frequency(counts: &HashMap<String, usize>, term: &str) -> usize {
    let c = term.chars().next();
    let single_cjk =
        term.graphemes(true).count() == 1 && c.is_some_and(mdq_rg::text::tokenize::cjk);
    if single_cjk {
        counts
            .iter()
            .filter(|(token, _)| {
                let graphemes: Vec<_> = token.graphemes(true).collect();
                graphemes.len() <= 2 && graphemes.contains(&term)
            })
            .map(|(_, n)| n)
            .sum()
    } else {
        counts.get(term).copied().unwrap_or(0)
    }
}

#[test]
fn query_counts_match_all_tokens_and_surface_lengths() {
    for (text, query_text) in [
        (
            "release__log 123_456 release_release",
            "release_log release 123_456",
        ),
        ("中文文書 文 文文 文字列 東京大学", "文 京 東京大学"),
        ("한국어 한국 국어 국", "국 한국"),
        ("あ\u{3099}いう あ\u{309a}いう", "あ\u{3099} あ\u{309a} い"),
        ("葛\u{e0100}飾区", "葛\u{e0100} 葛 飾"),
        (
            "\u{1100}\u{1100}\u{1161}나다",
            "\u{1100}\u{1100}\u{1161} 나",
        ),
        ("食べました 食べる 読んだ 読む", "食べる 読む"),
        ("東京 トウキョウ 学校 ガッコウ", "とうきょう がっこう"),
        ("あ ア 々 ㇰ 中\u{3347f}国", "あ ア 々 ㇰ \u{3347f}"),
        (
            "hello HELLO é e\u{301} 😀 1️⃣ irrelevant",
            "hello é 😀 1️⃣ missing",
        ),
    ] {
        let query = query_terms(query_text);

        let normalized = normalize(text);
        let (tokens, length) = document_tokens(&normalized);
        let mut all = HashMap::new();
        for token in tokens {
            *all.entry(token).or_insert(0) += 1;
        }
        let filter = Filter::new(query.iter());
        let (restricted, restricted_length) = document_counts(&normalized, &filter);
        assert_eq!(length, restricted_length, "{text}");
        for term in &query {
            assert_eq!(
                reference_frequency(&all, term),
                reference_frequency(&restricted, term),
                "{term} {text}"
            );
        }
        let (counts, empty_length) = document_counts(&normalized, &Filter::new([].iter()));
        assert!(counts.is_empty());
        assert_eq!(length, empty_length);
    }
}

#[test]
fn single_character_counts_are_exercised() {
    for (text, term) in [("한국어", "국"), ("中文書", "文")] {
        let term = term.to_string();
        let filter = Filter::new(std::iter::once(&term));
        let (counts, _) = document_counts(&normalize(text), &filter);
        assert!(reference_frequency(&counts, &term) > 0, "{text} {term}");
    }
}
