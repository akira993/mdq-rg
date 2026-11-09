use serde_json::Value;
use std::process::{Command, Output};

#[test]
fn rank_budget_counts_ascii_quarters_and_non_ascii_characters() {
    for (text, two_rows) in [("日本語".repeat(35), 262), ("a".repeat(105), 106)] {
        let root = tempfile::tempdir().unwrap();
        for name in ["a.md", "b.md", "c.md"] {
            std::fs::write(root.path().join(name), format!("# needle\n{text}\n")).unwrap();
        }
        let all = run(&root, &["--q", "needle", "--max-tokens", "10000"]);
        assert_eq!(rows(&all).len(), 3);
        for (budget, expected) in [(0, 1), (two_rows - 1, 1), (two_rows, 2)] {
            let output = run(
                &root,
                &["--q", "needle", "--max-tokens", &budget.to_string()],
            );
            assert_eq!(
                rows(&output).len(),
                expected,
                "budget={budget}, text={text}"
            );
        }
    }
}

fn run(root: &tempfile::TempDir, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mdrg"))
        .current_dir(root.path())
        .env("MDRG_NO_USAGE_LOG", "1")
        .arg("search")
        .args(args)
        .output()
        .unwrap()
}

fn rows(output: &Output) -> Vec<Value> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn assert_budget_boundaries(text: &str) {
    let root = tempfile::tempdir().unwrap();
    for name in ["a.md", "b.md", "c.md"] {
        std::fs::write(root.path().join(name), format!("# needle\n{text}\n")).unwrap();
    }
    for mode in ["exact", "rank"] {
        let all = run(
            &root,
            &["--q", "needle", "--mode", mode, "--max-tokens", "10000"],
        );
        assert_eq!(rows(&all).len(), 3);
        let stdout = String::from_utf8(all.stdout).unwrap();
        let lines: Vec<_> = stdout.lines().collect();
        let mut total = 0;
        for (index, line) in lines.iter().enumerate() {
            let (ascii, non_ascii) = line.chars().fold((0usize, 0usize), |(a, n), c| {
                if c.is_ascii() { (a + 1, n) } else { (a, n + 1) }
            });
            total += non_ascii + ascii.div_ceil(4);
            let boundaries = if index == 0 {
                vec![(1, 1)]
            } else {
                vec![(total - 1, index), (total, index + 1)]
            };
            for (budget, expected) in boundaries {
                let output = run(
                    &root,
                    &[
                        "--q",
                        "needle",
                        "--mode",
                        mode,
                        "--max-tokens",
                        &budget.to_string(),
                    ],
                );
                assert_eq!(
                    rows(&output).len(),
                    expected,
                    "mode={mode}, budget={budget}, text={text}"
                );
                assert_eq!(
                    String::from_utf8(output.stdout).unwrap(),
                    format!("{}\n", lines[..expected].join("\n")),
                    "mode={mode}, budget={budget}"
                );
            }
        }
    }
}

#[test]
fn both_modes_count_japanese_budget_and_preserve_output_prefix() {
    assert_budget_boundaries(&"日本語".repeat(35));
}

#[test]
fn both_modes_round_ascii_budget_per_json_line() {
    for length in 105..109 {
        assert_budget_boundaries(&"a".repeat(length));
    }
}

fn diagnostic(output: &Output) -> Value {
    assert!(rows(output).is_empty());
    serde_json::from_slice(&output.stderr).unwrap()
}

#[test]
fn rank_schema_heading_only_match_and_extension_exclusion() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("plain.md"), "# needle\nother\n").unwrap();
    let out = run(&root, &["--q", "needle"]);
    let rows = rows(&out);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["lines"], serde_json::json!([1, 2]));
    assert_eq!(rows[0]["heading_path"], "needle");
    assert_eq!(rows[0]["snippet"], "# needle\nother");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with(
        "{\"path\":\"plain.md\",\"lines\":[1,2],\"heading_path\":\"needle\",\"score\":"
    ));
    assert_eq!(rows[0].as_object().unwrap().len(), 5);
    assert_eq!(
        diagnostic(&run(&root, &["--q", "md"]))["no_results"]["cause"],
        "no_match"
    );
}

