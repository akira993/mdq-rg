use serde_json::{Value, json};
use std::fs;
use std::process::{Command, Output};
use tempfile::TempDir;
use unicode_segmentation::UnicodeSegmentation;

struct Project {
    root: TempDir,
    state: TempDir,
}

impl Project {
    fn new() -> Self {
        Self {
            root: tempfile::tempdir().unwrap(),
            state: tempfile::tempdir().unwrap(),
        }
    }

    fn write(&self, path: &str, bytes: impl AsRef<[u8]>) {
        let path = self.root.path().join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, bytes).unwrap();
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mdrg"));
        command
            .current_dir(self.root.path())
            .env("XDG_STATE_HOME", self.state.path())
            .env_remove("MDRG_NO_USAGE_LOG");
        command
    }

    fn run(&self, args: &[&str]) -> Output {
        self.command().args(args).output().unwrap()
    }

    fn search(&self, query: &str) -> Vec<Value> {
        let output = self.run(&["search", "--q", query]);
        status(&output, 0);
        rows(&output.stdout)
    }

    fn log(&self) -> Vec<Value> {
        let path = self.state.path().join("mdrg/usage.jsonl");
        if path.exists() {
            rows(&fs::read(path).unwrap())
        } else {
            Vec::new()
        }
    }
}

