use mdq_rg::markdown::split;
use mdq_rg::search::{
    excerpt::{exact_excerpt, excerpt},
    output::{json_string, output_lines},
    rank::rank,
    tokenize,
};
use mdq_rg::text::{normalize::normalize, query_terms as terms, tokenize as text_tokenize};
use mdq_rg::walk::fnmatch;

#[test]
fn frontmatter_fences_original_lines_and_hierarchy() {
    let source = "---\ndate: 2026-11-02\n---\nintro\n# Parent\nbody\n```rust\n# ignored\n```\n## Child\nchild\n# Other\nend\n";
    let s = split("a.md", source);
    assert_eq!(s.len(), 4);
    assert_eq!((s[0].start_line, s[0].end_line), (4, 4));
    assert_eq!(s[0].heading_path, "(preface)");
    assert_eq!((s[1].start_line, s[1].end_line), (5, 9));
    assert!(s[1].text.contains("# ignored"));
    assert_eq!(s[2].heading_path, "Parent > Child");
    assert_eq!((s[2].start_line, s[2].end_line), (10, 11));
    assert_eq!(s[3].heading_path, "Other");
    let s = split("b.md", "# A\n~~~\n## code\n~~~~\n## B\n");
    assert_eq!(s.len(), 2);
    assert_eq!(s[1].start_line, 5);
}

#[test]
fn normalization_and_bigram() {
    assert_eq!(normalize("ＡＺａｚ０９＿！　ｶﾞﾊﾟｳﾞ ①"), "azaz09_! ガパヴ ①");
    assert_eq!(
        text_tokenize::tokenize(&normalize("日本語 カナ あ ab_CD09 😀 e\u{301}")),
        ["日本", "本語", "カナ", "あ", "ab_cd09", "😀", "é"]
    );
    assert_eq!(
        normalize("｡｢｣､･ｦｧｨｩｪｫｬｭｮｯｰﾜﾝ"),
        "。「」、・ヲァィゥェォャュョッーワン"
    );
}

#[test]
fn glob_matches_posix_paths() {
    for (pattern, path, expected) in [
        ("docs/*.md", "docs/a/b.md", true),
        ("?.md", "a.md", true),
        ("?.md", "ab.md", false),
        ("[ab].md", "a.md", true),
        ("[!a-c].md", "b.md", false),
        ("[^a].md", "a.md", false),
        ("[^a].md", "b.md", true),
        ("[a-].md", "-.md", true),
        ("[a-].md", "a.md", true),
        ("[a-].md", "b.md", false),
        ("ab*", "ab", true),
        ("[a-c].md", "c.md", true),
        ("[[]x", "[x", true),
        ("[]].md", "].md", true),
        ("[.md", "[.md", true),
        ("*.md", "a.rs", false),
    ] {
        assert_eq!(fnmatch(pattern, path), expected, "{pattern} {path}");
    }
}

#[test]
fn json_controls_and_unicode() {
    assert_eq!(
        json_string("\"\\\n\r\t\u{0}\u{8}\u{1f}日本😀"),
        "\"\\\"\\\\\\n\\r\\t\\u0000\\u0008\\u001f日本😀\""
    );
}

#[test]
fn ranking_ties_and_budget() {
    let mut sections = split("b.md", "# Plain\nneedle\n");
    sections.extend(split("a.md", "# Plain\nneedle\n"));
    sections.extend(split("c.md", "# needle\nneedle needle needle\n"));
    sections.extend(split("d.md", "# Empty\nother\n"));
    let query = terms("needle");
    let (docs, n, avg) = tokenize(sections.clone(), &query);
    assert_eq!(n, 4);
    let ranked = rank(&docs, &query, n, avg);
    let paths: Vec<_> = ranked
        .iter()
        .map(|(i, _)| docs[*i].section.path.as_str())
        .collect();
    assert_eq!(paths, ["c.md", "a.md", "b.md"]);
    assert!(ranked.iter().all(|(_, s)| *s > 0.0));
    assert_eq!(output_lines(&docs, &ranked, &query, 5, 0).len(), 1);
    assert_eq!(output_lines(&docs, &ranked, &query, 5, 800).len(), 3);
    let sections = split("same.md", "# A\nneedle\n# B\nneedle\n");
    let (docs, n, avg) = tokenize(sections, &query);
    assert_eq!(rank(&docs, &query, n, avg)[0].0, 0);
}