#[test]
fn dotted_i_matches_ascii_in_both_directions_and_modes() {
    for (document, query) in [("İSTANBUL", "istanbul"), ("istanbul", "İSTANBUL")] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.md"), document).unwrap();
        for mode in ["rank", "exact"] {
            let found = rows(&run(&root, &["--q", query, "--mode", mode]));
            assert_eq!(found.len(), 1, "{mode}: {document:?}, {query:?}");
            assert_eq!(found[0]["path"], "a.md");
            assert_eq!(found[0]["snippet"], document);
            assert!(found[0]["score"].as_f64().unwrap() > 0.0);
            if mode == "exact" {
                assert_eq!(found[0]["match"], "phrase");
                assert_eq!(found[0]["score"], 1);
            }
        }
    }
}

#[test]
fn dotted_i_canonical_spellings_match_in_both_directions_and_modes() {
    for (left, right) in [
        ("I\u{307}STANBUL", "İSTANBUL"),
        ("İSTANBUL", "I\u{307}STANBUL"),
        ("İSTANBUL", "i\u{307}stanbul"),
        ("İ\u{301}STANBUL", "I\u{307}\u{301}STANBUL"),
        ("İ\u{323}STANBUL", "I\u{323}\u{307}STANBUL"),
        ("İ\u{301}STANBUL", "i\u{301}\u{307}stanbul"),
        ("İ\u{323}STANBUL", "i\u{307}\u{323}stanbul"),
        ("Í\u{307}STANBUL", "I\u{301}\u{307}STANBUL"),
        ("Ị\u{307}STANBUL", "I\u{307}\u{323}STANBUL"),
    ] {
        for (document, query) in [(left, right), (right, left)] {
            let root = tempfile::tempdir().unwrap();
            std::fs::write(
                root.path().join("a.md"),
                format!("# Locations\nintro\ncontext\npreface\n\n\n{document}\nnext\nlast\n"),
            )
            .unwrap();
            for mode in ["rank", "exact"] {
                let found = rows(&run(&root, &["--q", query, "--mode", mode]));
                assert_eq!(found.len(), 1, "{mode}: {document:?}, {query:?}");
                assert_eq!(found[0]["path"], "a.md");
                assert_eq!(found[0]["lines"], serde_json::json!([1, 9]));
                assert_eq!(found[0]["snippet"], format!("{document}\nnext\nlast"));
                assert!(found[0]["score"].as_f64().unwrap() > 0.0);
                if mode == "exact" {
                    assert_eq!(found[0]["match"], "phrase");
                    assert_eq!(found[0]["score"], 1);
                }
            }
        }
    }
}

#[test]
fn dotted_i_folding_preserves_other_unicode_searches() {
    for (document, query, expected) in [
        ("ıstanbul", "istanbul", 0),
        ("istanbul", "ıstanbul", 0),
        ("i\u{307}stanbul", "istanbul", 1),
        ("istanbul", "i\u{307}stanbul", 1),
        ("i\u{307}stanbul", "I\u{307}STANBUL", 1),
        ("ė", "E\u{307}", 1),
        ("E\u{307}", "ė", 1),
        ("ė", "e", 0),
        ("e", "ė", 0),
        ("ı\u{307}stanbul", "istanbul", 0),
        ("istanbul", "ı\u{307}stanbul", 0),
        ("cafe\u{301}", "CAFÉ", 1),
        ("café", "CAFE\u{301}", 1),
        ("cafe\u{301}", "cafe", 0),
        ("cafe", "café", 0),
        ("q\u{301}", "Q\u{301}", 1),
    ] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("a.md"), document).unwrap();
        for mode in ["rank", "exact"] {
            let found = rows(&run(&root, &["--q", query, "--mode", mode]));
            assert_eq!(found.len(), expected, "{mode}: {document:?}, {query:?}");
        }
    }
}