fn status(output: &Output, expected: i32) {
    assert_eq!(
        output.status.code(),
        Some(expected),
        "stdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn rows(bytes: &[u8]) -> Vec<Value> {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn keys_in_order(bytes: &[u8], keys: &[&str]) {
    let lines = String::from_utf8(bytes.to_vec()).unwrap();
    assert!(!lines.is_empty());
    for line in lines.lines() {
        let value: Value = serde_json::from_str(line).unwrap();
        assert_eq!(value.as_object().unwrap().len(), keys.len());
        let mut cursor = 0;
        for key in keys {
            assert!(value.get(key).is_some(), "missing {key}: {line}");
            let needle = format!("\"{key}\"");
            cursor += line[cursor..].find(&needle).unwrap() + needle.len();
        }
    }
}

fn has_path(values: &[Value], path: &str) -> bool {
    values.iter().any(|row| row["path"] == path)
}

fn get_error(project: &Project, spec: &str, code: &str) {
    let output = project.run(&["get", spec]);
    status(&output, 1);
    assert!(output.stdout.is_empty());
    let errors = rows(&output.stderr);
    assert_eq!(errors.len(), 1);
    assert_eq!(errors[0]["error"]["code"], code);
    assert!(!errors[0]["error"]["detail"].as_str().unwrap().is_empty());
}

fn no_results(project: &Project, args: &[&str], cause: &str) -> Value {
    let output = project.run(args);
    status(&output, 0);
    assert!(output.stdout.is_empty());
    let errors = rows(&output.stderr);
    assert_eq!(errors.len(), 1);
    let diagnostic = &errors[0]["no_results"];
    assert_eq!(diagnostic["cause"], cause);
    let detail = diagnostic["detail"].as_str().unwrap();
    assert!(!detail.is_empty() && detail.is_ascii());
    let next = diagnostic["next"].as_array().unwrap();
    assert!(!next.is_empty());
    for suggestion in next {
        let text = suggestion.as_str().unwrap();
        assert!(!text.is_empty() && text.is_ascii());
    }
    diagnostic.clone()
}

#[test]
fn search_json_keys_have_contract_order() {
    let p = Project::new();
    p.write("a.md", "# Topic\nneedle\n");
    let out = p.run(&["search", "--q", "needle"]);
    status(&out, 0);
    keys_in_order(
        &out.stdout,
        &["path", "lines", "heading_path", "score", "snippet"],
    );
    let values = rows(&out.stdout);
    assert_eq!(values[0]["path"], "a.md");
    assert_eq!(values[0]["lines"], json!([1, 2]));
    assert_eq!(values[0]["heading_path"], "Topic");
    assert!(values[0]["score"].is_number());
    assert!(values[0]["snippet"].as_str().unwrap().contains("needle"));
}

#[test]
fn exact_json_appends_match_after_snippet() {
    let p = Project::new();
    p.write("a.md", "# Topic\nneedle\n");
    let out = p.run(&["search", "--q", "needle", "--mode", "exact"]);
    status(&out, 0);
    keys_in_order(
        &out.stdout,
        &["path", "lines", "heading_path", "score", "snippet", "match"],
    );
    assert_eq!(rows(&out.stdout)[0]["match"], "phrase");
}

#[test]
fn get_json_keys_and_original_text_follow_contract() {
    let p = Project::new();
    p.write(
        "a.md",
        "# Parent\n## Child\nA \"quote\" and \\ slash\nlast\n",
    );
    let out = p.run(&["get", "a.md:3-4"]);
    status(&out, 0);
    keys_in_order(&out.stdout, &["path", "lines", "heading_path", "text"]);
    let values = rows(&out.stdout);
    assert_eq!(values.len(), 1);
    assert_eq!(
        values[0],
        json!({"path":"a.md", "lines":[3,4], "heading_path":"Parent > Child", "text":"A \"quote\" and \\ slash\nlast\n"})
    );
}

#[test]
fn list_json_keys_and_heading_levels_follow_contract() {
    let p = Project::new();
    p.write("a.md", "intro\n# Parent\nbody\n## Child\nlast\n");
    let out = p.run(&["list"]);
    status(&out, 0);
    keys_in_order(&out.stdout, &["path", "lines", "heading_path", "level"]);
    assert_eq!(
        rows(&out.stdout),
        json!([
            {"path":"a.md","lines":[1,1],"heading_path":"(preface)","level":0},
            {"path":"a.md","lines":[2,3],"heading_path":"Parent","level":1},
            {"path":"a.md","lines":[4,5],"heading_path":"Parent > Child","level":2}
        ])
        .as_array()
        .unwrap()
        .to_vec()
    );
}

macro_rules! list_preface {
    ($name:ident, $text:expr, $lines:expr) => {
        #[test]
        fn $name() {
            let p = Project::new();
            p.write("a.md", $text);
            let out = p.run(&["list"]);
            status(&out, 0);
            assert_eq!(rows(&out.stdout), vec![json!({"path":"a.md", "lines":$lines, "heading_path":"(preface)", "level":0})]);
        }
    };
}
list_preface!(list_includes_headingless_document, "plain\ntext\n", [1, 2]);
list_preface!(list_includes_empty_document, "", [0, 0]);
list_preface!(
    list_includes_frontmatter_only_document,
    "---\ntitle: Empty\n---\n",
    [0, 0]
);

macro_rules! usage_error {
    ($name:ident, $args:expr) => {
        #[test]
        fn $name() {
            let p = Project::new();
            status(&p.run($args), 2);
        }
    };
}
usage_error!(missing_search_query_exits_two, &["search"]);
usage_error!(
    invalid_search_mode_exits_two,
    &["search", "--q", "x", "--mode", "wrong"]
);
usage_error!(malformed_get_range_exits_two, &["get", "a.md:wrong"]);
usage_error!(reversed_get_range_exits_two, &["get", "a.md:3-1"]);
usage_error!(unknown_list_option_exits_two, &["list", "--wrong"]);

macro_rules! config_error {
    ($name:ident, $config:expr) => {
        #[test]
        fn $name() {
            let p = Project::new();
            p.write("mdrg.toml", $config);
            let out = p.run(&["search", "--q", "needle"]);
            status(&out, 2);
            assert!(!out.stderr.is_empty());
        }
    };
}
config_error!(unknown_config_key_exits_two, "unknown_setting = true\n");
config_error!(invalid_config_type_exits_two, "roots = 12\n");

macro_rules! scope_test {
    ($name:ident, $args:expr, $config:expr, $blocked:expr) => {
        #[test]
        fn $name() {
            let p = Project::new();
            p.write("inside/keep.md", "# Topic\nneedle\n");
            p.write($blocked, "# Topic\nneedle\n");
            p.write("mdrg.toml", $config);
            let out = p.run($args);
            status(&out, 0);
            let values = rows(&out.stdout);
            assert!(has_path(&values, "inside/keep.md"));
            assert!(!has_path(&values, $blocked));
        }
    };
}
scope_test!(
    search_obeys_roots,
    &["search", "--q", "needle"],
    "roots = ['inside']",
    "outside.md"
);
scope_test!(
    list_obeys_roots,
    &["list"],
    "roots = ['inside']",
    "outside.md"
);
scope_test!(
    search_obeys_exclude,
    &["search", "--q", "needle"],
    "exclude = ['blocked.md']",
    "blocked.md"
);
scope_test!(
    list_obeys_exclude,
    &["list"],
    "exclude = ['blocked.md']",
    "blocked.md"
);
scope_test!(
    search_always_excludes_git,
    &["search", "--q", "needle", "--hidden", "--no-ignore"],
    "",
    ".git/blocked.md"
);
scope_test!(
    search_always_excludes_node_modules,
    &["search", "--q", "needle", "--hidden", "--no-ignore"],
    "",
    "node_modules/blocked.md"
);
scope_test!(
    list_always_excludes_git,
    &["list", "--hidden", "--no-ignore"],
    "",
    ".git/blocked.md"
);
scope_test!(
    list_always_excludes_node_modules,
    &["list", "--hidden", "--no-ignore"],
    "",
    "node_modules/blocked.md"
);

#[test]
fn explicitly_named_hidden_root_is_searched() {
    let p = Project::new();
    p.write(".notes/a.md", "needle\n");
    p.write("mdrg.toml", "roots = ['.notes']");
    assert!(has_path(&p.search("needle"), ".notes/a.md"));
}

#[test]
fn search_result_path_and_lines_can_be_passed_to_get() {
    let p = Project::new();
    p.write("notes/a.md", "# Topic\nneedle original text\n");
    let found = p.search("needle");
    assert!(!found.is_empty());
    let row = &found[0];
    let spec = format!(
        "{}:{}-{}",
        row["path"].as_str().unwrap(),
        row["lines"][0],
        row["lines"][1]
    );
    let out = p.run(&["get", &spec]);
    status(&out, 0);
    assert!(
        rows(&out.stdout)[0]["text"]
            .as_str()
            .unwrap()
            .contains("needle original text")
    );
}

macro_rules! denied_get {
    ($name:ident, $path:expr, $config:expr, $code:expr) => {
        #[test]
        fn $name() {
            let p = Project::new();
            p.write($path, "needle\n");
            p.write("mdrg.toml", $config);
            get_error(&p, &format!("{}:1-1", $path), $code);
        }
    };
}
#[test]
fn get_rejects_document_outside_configured_roots() {
    let p = Project::new();
    p.write("outside.md", "needle\n");
    p.write("inside/keep.md", "needle\n");
    p.write("mdrg.toml", "roots = ['inside']");
    get_error(&p, "outside.md:1-1", "not_found");
}
denied_get!(
    get_rejects_excluded_document,
    "blocked.md",
    "exclude = ['blocked.md']",
    "not_found"
);
denied_get!(
    get_rejects_hidden_document_by_default,
    ".secret.md",
    "",
    "hidden"
);
denied_get!(
    get_rejects_non_markdown_document,
    "plain.txt",
    "",
    "unsupported_type"
);

#[test]
fn get_rejects_missing_document_with_exit_one() {
    let p = Project::new();
    get_error(&p, "missing.md:1-1", "not_found");
}

#[test]
fn get_rejects_parent_traversal() {
    let p = Project::new();
    let parent = p.root.path().parent().unwrap();
    let outside = tempfile::Builder::new()
        .suffix(".md")
        .tempfile_in(parent)
        .unwrap();
    fs::write(outside.path(), "outside\n").unwrap();
    let spec = format!(
        "../{}:1-1",
        outside.path().file_name().unwrap().to_str().unwrap()
    );
    get_error(&p, &spec, "outside_root");
}

#[test]
fn get_rejects_absolute_path_outside_working_directory() {
    let p = Project::new();
    let outside = tempfile::Builder::new().suffix(".md").tempfile().unwrap();
    fs::write(outside.path(), "outside\n").unwrap();
    get_error(
        &p,
        &format!("{}:1-1", outside.path().display()),
        "outside_root",
    );
}

#[cfg(unix)]
#[test]
fn get_rejects_symlink_escaping_working_directory() {
    let p = Project::new();
    let outside = tempfile::Builder::new().suffix(".md").tempfile().unwrap();
    fs::write(outside.path(), "outside\n").unwrap();
    std::os::unix::fs::symlink(outside.path(), p.root.path().join("link.md")).unwrap();
    get_error(&p, "link.md:1-1", "outside_root");
}

fn git(project: &Project, args: &[&str]) -> Output {
    let out = Command::new("git")
        .args(args)
        .current_dir(project.root.path())
        .output()
        .unwrap();
    status(&out, 0);
    out
}

#[test]
fn get_rejects_gitignored_document() {
    let p = Project::new();
    git(&p, &["init", "--quiet"]);
    p.write(".gitignore", "ignored.md\n");
    p.write("ignored.md", "needle\n");
    get_error(&p, "ignored.md:1-1", "ignored");
}

#[test]
fn get_hidden_flag_includes_hidden_document() {
    let p = Project::new();
    p.write(".hidden.md", "needle\n");
    let out = p.run(&["get", ".hidden.md:1-1", "--hidden"]);
    status(&out, 0);
    assert_eq!(rows(&out.stdout)[0]["text"], "needle\n");
}

#[test]
fn get_no_ignore_flag_includes_gitignored_document() {
    let p = Project::new();
    git(&p, &["init", "--quiet"]);
    p.write(".gitignore", "ignored.md\n");
    p.write("ignored.md", "needle\n");
    let out = p.run(&["get", "ignored.md:1-1", "--no-ignore"]);
    status(&out, 0);
    assert_eq!(rows(&out.stdout)[0]["text"], "needle\n");
}

macro_rules! range_error {
    ($name:ident, $range:expr) => {
        #[test]
        fn $name() {
            let p = Project::new();
            p.write("a.md", "one\ntwo\n");
            get_error(&p, $range, "out_of_range");
        }
    };
}
range_error!(get_rejects_start_beyond_last_line, "a.md:3-3");
#[test]
fn get_clamps_end_beyond_last_line() {
    let p = Project::new();
    p.write("a.md", "one\ntwo\n");
    let out = p.run(&["get", "a.md:1-3"]);
    status(&out, 0);
    assert_eq!(rows(&out.stdout)[0]["lines"], serde_json::json!([1, 2]));
    assert_eq!(rows(&out.stdout)[0]["text"], "one\ntwo\n");
}

#[test]
fn get_always_excludes_git_even_with_override_flags() {
    let p = Project::new();
    p.write(".git/a.md", "needle\n");
    let out = p.run(&["get", ".git/a.md:1-1", "--hidden", "--no-ignore"]);
    status(&out, 1);
    assert_eq!(rows(&out.stderr)[0]["error"]["code"], "not_found");
}

#[test]
fn get_always_excludes_node_modules_even_with_override_flags() {
    let p = Project::new();
    p.write("node_modules/a.md", "needle\n");
    let out = p.run(&["get", "node_modules/a.md:1-1", "--hidden", "--no-ignore"]);
    status(&out, 1);
    assert_eq!(rows(&out.stderr)[0]["error"]["code"], "not_found");
}

fn exact(project: &Project, query: &str) -> Vec<Value> {
    let out = project.run(&["search", "--q", query, "--mode", "exact"]);
    status(&out, 0);
    rows(&out.stdout)
}

#[test]
fn exact_ignores_latin_case_and_width() {
    let p = Project::new();
    p.write("a.md", "# Topic\nAbC\n");
    let found = exact(&p, "ＡＢＣ");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["match"], "phrase");
}

