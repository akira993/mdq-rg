mod common;

use common::TempDir;
use std::process::Command;

#[test]
fn search_defaults_to_1300_tokens_and_accepts_an_override() {
    use clap::Parser;
    use mdq_rg::cli::{Cli, Command as ParsedCommand};

    for (args, expected) in [
        (vec!["mdrg", "search", "--q", "needle"], 1300),
        (
            vec!["mdrg", "search", "--q", "needle", "--max-tokens", "800"],
            800,
        ),
    ] {
        let cli = Cli::try_parse_from(args).unwrap();
        let ParsedCommand::Search(search) = cli.command else {
            panic!("expected search arguments");
        };
        assert_eq!(search.max_tokens, expected);
    }
}

fn fixture() -> TempDir {
    let root = TempDir::new();
    std::fs::create_dir_all(root.join(".claude/rules")).unwrap();
    std::fs::write(root.join("good.md"), "# Good\nneedle\n").unwrap();
    std::fs::write(root.join("bad.md"), [0xff, 0xfe]).unwrap();
    std::fs::write(root.join("ignored.md"), "needle").unwrap();
    std::fs::write(root.join(".gitignore"), "ignored.md\n").unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(&*root)
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(root.join(".claude/rules/hidden.md"), "needle").unwrap();
    root
}

fn command(root: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_mdrg"))
        .env("MDRG_NO_USAGE_LOG", "1")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn invalid_utf8_is_skipped_ignore_and_hidden_are_respected() {
    let root = fixture();
    let output = command(&root, &["search", "--q", "needle"]);
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).lines().count(), 1);
    assert_eq!(
        String::from_utf8_lossy(&output.stderr).trim(),
        "{\"skipped\":{\"path\":\"bad.md\",\"reason\":\"invalid_utf8\"}}"
    );
    let output = command(
        &root,
        &[
            "search",
            "--q",
            "needle",
            "--hidden",
            "--paths",
            ".claude/*.md",
            "good.md",
            "--mode",
            "rank",
            "--timing",
        ],
    );
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).lines().count(), 2);
    assert!(String::from_utf8_lossy(&output.stderr).contains("tokenize_ms="));
    let output = command(&root, &["search", "--q", "absent"]);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn paths_exclude_matching_documents_outside_the_pattern() {
    let root = fixture();
    std::fs::write(root.join("other.md"), "# Other\nneedle\n").unwrap();
    let output = command(&root, &["search", "--q", "needle"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.lines().count(), 2);
    assert!(stdout.contains("good.md"));
    assert!(stdout.contains("other.md"));

    let output = command(&root, &["search", "--q", "needle", "--paths", "good.md"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_eq!(stdout.lines().count(), 1);
    assert!(stdout.contains("good.md"));
    assert!(!stdout.contains("other.md"));
}

#[test]
fn modes_validate_values_and_support_exact() {
    let root = fixture();
    let rank = command(
        &root,
        &[
            "search", "--q", "needle", "--paths", "good.md", "--mode", "rank",
        ],
    );
    assert!(rank.status.success());
    assert!(String::from_utf8_lossy(&rank.stdout).contains("good.md"));
    assert!(rank.stderr.is_empty());
    let exact = command(&root, &["search", "--q", "needle", "--mode", "exact"]);
    assert_eq!(exact.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&exact.stdout).contains("\"match\":\"phrase\""));
    assert_eq!(
        exact.stderr,
        b"{\"skipped\":{\"path\":\"bad.md\",\"reason\":\"invalid_utf8\"}}\n"
    );
    for mode in ["anything", "bm25", "grep"] {
        let invalid = command(&root, &["search", "--q", "needle", "--mode", mode]);
        assert_eq!(invalid.status.code(), Some(2));
        assert!(invalid.stdout.is_empty());
        let stderr = String::from_utf8_lossy(&invalid.stderr);
        assert!(stderr.contains("invalid value"));
        assert!(stderr.contains("rank, exact"));
    }
}

#[test]
fn version_and_bad_arguments() {
    let root = fixture();
    let output = command(&root, &["--version"]);
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "mdrg 0.1.0");
    for args in [
        vec!["search", "--q", "x", "--unknown"],
        vec!["search", "--q", "x", "--scan", "oops"],
        vec!["search", "--q", "x", "--tokenizer", "oops"],
        vec!["search", "--q", "x", "--features", "oops"],
        vec!["search"],
        vec!["search", "--q", "x", "--top-k", "bad"],
    ] {
        assert_eq!(command(&root, &args).status.code(), Some(2));
    }
}

#[test]
fn no_ignore_reads_files_ignored_by_parent() {
    let parent = fixture();
    std::fs::write(parent.join(".gitignore"), "tasks/\n").unwrap();
    let root = parent.join("corpus");
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    std::fs::write(root.join("visible.md"), "needle\n").unwrap();
    std::fs::write(root.join("tasks/ignored.md"), "needle\n").unwrap();
    let output = command(&root, &["search", "--q", "needle", "--timing"]);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("visible.md"));
    assert!(!stdout.contains("ignored.md"));
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("files=1 bytes=7 sections=1 "));

    let output = command(
        &root,
        &["search", "--q", "needle", "--no-ignore", "--timing"],
    );
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("visible.md"));
    assert!(stdout.contains("tasks/ignored.md"));
    assert!(String::from_utf8_lossy(&output.stderr).starts_with("files=2 bytes=14 sections=2 "));
}

