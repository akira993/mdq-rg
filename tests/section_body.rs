use mdq_rg::{markdown::split, search, text::query_terms};
use serde_json::Value;
use std::process::Command;

fn search(root: &tempfile::TempDir, mode: &str) -> Vec<Value> {
    let output = Command::new(env!("CARGO_BIN_EXE_mdrg"))
        .current_dir(root.path())
        .env("MDRG_NO_USAGE_LOG", "1")
        .args([
            "search",
            "--q",
            "needle",
            "--mode",
            mode,
            "--top-k",
            "100",
            "--max-tokens",
            "10000",
        ])
        .output()
        .unwrap();
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

fn compare_body(body: &str, retained: bool) {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.md"), format!("# needle\n{body}\n")).unwrap();
    std::fs::write(
        root.path().join("z.md"),
        "# needle\ntext text text text text\n",
    )
    .unwrap();
    let found = search(&root, "rank");
    assert_eq!(found.len(), if retained { 2 } else { 1 }, "{body:?}");
    assert_eq!(
        found.iter().any(|row| row["path"] == "a.md"),
        retained,
        "{body:?}"
    );
    assert!(found.iter().any(|row| row["path"] == "z.md"));
}

#[test]
fn rank_excludes_comment_only_bodies() {
    for body in [
        "",
        " \t\n\u{3000}",
        "<!-- c -->",
        " \t<!-- c -->\n ",
        "<!--\nc\n-->",
        "<!-- first -->\n<!-- second -->",
        "<!-- first --><!-- second -->",
        "<!--",
        "<!-- c\ntext",
        "<!-- outer <!-- inner -->",
        "<!-- c -->\n---\n<!-- unclosed",
    ] {
        compare_body(body, false);
    }
}

#[test]
fn rank_excludes_thematic_break_only_bodies() {
    for body in [
        "---",
        "***",
        "___",
        "- - -",
        "   ---",
        " * * * \t",
        "__\t_",
        "---\n***\n___",
        "--<!-- c -->-",
        "-<!-- c\nmore -->--",
    ] {
        compare_body(body, false);
    }
}

#[test]
fn rank_keeps_lines_outside_the_thematic_break_definition() {
    for body in [
        "    ---",
        "\t---",
        "  \t---",
        "--",
        "-*-",
        "--- text",
        "-\u{a0}-\u{a0}-",
    ] {
        compare_body(body, true);
    }
}

#[test]
fn rank_keeps_text_around_comments() {
    for body in [
        "<!-- c -->\ntext",
        "text <!-- c -->",
        "<!-- first -->text<!-- second -->",
        "<!-- first\ncomment -->text<!-- second\ncomment -->",
        "text <!-- unclosed",
        "<!-- c -->text<!-- unclosed",
        "--> text",
    ] {
        compare_body(body, true);
    }
}

#[test]
fn rank_keeps_tables_tags_images_links_and_code() {
    for body in [
        "|---|",
        "```\n---\n```",
        "```\n<!-- c -->\n```",
        "~~~\n<!-- c -->\n~~~",
        "```\n<!-- unclosed\n```",
        "<div></div>",
        "![image](image.png)",
        "[link](https://example.com)",
        "`code`",
        "    let value = 1;",
    ] {
        compare_body(body, true);
    }
}

#[test]
fn rank_keeps_bodyless_matches_when_no_matching_body_has_text() {
    let root = tempfile::tempdir().unwrap();
    for (name, text) in [
        ("a.md", "# needle\n<!-- c -->\n"),
        ("b.md", "# needle\n---\n"),
        ("c.md", "# needle\n \t\n"),
        ("d.md", "# other\ntext\n"),
    ] {
        std::fs::write(root.path().join(name), text).unwrap();
    }
    let found = search(&root, "rank");
    assert_eq!(found.len(), 3);
    for name in ["a.md", "b.md", "c.md"] {
        assert!(found.iter().any(|row| row["path"] == name));
    }
}

#[test]
fn exact_keeps_comment_only_thematic_break_and_heading_only_matches() {
    let root = tempfile::tempdir().unwrap();
    for (name, body) in [
        ("a.md", "<!-- c -->"),
        ("b.md", "---"),
        ("c.md", ""),
        ("d.md", "text"),
    ] {
        std::fs::write(root.path().join(name), format!("# needle\n{body}\n")).unwrap();
    }
    let found = search(&root, "exact");
    assert_eq!(found.len(), 4);
    for (row, name) in found.iter().zip(["a.md", "b.md", "c.md", "d.md"]) {
        assert_eq!(row["path"], name);
        assert_eq!(row["match"], "phrase");
        assert_eq!(row["score"], 1);
    }
}

#[test]
fn rank_excludes_a_comment_only_level_zero_section() {
    let root = tempfile::tempdir().unwrap();
    let text = "<!-- needle -->\n# needle\ntext\n";
    std::fs::write(root.path().join("a.md"), text).unwrap();
    let sections = split("a.md", text);
    assert_eq!(sections.len(), 2);
    assert_eq!(sections[0].level, 0);
    let ranked = search(&root, "rank");
    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0]["lines"], serde_json::json!([2, 3]));
    assert_eq!(search(&root, "exact").len(), 2);
}

#[test]
fn retained_scores_use_all_sections_for_df_n_and_average_length() {
    let root = tempfile::tempdir().unwrap();
    let mut sections = Vec::new();
    for (name, text) in [
        ("a.md", "# needle\n<!-- c -->\n"),
        ("b.md", "# needle\n---\n"),
        ("c.md", "# needle\ntext text text text text\n"),
        ("d.md", "# haystack\nunmatched text\n"),
    ] {
        std::fs::write(root.path().join(name), text).unwrap();
        sections.extend(split(name, text));
    }
    let (docs, n, avgdl) = search::tokenize(sections, &query_terms("needle"));
    assert_eq!(n, 4);
    assert_eq!(avgdl, 8.0);
    assert_eq!(
        docs.iter()
            .filter(|doc| doc.frequencies.contains_key("needle"))
            .count(),
        3
    );
    let found = search(&root, "rank");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["path"], "c.md");
    // N=4, df=3, avgdl=8, tf=3, length=11.
    let expected = (10.0_f64 / 7.0).ln() * 200.0 / 123.0;
    assert!((found[0]["score"].as_f64().unwrap() - expected).abs() < 1e-12);
}