#[test]
fn exact_orders_phrase_before_words_even_with_lower_score() {
    let p = Project::new();
    p.write("z.md", "# Topic\nred blue\n");
    p.write(
        "a.md",
        "# Topic\nblue and red and blue and red and blue and red\n",
    );
    let found = exact(&p, "red blue");
    // Keep the words-only fixture free of the phrase, including across word boundaries.
    assert_eq!(found.len(), 2);
    assert_eq!(found[0]["path"], "z.md");
    assert_eq!(found[0]["match"], "phrase");
    assert_eq!(found[1]["match"], "words");
}

#[test]
fn exact_requires_all_words_in_the_same_section() {
    let p = Project::new();
    p.write("a.md", "# First\nred\n# Second\nblue\n");
    assert!(exact(&p, "red blue").is_empty());
}

#[test]
fn exact_does_not_match_path_only() {
    let p = Project::new();
    p.write("needle.md", "# Topic\nother content\n");
    assert!(exact(&p, "needle").is_empty());
}

#[test]
fn exact_no_match_reports_section_counts_for_each_word() {
    let p = Project::new();
    p.write("a.md", "# First\nred red\n# Second\nred\n# Third\nblue\n");
    let diagnostic = no_results(
        &p,
        &["search", "--q", "red blue", "--mode", "exact"],
        "no_match",
    );
    assert_eq!(diagnostic["words"], json!({"red":2,"blue":1}));
}