#[test]
fn timing_counts_successful_reads_bytes_sections_and_skipped_files() {
    let root = fixture();
    let text = "# Good\nneedle 日本\n## Other\nother\n";
    std::fs::write(root.join("good.md"), text).unwrap();
    {
        let output = command(&root, &["search", "--q", "absent", "--timing"]);
        assert!(output.status.success());
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("{\"skipped\":{\"path\":\"bad.md\",\"reason\":\"invalid_utf8\"}}\n")
        );
        let timing = stderr
            .lines()
            .find(|line| line.starts_with("files="))
            .unwrap();
        assert!(timing.starts_with(&format!(
            "files=1 bytes={} sections=2 skipped=1 scan_ms=",
            text.len()
        )));
    }
}

#[test]
fn version() {
    let output = Command::new(env!("CARGO_BIN_EXE_mdrg"))
        .env("MDRG_NO_USAGE_LOG", "1")
        .arg("--version")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        format!("mdrg {}\n", env!("CARGO_PKG_VERSION"))
    );
    assert!(output.stderr.is_empty());
}

#[test]
fn help_and_unsupported_arguments() {
    let root = fixture();
    for args in [
        vec!["--help"],
        vec!["search", "--help"],
        vec!["get", "--help"],
        vec!["list", "--help"],
    ] {
        let output = command(&root, &args);
        assert!(output.status.success());
        assert!(String::from_utf8_lossy(&output.stdout).contains("Usage:"));
        assert!(output.stderr.is_empty());
    }
    for args in [vec![], vec!["search", "--unknown"]] {
        let output = command(&root, &args);
        assert_eq!(output.status.code(), Some(2));
    }
}

#[test]
fn get_and_list_return_document_content_and_sections() {
    let root = fixture();
    for args in [
        vec!["get", "good.md:1-2", "--hidden", "--no-ignore"],
        vec![
            "list",
            "--paths",
            "*.md",
            "--heading-level",
            "2",
            "--limit",
            "5",
            "--hidden",
            "--no-ignore",
        ],
    ] {
        let output = command(&root, &args);
        assert_eq!(output.status.code(), Some(0));
        let rows: Vec<serde_json::Value> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        if args[0] == "get" {
            assert_eq!(
                rows,
                [
                    serde_json::json!({"path":"good.md","lines":[1,2],"heading_path":"Good","text":"# Good\nneedle\n"})
                ]
            );
            assert!(output.stderr.is_empty());
        } else {
            assert_eq!(rows.len(), 3);
            assert_eq!(
                rows[1],
                serde_json::json!({"path":"good.md","lines":[1,2],"heading_path":"Good","level":1})
            );
            assert_eq!(
                output.stderr,
                b"{\"skipped\":{\"path\":\"bad.md\",\"reason\":\"invalid_utf8\"}}\n"
            );
        }
    }
}