#[test]
fn heading_weight_is_three_and_bm25_is_exact() {
    let sections = split("p.md", "# target\nbody\n");
    let query = terms("target");
    let (docs, n, avg) = tokenize(sections, &query);
    assert_eq!(docs[0].frequencies["target"], 3);
    assert_eq!(docs[0].length, 7);
    let scores = rank(&docs, &query, n, avg);
    let expected = (4.0_f64 / 3.0).ln() * 3.0 * 2.5 / 4.5;
    assert!((scores[0].1 - expected).abs() < 1e-12);

    let mut sections = split("p.md", "# target\nbody\n");
    sections.extend(split("q.md", "# other\none two three four five\n"));
    let (docs, n, avg) = tokenize(sections, &query);
    assert_eq!(docs[0].length, 7);
    assert_eq!(docs[1].length, 11);
    assert_eq!(avg, 9.0);
    let scores = rank(&docs, &query, n, avg);
    assert_eq!(scores.len(), 1);
    // N=2, df=1, tf=3, length=7, avgdl=9: idf=ln(2), denominator=133/30.
    let expected = 2.0_f64.ln() * 225.0 / 133.0;
    assert!((scores[0].1 - expected).abs() < 1e-12);
}

#[test]
fn literal_preface_heading_is_weighted_but_synthetic_preface_is_not() {
    let sections = split("p.md", "intro\n# (preface)\nbody\n");
    assert_eq!(sections.len(), 2);
    assert_eq!(sections[0].heading_path, "(preface)");
    assert_eq!(sections[1].heading_path, "(preface)");
    let (docs, _, _) = tokenize(sections, &terms("preface intro body"));
    assert_eq!(docs[0].frequencies.get("preface"), None);
    assert_eq!(docs[0].frequencies["intro"], 1);
    assert_eq!(docs[1].frequencies["preface"], 3);
    assert_eq!(docs[1].frequencies["body"], 1);
}

#[test]
fn excerpt_window_and_character_limit() {
    let section = split("a.md", "# H\n0\n1\n2\nneedle needle\n4\n5\n6").remove(0);
    assert_eq!(
        excerpt(&section, &["needle".into()]),
        "needle needle\n4\n5\n6"
    );
    let section = split("a.md", &"日".repeat(401)).remove(0);
    let text = excerpt(&section, &[]);
    assert_eq!(text.chars().count(), 400);
    assert!(text.ends_with('…'));
}

fn assert_excerpt_window(source: &str, query: &[&str], expected: &str) {
    let section = mdq_rg::markdown::Section {
        path: "a.md".into(),
        start_line: 1,
        end_line: source.lines().count(),
        heading_path: "(preface)".into(),
        text: source.into(),
        level: 0,
    };
    let query: Vec<_> = query.iter().map(|term| (*term).to_string()).collect();
    assert_eq!(excerpt(&section, &query), expected);
    assert_eq!(exact_excerpt(&section, &query), expected);
}

#[test]
fn excerpt_window_covers_terms_on_separate_lines_before_repeated_terms() {
    assert_excerpt_window(
        "alpha alpha alpha alpha\n0\n1\n2\n3\n4\nALPHA\n6\n7\n8\nbeta",
        &["alpha", "beta"],
        "ALPHA\n6\n7\n8\nbeta",
    );
}

#[test]
fn excerpt_window_starts_at_the_only_matching_line_and_includes_following_lines() {
    assert_excerpt_window(
        "intro\n0\n1\n2\nNEEDLE\nanswer\nexplanation\nexample\nsummary\nafter",
        &["needle"],
        "NEEDLE\nanswer\nexplanation\nexample\nsummary",
    );
}

#[test]
fn excerpt_window_breaks_coverage_ties_by_occurrences_then_start() {
    assert_excerpt_window(
        "alpha\n0\n1\n2\n3\n4\nalpha alpha\n6\n7\n8\n9\n10\nalpha alpha",
        &["alpha"],
        "alpha alpha\n6\n7\n8\n9",
    );
}

#[test]
fn excerpt_window_keeps_first_three_lines_without_visible_terms() {
    for query in [&[][..], &["absent"][..]] {
        assert_excerpt_window("0\n1\n2\n3\n4\n5\n6", query, "0\n1\n2");
    }
    assert_excerpt_window("", &["absent"], "");
    assert_excerpt_window("short", &["absent"], "short");
}