macro_rules! language_hit {
    ($name:ident, $body:expr, $query:expr) => {
        #[test]
        fn $name() {
            let p = Project::new();
            p.write("a.md", $body);
            assert!(has_path(&p.search($query), "a.md"));
        }
    };
}
language_hit!(
    variant_tall_takahashi_matches_standard_spelling,
    "高橋さんの案内です。\n",
    "髙橋"
);
language_hit!(
    variant_watanabe_matches_standard_spelling,
    "渡辺さんの案内です。\n",
    "渡邊"
);
language_hit!(fullwidth_latin_query_matches_ascii, "abc\n", "ＡＢＣ");
language_hit!(
    halfwidth_kana_query_matches_fullwidth,
    "これはガイドです。\n",
    "ｶﾞｲﾄﾞ"
);
language_hit!(
    nfd_kana_query_matches_nfc_document,
    "ガイドを読みます。\n",
    "カ\u{3099}イド"
);
language_hit!(
    nfc_kana_query_matches_nfd_document,
    "カ\u{3099}イドを読みます。\n",
    "ガイド"
);
language_hit!(
    chinese_document_matches_two_character_term,
    "中華人民共和国\n",
    "人民"
);
language_hit!(
    korean_document_matches_two_character_term,
    "대한민국서울\n",
    "민국"
);
language_hit!(
    prototype_single_character_matches_contiguous_bigram,
    "中文字符\n",
    "文"
);
language_hit!(
    prototype_heading_only_query_is_not_prefiltered,
    "# needle\nother content\n",
    "needle"
);
language_hit!(
    prototype_iteration_mark_stays_in_japanese_word,
    "人々が集まります。\n",
    "人々"
);

