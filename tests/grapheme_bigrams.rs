use mdq_rg::text::tokenize::{self, Filter, contains_single, expands_single};
use mdq_rg::text::{document_counts, normalize, query_terms};
use unicode_segmentation::UnicodeSegmentation;

#[path = "common/text.rs"]
mod text;
use text::document_tokens;

#[test]
fn cjk_bigrams_start_and_end_at_grapheme_boundaries() {
    for text in [
        "あ\u{3099}いう",
        "あ\u{309a}いう",
        "あ\u{3099}\u{309a}い",
        "葛\u{e0100}飾区",
        "葛\u{fe00}飾",
        "\u{1100}\u{1100}\u{1161}나다",
        "가\u{11a8}\u{11a8}나다",
        "\u{a960}\u{1160}\u{d7cb}나다",
        "東京\u{301}都",
        "東\u{200d}京都",
    ] {
        for normalized in [text.to_string(), normalize(text)] {
            let graphemes: Vec<_> = normalized.graphemes(true).collect();
            let expected: Vec<_> = if graphemes.len() == 1 {
                vec![normalized.clone()]
            } else {
                graphemes.windows(2).map(|pair| pair.concat()).collect()
            };
            let actual = tokenize::tokenize(&normalized);
            let length = tokenize::for_each(&normalized, &mut |_| {});
            assert_eq!(actual, expected, "{normalized:?}");
            assert_eq!(length, expected.len(), "{normalized:?}");
        }
    }
}

#[test]
fn grapheme_queries_match_document_counts_without_partial_matches() {
    for (text, single, partial) in [
        ("あ\u{3099}いう", "あ\u{3099}", "あ"),
        ("あ\u{309a}いう", "あ\u{309a}", "あ"),
        ("葛\u{e0100}飾区", "葛\u{e0100}", "葛"),
        (
            "\u{1100}\u{1100}\u{1161}나다",
            "\u{1100}\u{1100}\u{1161}",
            "\u{1100}",
        ),
    ] {
        let text = normalize(text);
        let single = normalize(single);
        assert!(expands_single(&single));

        for query_text in [&text, &single] {
            let query = query_terms(query_text);
            let terms = &query;
            let (counts, length) = document_counts(&text, &Filter::new(terms.iter()));
            let (all, all_length) = document_tokens(&text);
            assert_eq!(length, all_length);
            for term in terms {
                let expected = all
                    .iter()
                    .filter(|token| {
                        if term.graphemes(true).count() == 1 {
                            token.graphemes(true).any(|g| g == term)
                        } else {
                            *token == term
                        }
                    })
                    .count();
                let actual: usize = counts
                    .iter()
                    .filter(|(token, _)| {
                        if expands_single(term) {
                            contains_single(token, term)
                        } else {
                            *token == term
                        }
                    })
                    .map(|(_, count)| count)
                    .sum();
                assert!(actual > 0, "{text:?} {term:?}");
                assert_eq!(actual, expected, "{text:?} {term:?}");
            }
        }
        let partial = partial.to_string();
        let (counts, _) = document_counts(&text, &Filter::new(std::iter::once(&partial)));
        assert!(counts.is_empty(), "{text:?} {partial:?}");
    }
}

#[test]
fn ordinary_japanese_bigrams_are_unchanged() {
    assert_eq!(
        tokenize::tokenize("東京都で検索する 日本語の文章"),
        [
            "東京", "京都", "都で", "で検", "検索", "索す", "する", "日本", "本語", "語の", "の文",
            "文章"
        ]
    );
}

#[test]
fn single_grapheme_matching_rejects_partial_and_longer_tokens() {
    assert!(!contains_single("あ\u{3099}い", "あ"));
    assert!(contains_single("あ\u{3099}い", "い"));
    assert!(!contains_single("あ\u{3099}いう", "い"));
}

#[test]
fn ranked_search_matches_complete_cjk_graphemes() {
    for (text, single, partial) in [
        ("あ\u{3099}いう", "あ\u{3099}", "あ"),
        ("あ\u{309a}いう", "あ\u{309a}", "あ"),
        ("葛\u{e0100}飾区", "葛\u{e0100}", "葛"),
        (
            "\u{1100}\u{1100}\u{1161}나다",
            "\u{1100}\u{1100}\u{1161}",
            "\u{1100}",
        ),
    ] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("sample.md"), format!("# sample\n{text}\n")).unwrap();
        for query in [text, single, partial] {
            let output = std::process::Command::new(env!("CARGO_BIN_EXE_mdrg"))
                .current_dir(root.path())
                .env("MDRG_NO_USAGE_LOG", "1")
                .args(["search", "--q", query])
                .output()
                .unwrap();
            assert!(output.status.success(), "{:?}", output.stderr);
            if query == partial {
                assert!(output.stdout.is_empty(), "{text:?} {query:?}");
            } else {
                let row: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(row["path"], "sample.md");
                assert!(row["score"].as_f64().unwrap() > 0.0);
            }
        }
    }
}