#[test]
fn exact_phrase_precedes_words_and_single_word_has_only_phrase() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("a.md"),
        "# first\nalpha x beta alpha x beta\n",
    )
    .unwrap();
    std::fs::write(root.path().join("z.md"), "# second\nＡＬＰＨＡ　ＢＥＴＡ\n").unwrap();
    let found = rows(&run(&root, &["--q", "alpha beta", "--mode", "exact"]));
    assert_eq!(found.len(), 2);
    assert_eq!(found[0]["path"], "z.md");
    assert_eq!(found[0]["match"], "phrase");
    assert_eq!(found[0]["score"], 1);
    assert_eq!(found[1]["match"], "words");
    assert_eq!(found[1]["score"], 4);
    let found = rows(&run(&root, &["--q", "alpha", "--mode", "exact"]));
    assert_eq!(found[0]["path"], "a.md");
    assert!(found.iter().all(|r| r["match"] == "phrase"));
}

#[test]
fn exact_ties_use_path_then_line_and_ignore_paths_as_text() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("b.md"), "# needle\n# needle\n").unwrap();
    std::fs::write(root.path().join("a.md"), "# needle\n# needle\n").unwrap();
    let found = rows(&run(&root, &["--q", "needle", "--mode", "exact"]));
    assert_eq!(
        found
            .iter()
            .map(|r| (r["path"].as_str().unwrap(), r["lines"][0].as_u64().unwrap()))
            .collect::<Vec<_>>(),
        [("a.md", 1), ("a.md", 2), ("b.md", 1), ("b.md", 2)]
    );
    assert_eq!(
        diagnostic(&run(&root, &["--q", "a.md", "--mode", "exact"]))["no_results"]["cause"],
        "no_match"
    );
}

#[test]
fn all_four_empty_causes_and_exact_word_counts() {
    let root = tempfile::tempdir().unwrap();
    let hint = diagnostic(&run(&root, &["--q", "needle"]));
    assert_eq!(hint["no_results"]["cause"], "no_documents");
    assert!(hint.to_string().contains("--hidden"));
    assert!(!hint.to_string().contains("--no-ignore"));
    std::fs::write(root.path().join("a.md"), "# alpha\n# beta\n").unwrap();
    assert_eq!(
        diagnostic(&run(&root, &["--q", "needle", "--paths", "missing/*"]))["no_results"]["cause"],
        "paths_matched_nothing"
    );
    for mode in ["rank", "exact"] {
        assert_eq!(
            diagnostic(&run(&root, &["--q", "  ", "--mode", mode]))["no_results"]["cause"],
            "empty_query"
        );
        assert_eq!(
            diagnostic(&run(&root, &["--q", "unfindable", "--mode", mode]))["no_results"]["cause"],
            "no_match"
        );
    }
    let hint = diagnostic(&run(&root, &["--q", "alpha beta", "--mode", "exact"]));
    assert_eq!(
        hint["no_results"]["words"],
        serde_json::json!({"alpha":1,"beta":1})
    );
    assert!(hint.to_string().contains("--mode rank"));
}

#[test]
fn skipped_documents_continue_and_timing_counts_readable_bytes() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("bad.md"), [0xff]).unwrap();
    std::fs::write(root.path().join("nul.md"), b"a\0b").unwrap();
    for mode in ["rank", "exact"] {
        let output = run(&root, &["--q", "needle", "--mode", mode, "--timing"]);
        assert!(rows(&output).is_empty());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains("invalid_utf8"));
        assert!(error.contains("contains_nul"));
        assert!(error.contains("files=0 bytes=0 sections=0 skipped=2"));
        assert!(error.contains("\"cause\":\"no_documents\""));
    }
    std::fs::write(root.path().join("ok.md"), "# needle\n").unwrap();
    let output = run(&root, &["--q", "needle", "--timing"]);
    assert_eq!(rows(&output).len(), 1);
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("files=1 bytes=9 sections=1 skipped=2")
    );
}