#[test]
fn iteration_mark_query_does_not_degenerate_to_shared_suffix() {
    let p = Project::new();
    p.write("a.md", "人々が集まります。\n");
    p.write("b.md", "時々ここへ来ます。\n");
    let found = p.search("人々");
    assert!(has_path(&found, "a.md"));
    assert!(!has_path(&found, "b.md"));
}

#[test]
fn prototype_people_iteration_word_does_not_match_singular_person() {
    let p = Project::new();
    p.write("a.md", "人々が集まります。\n");
    p.write("b.md", "人が集まります。\n");
    let found = p.search("人々");
    assert!(has_path(&found, "a.md"));
    assert!(!has_path(&found, "b.md"));
}

#[test]
fn prototype_sometimes_iteration_word_does_not_match_time_alone() {
    let p = Project::new();
    p.write("a.md", "時々ここへ来ます。\n");
    p.write("b.md", "時を知らせます。\n");
    let found = p.search("時々");
    assert!(has_path(&found, "a.md"));
    assert!(!has_path(&found, "b.md"));
}

#[test]
fn kanji_only_query_matches_both_japanese_and_chinese_documents() {
    let p = Project::new();
    p.write("ja.md", "東京へ行きます。\n");
    p.write("zh.md", "東京城市\n");
    let found = p.search("東京");
    assert!(has_path(&found, "ja.md"));
    assert!(has_path(&found, "zh.md"));
}

fn snippet_boundaries(cluster: &str) {
    let p = Project::new();
    // Move a cluster across both sides of the 400-character clipping boundary.
    for offset in 370..410 {
        let body = format!(
            "needle {}{}{}\n",
            "x".repeat(offset),
            cluster.repeat(8),
            "z".repeat(450)
        );
        p.write("a.md", &body);
        let found = p.search("needle");
        assert_eq!(found.len(), 1);
        let snippet = found[0]["snippet"].as_str().unwrap();
        assert!(!snippet.is_empty());
        assert!(snippet.chars().count() <= 400);
        assert!(snippet.lines().count() <= 5);
        let allowed: Vec<&str> = body.graphemes(true).collect();
        for grapheme in snippet.graphemes(true) {
            assert!(
                allowed.contains(&grapheme) || matches!(grapheme, "…" | "."),
                "broken grapheme {grapheme:?} at offset {offset}: {snippet:?}"
            );
        }
    }
}

#[test]
fn snippet_does_not_split_family_emoji_at_long_line_boundary() {
    snippet_boundaries("👨‍👩‍👧");
}

#[test]
fn snippet_does_not_split_combining_character_at_long_line_boundary() {
    // q + acute has no precomposed NFC equivalent.
    snippet_boundaries("q\u{301}");
}

#[test]
fn search_appends_one_usage_log_line_per_invocation() {
    let p = Project::new();
    p.write("a.md", "needle\n");
    assert!(p.log().is_empty());
    p.search("needle");
    assert_eq!(p.log().len(), 1);
    p.search("needle");
    let log = p.log();
    assert_eq!(log.len(), 2);
    let entry = &log[1];
    for key in [
        "ts",
        "version",
        "command",
        "cwd",
        "args",
        "result",
        "elapsed_ms",
        "exit",
    ] {
        assert!(entry.get(key).is_some(), "missing log key {key}");
    }
    assert_eq!(entry["command"], "search");
    assert_eq!(entry["exit"], 0);
}

#[test]
fn usage_log_can_be_disabled_by_configuration() {
    let p = Project::new();
    p.write("a.md", "needle\n");
    p.search("needle");
    assert_eq!(p.log().len(), 1);
    p.write("mdrg.toml", "usage_log = false\n");
    p.search("needle");
    assert_eq!(p.log().len(), 1);
}

#[test]
fn usage_log_can_be_disabled_by_environment() {
    let p = Project::new();
    p.write("a.md", "needle\n");
    p.search("needle");
    assert_eq!(p.log().len(), 1);
    let out = p
        .command()
        .env("MDRG_NO_USAGE_LOG", "1")
        .args(["search", "--q", "needle"])
        .output()
        .unwrap();
    status(&out, 0);
    assert_eq!(p.log().len(), 1);
}

#[test]
fn usage_log_does_not_contain_document_body() {
    let p = Project::new();
    p.write("a.md", "needle PRIVATE_BODY_SENTINEL_792584\n");
    p.search("needle");
    let log = p.log();
    assert_eq!(log.len(), 1);
    assert!(
        !serde_json::to_string(&log)
            .unwrap()
            .contains("PRIVATE_BODY_SENTINEL_792584")
    );
}

#[test]
fn search_and_usage_logging_leave_git_status_unchanged() {
    let p = Project::new();
    git(&p, &["init", "--quiet"]);
    p.write("a.md", "needle\n");
    let before = git(&p, &["status", "--porcelain"]).stdout;
    p.search("needle");
    assert_eq!(git(&p, &["status", "--porcelain"]).stdout, before);
}