#[test]
fn excerpt_window_scores_only_terms_visible_after_truncation() {
    let source = format!(
        "alpha {} beta beta beta\n0\n1\n2\n3\nalpha beta",
        "x".repeat(400)
    );
    assert_excerpt_window(&source, &["alpha", "beta"], "alpha beta");
}

#[test]
fn excerpt_window_preserves_graphemes_at_character_limit() {
    let source = format!("{}e\u{301}needle", "x".repeat(398));
    let expected = format!("{}…", "x".repeat(398));
    assert_excerpt_window(&source, &["needle"], &expected);
}

#[test]
fn equal_frequency_shorter_section_ranks_above_earlier_longer_section() {
    let sections = split(
        "same.md",
        "# Plain\nneedle padding padding padding\n# Plain\nneedle\n",
    );
    let query = terms("needle");
    let (docs, n, avg) = tokenize(sections, &query);
    assert_eq!(docs.len(), 2);
    assert_eq!(docs[0].frequencies["needle"], 1);
    assert_eq!(docs[1].frequencies["needle"], 1);
    assert!(docs[0].length > docs[1].length);
    let ranked = rank(&docs, &query, n, avg);
    assert_eq!(ranked.len(), 2);
    assert_eq!(ranked[0].0, 1);
    assert!(ranked[0].1 > ranked[1].1);
}

#[test]
fn section_levels_follow_heading_depth_and_preface_is_zero() {
    let sections = split("a.md", "intro\n# A\n### B\n## C\n");
    assert_eq!(
        sections.iter().map(|s| s.level).collect::<Vec<_>>(),
        [0, 1, 3, 2]
    );
}

#[test]
fn preface_label_is_not_searchable_but_literal_content_is() {
    let query = terms("preface");
    let mut sections = split("a.md", "ordinary body\n");
    sections.extend(split("b.md", "literal preface\n"));
    sections.extend(split("c.md", "# preface\nother\n"));
    let (docs, n, avg) = tokenize(sections, &query);
    assert!(!docs[0].frequencies.contains_key("preface"));
    assert_eq!(docs[1].frequencies["preface"], 1);
    assert_eq!(docs[2].frequencies["preface"], 3);
    let ranked = rank(&docs, &terms("preface"), n, avg);
    assert_eq!(ranked.len(), 2);
    assert!(ranked.iter().all(|(i, _)| *i != 0));
}

#[test]
fn whitespace_prefaces_are_omitted_without_changing_heading_lines() {
    for text in ["", " \t\n  \n", "---\ntitle: example\n---\n \t\n"] {
        assert!(split("a.md", text).is_empty(), "{text:?}");
    }
    for (text, start) in [
        (" \t\n  \n# Heading\nbody\n", 3),
        ("---\ntitle: example\n---\n \t\n# Heading\nbody\n", 5),
    ] {
        let sections = split("a.md", text);
        assert_eq!(sections.len(), 1);
        assert_eq!(sections[0].start_line, start);
        assert_eq!(sections[0].heading_path, "Heading");
        assert_eq!(sections[0].text, "# Heading\nbody");
    }
    assert_eq!(split("a.md", "intro\n# Heading\n").len(), 2);
    assert_eq!(split("a.md", "# Heading\n \t\n").len(), 1);
}

#[test]
fn single_character_matches_inside_bigram() {
    let (docs, n, avg) = tokenize(split("a.md", "東京"), &terms("京"));
    assert_eq!(docs[0].frequencies["東京"], 1);
    assert!(!docs[0].frequencies.contains_key("京"));
    let ranked = rank(&docs, &terms("京"), n, avg);
    assert_eq!(ranked.len(), 1);
    assert!((ranked[0].1 - (4.0_f64 / 3.0).ln()).abs() < 1e-12);
}

#[test]
fn single_character_rank_covers_the_full_tokenizer_cjk_range() {
    for c in ['\u{303b}', '\u{31f0}', '\u{3347f}', '\u{3ffff}'] {
        let (docs, n, avg) = tokenize(split("a.md", &format!("中{c}国")), &terms(&c.to_string()));
        assert_eq!(
            rank(&docs, &terms(&c.to_string()), n, avg).len(),
            1,
            "{c:?}"
        );
    }
}