#[test]
fn snippet_limits_budget_and_top_k_apply_to_both_modes() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("a.md"),
        format!("# needle\n{}\n# needle\nneedle\n", "x".repeat(500)),
    )
    .unwrap();
    for mode in ["rank", "exact"] {
        let found = rows(&run(
            &root,
            &["--q", "needle", "--mode", mode, "--max-tokens", "0"],
        ));
        assert_eq!(found.len(), 1);
        assert!(found[0]["snippet"].as_str().unwrap().chars().count() <= 400);
        assert_eq!(
            rows(&run(
                &root,
                &["--q", "needle", "--mode", mode, "--top-k", "1"]
            ))
            .len(),
            1
        );
    }
}

#[test]
fn exact_repeated_words_contribute_for_each_query_word() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.md"), "# alpha\n").unwrap();
    let found = rows(&run(&root, &["--q", "alpha alpha", "--mode", "exact"]));
    assert_eq!(found[0]["match"], "words");
    assert_eq!(found[0]["score"], 2);
    let found = rows(&run(&root, &["--q", " alpha ", "--mode", "exact"]));
    assert_eq!(found[0]["match"], "phrase");
    assert_eq!(found[0]["score"], 1);
}

#[test]
fn usage_outcome_records_search_arguments_hits_and_skips() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("ok.md"),
        "# first\nneedle\n# second\nneedle\n",
    )
    .unwrap();
    std::fs::write(root.path().join("bad.md"), [0xff]).unwrap();
    std::fs::write(root.path().join("nul.md"), b"a\0b").unwrap();
    for (index, mode) in ["rank", "exact"].into_iter().enumerate() {
        let output = Command::new(env!("CARGO_BIN_EXE_mdrg"))
            .current_dir(root.path())
            .env("XDG_STATE_HOME", state.path())
            .env_remove("MDRG_NO_USAGE_LOG")
            .args([
                "search",
                "--q",
                "needle",
                "--mode",
                mode,
                "--top-k",
                "2",
                "--max-tokens",
                "100",
            ])
            .output()
            .unwrap();
        assert_eq!(rows(&output).len(), 2);
        let log = std::fs::read_to_string(state.path().join("mdrg/usage.jsonl")).unwrap();
        let entries: Vec<Value> = log
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(entries.len(), index + 1);
        let entry = entries.last().unwrap();
        assert_eq!(entry["exit"], 0);
        assert_eq!(entry["command"], "search");
        assert_eq!(
            entry["args"],
            serde_json::json!({"q":"needle", "mode":mode, "top_k":2, "max_tokens":100})
        );
        assert_eq!(entry["result"], serde_json::json!({"hits":2,"skipped":2}));
    }
}

#[test]
fn exact_scores_count_phrase_occurrences_and_sum_word_occurrences() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.md"), "# Heading\nred blue red blue\n").unwrap();
    std::fs::write(
        root.path().join("b.md"),
        "# Heading\nred x blue red x blue blue\n",
    )
    .unwrap();
    let found = rows(&run(&root, &["--q", "red blue", "--mode", "exact"]));
    assert_eq!(found.len(), 2);
    assert_eq!(found[0]["path"], "a.md");
    assert_eq!(found[0]["match"], "phrase");
    assert_eq!(found[0]["score"], 2);
    assert_eq!(found[1]["path"], "b.md");
    assert_eq!(found[1]["match"], "words");
    assert_eq!(found[1]["score"], 5);
}