macro_rules! dated_frontmatter {
    ($name:ident, $date:expr) => {
        #[test]
        fn $name() {
            let p = Project::new();
            p.write(
                "a.md",
                format!("---\ndate: {}\n---\n# Topic\nneedle\n", $date),
            );
            assert!(has_path(&p.search("needle"), "a.md"));
        }
    };
}
dated_frontmatter!(frontmatter_date_does_not_stop_search, "2026-11-02");
dated_frontmatter!(
    frontmatter_timestamp_does_not_stop_search,
    "2026-11-02T10:00:00+09:00"
);

#[test]
fn search_does_not_include_gitignored_documents() {
    let p = Project::new();
    git(&p, &["init", "--quiet"]);
    p.write(".gitignore", "ignored.md\n");
    p.write("ignored.md", "needle\n");
    p.write("kept.md", "needle\n");
    let found = p.search("needle");
    assert!(has_path(&found, "kept.md"));
    assert!(!has_path(&found, "ignored.md"));
}

#[test]
fn removed_document_is_absent_from_next_search() {
    let p = Project::new();
    let path = p.root.path().join("removed.md");
    let removed = tempfile::NamedTempFile::new_in(p.root.path()).unwrap();
    let mut removed = removed.persist(&path).unwrap();
    use std::io::Write;
    writeln!(removed, "needle").unwrap();
    assert!(has_path(&p.search("needle"), "removed.md"));
    // Reclaim the file with an RAII owner; dropping it models an external deletion.
    let removed = tempfile::TempPath::try_from_path(&path).unwrap();
    drop(removed);
    assert!(!has_path(&p.search("needle"), "removed.md"));
}

macro_rules! skipped_document {
    ($name:ident, $bytes:expr, $reason:expr) => {
        #[test]
        fn $name() {
            let p = Project::new();
            p.write("broken.md", $bytes);
            p.write("good.md", "needle\n");
            let out = p.run(&["search", "--q", "needle"]);
            status(&out, 0);
            let found = rows(&out.stdout);
            assert!(has_path(&found, "good.md"));
            assert!(!has_path(&found, "broken.md"));
            assert_eq!(rows(&out.stderr), vec![json!({"skipped":{"path":"broken.md", "reason":$reason}})]);
        }
    };
}
skipped_document!(
    invalid_utf8_document_is_named_and_skipped_without_failure,
    b"needle\xff\n",
    "invalid_utf8"
);
skipped_document!(
    nul_document_is_named_and_skipped_without_failure,
    b"needle\0text\n",
    "contains_nul"
);

#[test]
fn no_documents_reports_cause_and_next_action_with_empty_stdout() {
    let p = Project::new();
    no_results(&p, &["search", "--q", "needle"], "no_documents");
}

#[test]
fn paths_matching_nothing_reports_cause_and_next_action_with_empty_stdout() {
    let p = Project::new();
    p.write("a.md", "needle\n");
    no_results(
        &p,
        &["search", "--q", "needle", "--paths", "missing/*"],
        "paths_matched_nothing",
    );
}

#[test]
fn empty_query_reports_cause_and_next_action_with_empty_stdout() {
    let p = Project::new();
    p.write("a.md", "needle\n");
    no_results(&p, &["search", "--q", "   "], "empty_query");
}

#[test]
fn no_match_reports_cause_and_next_action_with_empty_stdout() {
    let p = Project::new();
    p.write("a.md", "needle\n");
    no_results(&p, &["search", "--q", "absentword"], "no_match");
}

#[test]
fn prototype_double_inline_fence_does_not_hide_following_heading() {
    let p = Project::new();
    p.write("a.md", "``` code ```\n# Target\nneedle\n");
    let found = p.search("needle");
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["heading_path"], "Target");
    assert_eq!(found[0]["lines"], json!([2, 3]));
}

#[test]
fn prototype_md_query_does_not_match_file_extension() {
    let p = Project::new();
    p.write("a.md", "# Topic\nother content\n");
    assert!(p.search("md").is_empty());
}

#[test]
fn prototype_bom_frontmatter_is_not_searchable_body() {
    let p = Project::new();
    p.write(
        "a.md",
        "\u{feff}---\ntitle: metadataunique\n---\n# Topic\nneedle\n",
    );
    assert!(has_path(&p.search("needle"), "a.md"));
    assert!(p.search("metadataunique").is_empty());
}

#[test]
fn prototype_temporary_project_and_log_directories_are_removed_after_test_scope() {
    let (root, state) = {
        let p = Project::new();
        p.write("a.md", "needle\n");
        p.search("needle");
        (p.root.path().to_path_buf(), p.state.path().to_path_buf())
    };
    assert!(!root.exists());
    assert!(!state.exists());
}

#[test]
fn two_hundred_deterministic_random_markdown_files_do_not_crash_search() {
    let p = Project::new();
    let mut seed = 0x4d44_5247_u64;
    for index in 0..200 {
        let bytes: Vec<u8> = (0..1024)
            .map(|_| {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                seed as u8
            })
            .collect();
        p.write(&format!("random-{index}.md"), bytes);
    }
    p.write("good.md", "needle\n");
    let out = p.run(&["search", "--q", "needle"]);
    status(&out, 0);
    assert!(has_path(&rows(&out.stdout), "good.md"));
}

#[test]
fn one_megabyte_single_japanese_line_does_not_crash_search() {
    let p = Project::new();
    let mut body = "東京へ行きます。".repeat(1_048_576 / "東京へ行きます。".len());
    body.push_str(&"a".repeat(1_048_576 - body.len()));
    assert_eq!(body.len(), 1_048_576);
    p.write("a.md", body);
    assert!(has_path(&p.search("東京"), "a.md"));
}

#[test]
fn empty_document_does_not_crash_search() {
    let p = Project::new();
    p.write("a.md", "");
    status(&p.run(&["search", "--q", "needle"]), 0);
}

#[test]
fn bare_heading_marker_does_not_crash_search() {
    let p = Project::new();
    p.write("a.md", "#\n");
    status(&p.run(&["search", "--q", "needle"]), 0);
}

#[test]
fn version_command_succeeds_and_names_binary() {
    let p = Project::new();
    let out = p.run(&["--version"]);
    status(&out, 0);
    assert!(String::from_utf8(out.stdout).unwrap().starts_with("mdrg "));
}

#[test]
fn list_heading_level_filters_deeper_sections() {
    let p = Project::new();
    p.write("a.md", "# Parent\nbody\n## Child\nbody\n");
    let out = p.run(&["list", "--heading-level", "1"]);
    status(&out, 0);
    let values = rows(&out.stdout);
    assert!(!values.is_empty());
    assert!(values.iter().all(|row| row["level"].as_u64().unwrap() <= 1));
}

#[test]
fn list_limit_bounds_output_count() {
    let p = Project::new();
    p.write("a.md", "# First\nbody\n# Second\nbody\n");
    let out = p.run(&["list", "--limit", "1"]);
    status(&out, 0);
    assert_eq!(rows(&out.stdout).len(), 1);
}

#[test]
fn list_paths_filter_excludes_other_documents() {
    let p = Project::new();
    p.write("a.md", "needle\n");
    p.write("b.md", "needle\n");
    let out = p.run(&["list", "--paths", "a.md"]);
    status(&out, 0);
    let values = rows(&out.stdout);
    assert!(has_path(&values, "a.md"));
    assert!(!has_path(&values, "b.md"));
}

#[test]
fn search_timing_starts_with_document_byte_and_section_counts() {
    let p = Project::new();
    p.write("a.md", "# Topic\nneedle\n");
    let out = p.run(&["search", "--q", "needle", "--timing"]);
    status(&out, 0);
    let stderr = String::from_utf8(out.stderr).unwrap();
    let fields: Vec<&str> = stderr.split_whitespace().collect();
    assert!(fields[0].starts_with("files="));
    assert!(fields[1].starts_with("bytes="));
    assert!(fields[2].starts_with("sections="));
    assert_eq!(stderr.lines().count(), 1);
}

#[test]
fn get_appends_one_usage_log_line() {
    let p = Project::new();
    p.write("a.md", "needle\n");
    status(&p.run(&["get", "a.md:1-1"]), 0);
    assert_eq!(p.log().len(), 1);
    assert_eq!(p.log()[0]["command"], "get");
}

#[test]
fn list_appends_one_usage_log_line() {
    let p = Project::new();
    p.write("a.md", "needle\n");
    status(&p.run(&["list"]), 0);
    assert_eq!(p.log().len(), 1);
    assert_eq!(p.log()[0]["command"], "list");
}