#[test]
fn exact_trims_surrounding_query_whitespace_before_phrase_matching() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.md"), "needle").unwrap();
    for query in [" needle ", "\tneedle\n", "　needle　"] {
        let found = rows(&run(&root, &["--q", query, "--mode", "exact"]));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0]["match"], "phrase");
        assert_eq!(found[0]["score"], 1);
    }
    std::fs::write(root.path().join("a.md"), "red blue").unwrap();
    let found = rows(&run(&root, &["--q", " red blue ", "--mode", "exact"]));
    assert_eq!(found[0]["match"], "phrase");
    assert_eq!(found[0]["score"], 1);
}

#[test]
fn rank_omits_blank_bodies_unless_all_matches_are_blank() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("a.md"),
        "# needle\n \t\n# other\ncontent\n",
    )
    .unwrap();
    assert_eq!(rows(&run(&root, &["--q", "needle"])).len(), 1);
    std::fs::write(root.path().join("b.md"), "# needle\ncontent\n").unwrap();
    let found = rows(&run(&root, &["--q", "needle"]));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["path"], "b.md");
    assert_eq!(
        rows(&run(&root, &["--q", "needle", "--mode", "exact"])).len(),
        2
    );
}

#[test]
fn rank_excerpt_starts_at_underscore_surface_instead_of_its_parts() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("a.md"),
        "# Settings\nrelease notes\nintro\nset release_log to true\nnext\nlast\n",
    )
    .unwrap();
    let found = rows(&run(&root, &["--q", "release_log"]));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["snippet"], "set release_log to true\nnext\nlast");
}

#[test]
fn rank_excerpt_keeps_explicit_words_that_are_also_underscore_parts() {
    let root = tempfile::tempdir().unwrap();
    for (query, word) in [
        ("foo_bar foo", "foo"),
        ("foo foo_bar", "foo"),
        ("foo_bar foo foo", "foo"),
        ("foo_foo foo", "foo"),
        ("foo_bar bar_baz foo", "foo"),
        ("ＦＯＯ_ＢＡＲ ＦＯＯ", "foo"),
        ("foo_bar bar", "bar"),
    ] {
        std::fs::write(
            root.path().join("a.md"),
            format!("# Settings\nintro\ncontext\nthe {word} line is here\nnext\nlast\n"),
        )
        .unwrap();
        let expected = format!("the {word} line is here\nnext\nlast");
        for query in [query, word] {
            let found = rows(&run(&root, &["--q", query]));
            assert_eq!(found.len(), 1, "{query}");
            assert_eq!(found[0]["snippet"], expected, "{query}");
        }
        let found = rows(&run(&root, &["--q", "foo_bar"]));
        assert_eq!(found.len(), 1);
        assert_eq!(found[0]["snippet"], "# Settings\nintro\ncontext");
    }
}

#[test]
fn rank_treats_literal_preface_headings_like_other_blank_headings() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.md"), "# (preface)\n").unwrap();
    std::fs::write(root.path().join("b.md"), "# preface\n").unwrap();
    std::fs::write(root.path().join("c.md"), "# Other\npreface body\n").unwrap();
    let found = rows(&run(&root, &["--q", "preface"]));
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["path"], "c.md");
}

#[test]
fn rank_matches_filename_words_without_body_matches() {
    for filename in ["08_porting-guide.md", "08-porting_guide.md"] {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join(filename), "# Other\nunrelated content\n").unwrap();
        for query in ["porting", "guide"] {
            let found = rows(&run(&root, &["--q", query]));
            assert_eq!(found.len(), 1, "{filename}: {query}");
            assert_eq!(found[0]["path"], filename);
        }
    }
}

#[test]
fn query_may_start_with_a_dash() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.md"), "# needle\n--validate option\n").unwrap();
    for query in ["--validate needle", "- needle", "-v needle"] {
        let output = run(&root, &["--q", query]);
        assert!(output.status.success(), "{query}: {output:?}");
        assert_eq!(rows(&output).len(), 1, "{query}");
    }
    let unknown = run(&root, &["--q", "needle", "--validate"]);
    assert_eq!(unknown.status.code(), Some(2));
}