#[cfg(unix)]
#[test]
fn usage_log_has_private_file_and_directory_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let p = Project::new();
    p.write("a.md", "needle\n");
    p.search("needle");
    let directory = p.state.path().join("mdrg");
    assert_eq!(
        fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(directory.join("usage.jsonl"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}

#[test]
fn usage_log_write_failure_warns_without_stopping_search() {
    let p = Project::new();
    p.write("a.md", "needle\n");
    fs::write(p.state.path().join("mdrg"), "blocks directory creation").unwrap();
    let out = p.run(&["search", "--q", "needle"]);
    status(&out, 0);
    assert!(has_path(&rows(&out.stdout), "a.md"));
    let diagnostics = rows(&out.stderr);
    assert_eq!(diagnostics.len(), 1);
    assert!(diagnostics[0].get("warning").is_some());
}

#[test]
fn hidden_configuration_and_no_ignore_argument_expand_documents_but_not_excluded_directories() {
    let p = Project::new();
    git(&p, &["init", "--quiet"]);
    p.write(".gitignore", "ignored.md\n");
    for path in [
        "kept.md",
        ".hidden.md",
        "ignored.md",
        ".git/private.md",
        "node_modules/private.md",
    ] {
        p.write(path, "needle\n");
    }
    let baseline = p.search("needle");
    assert!(has_path(&baseline, "kept.md"));
    assert!(!has_path(&baseline, ".hidden.md"));
    assert!(!has_path(&baseline, "ignored.md"));
    p.write("mdrg.toml", "hidden = true\n");
    let hidden = p.search("needle");
    assert!(has_path(&hidden, ".hidden.md"));
    assert!(!has_path(&hidden, "ignored.md"));
    p.write("mdrg.toml", "");
    let out = p.run(&["search", "--q", "needle", "--no-ignore"]);
    status(&out, 0);
    let ignored = rows(&out.stdout);
    assert!(has_path(&ignored, "ignored.md"));
    assert!(!has_path(&ignored, ".hidden.md"));
    p.write("mdrg.toml", "hidden = true\n");
    let out = p.run(&["search", "--q", "needle", "--no-ignore"]);
    status(&out, 0);
    let expanded = rows(&out.stdout);
    for path in ["kept.md", ".hidden.md", "ignored.md"] {
        assert!(has_path(&expanded, path));
    }
    for found in [&baseline, &hidden, &ignored, &expanded] {
        assert!(!has_path(found, ".git/private.md"));
        assert!(!has_path(found, "node_modules/private.md"));
    }
}

#[test]
fn exact_excerpt_locates_variant_without_folding_it() {
    let p = Project::new();
    p.write("a.md", "# Topic\n0\n1\n2\n3\n4\n5\n6\n髙橋\n8\n9\n");
    let out = p.run(&["search", "--mode", "exact", "--q", "髙橋"]);
    status(&out, 0);
    assert!(
        rows(&out.stdout)[0]["snippet"]
            .as_str()
            .unwrap()
            .contains("髙橋")
    );
}

fn flag_expands_all_commands(path: &str, config: &str, flags: &[&str]) {
    let p = Project::new();
    p.write("kept.md", "needle\n");
    p.write(path, "needle\n");
    p.write(".ignore", "ignored.md\n");
    let baseline = p.run(&["list"]);
    status(&baseline, 0);
    assert_eq!(rows(&baseline.stdout).len(), 1);
    assert!(!has_path(&rows(&baseline.stdout), path));
    assert!(!has_path(&p.search("needle"), path));
    get_error(
        &p,
        &format!("{path}:1-1"),
        if path == "ignored.md" {
            "ignored"
        } else {
            "hidden"
        },
    );

    p.write("mdrg.toml", config);
    let mut args = vec!["list"];
    args.extend_from_slice(flags);
    let expanded = p.run(&args);
    status(&expanded, 0);
    let listed = rows(&expanded.stdout);
    assert_eq!(listed.len(), 2);
    assert!(has_path(&listed, path));
    for mode in ["rank", "exact"] {
        let mut args = vec!["search", "--q", "needle", "--mode", mode];
        args.extend_from_slice(flags);
        let expanded = p.run(&args);
        status(&expanded, 0);
        let found = rows(&expanded.stdout);
        assert_eq!(found.len(), 2);
        assert!(has_path(&found, path));
    }
    let spec = format!("{path}:1-1");
    let mut args = vec!["get", &spec];
    args.extend_from_slice(flags);
    let expanded = p.run(&args);
    status(&expanded, 0);
    let found = rows(&expanded.stdout);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["path"], path);
    assert_eq!(found[0]["text"], "needle\n");
}

#[test]
fn configuration_hidden_expands_list_search_and_get() {
    flag_expands_all_commands(".hidden.md", "hidden = true\n", &[]);
}

#[test]
fn no_ignore_argument_expands_list_search_and_get() {
    flag_expands_all_commands("ignored.md", "", &["--no-ignore"]);
}

#[test]
fn list_preserves_whitespace_only_documents_without_searchable_sections() {
    let p = Project::new();
    p.write("a.md", " \t\n\n");
    p.write("b.md", "---\ntitle: example\n---\n \n");
    let out = p.run(&["list"]);
    status(&out, 0);
    let listed = rows(&out.stdout);
    assert_eq!(listed.len(), 2);
    for row in listed {
        assert_eq!(row["lines"], json!([0, 0]));
        assert_eq!(row["level"], 0);
        assert_eq!(row["heading_path"], "(preface)");
    }
    assert!(p.search("preface").is_empty());
}
