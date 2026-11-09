use serde_json::Value;
use std::process::{Command, Output};

fn run(root: &tempfile::TempDir, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mdrg"))
        .current_dir(root.path())
        .env("XDG_STATE_HOME", root.path().join("state"))
        .env("MDRG_NO_USAGE_LOG", "1")
        .args(args)
        .output()
        .unwrap()
}

fn hint(output: &Output) -> Value {
    assert!(output.status.success(), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    serde_json::from_slice::<Value>(&output.stderr).unwrap()["no_results"].clone()
}

fn replay(root: &tempfile::TempDir, command: &str) -> Output {
    Command::new("sh")
        .current_dir(root.path())
        .env("XDG_STATE_HOME", root.path().join("state"))
        .env("MDRG_NO_USAGE_LOG", "1")
        .arg("-c")
        .arg(format!(
            "mdrg() {{ \"$MDRG_TEST_BIN\" \"$@\"; }}; {command}"
        ))
        .env("MDRG_TEST_BIN", env!("CARGO_BIN_EXE_mdrg"))
        .output()
        .unwrap()
}

#[test]
fn search_hints_preserve_scope_and_quote_globs_when_replayed() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".private")).unwrap();
    std::fs::write(root.path().join(".ignore"), ".private/\n").unwrap();
    let selected = ".private/reader's guide.md";
    std::fs::write(root.path().join(selected), "# alpha\n").unwrap();
    std::fs::write(root.path().join("outside.md"), "# beta\n").unwrap();
    let glob = ".private/reader's *.md";
    for (mode, query) in [("exact", "alpha beta"), ("rank", "beta"), ("exact", "  ")] {
        let diagnostic = hint(&run(
            &root,
            &[
                "search",
                "--q",
                query,
                "--mode",
                mode,
                "--paths",
                glob,
                "absent*.md",
                "--hidden",
                "--no-ignore",
            ],
        ));
        let command = diagnostic["next"]
            .as_array()
            .unwrap()
            .last()
            .unwrap()
            .as_str()
            .unwrap();
        assert!(
            command.contains("--paths '.private/reader'\\''s *.md' 'absent*.md'"),
            "{command}"
        );
        assert!(command.contains("--hidden"), "{command}");
        assert!(command.contains("--no-ignore"), "{command}");
        let retried = replay(&root, command);
        assert!(retried.status.success(), "{retried:?}");
        if query.trim().is_empty() {
            assert_eq!(hint(&retried)["cause"], "no_match");
        } else if mode == "exact" {
            let rows: Vec<Value> = String::from_utf8(retried.stdout)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0]["path"], selected);
        } else {
            assert_eq!(hint(&retried)["cause"], "no_match");
        }
    }
}

#[test]
fn paths_failure_drops_paths_but_preserves_visibility_for_search_and_list() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join(".ignore"), "*.md\n").unwrap();
    std::fs::write(root.path().join(".hidden.md"), "# needle\n").unwrap();
    for args in [
        vec!["search", "--q", "needle", "--mode", "exact"],
        vec!["list"],
    ] {
        let mut args = args;
        args.extend(["--paths", "missing/*", "--hidden", "--no-ignore"]);
        let diagnostic = hint(&run(&root, &args));
        assert_eq!(diagnostic["cause"], "paths_matched_nothing");
        let command = diagnostic["next"][1].as_str().unwrap();
        assert!(!command.contains("--paths"), "{command}");
        assert!(command.contains("--hidden --no-ignore"), "{command}");
        let retried = replay(&root, command);
        assert!(retried.status.success(), "{retried:?}");
        assert!(!retried.stdout.is_empty(), "{retried:?}");
    }
}

#[test]
fn list_no_match_keeps_paths_and_no_documents_suggests_hidden() {
    let root = tempfile::tempdir().unwrap();
    for args in [vec!["list"], vec!["search", "--q", "needle"]] {
        let mut args = args;
        args.extend(["--paths", "missing/*"]);
        let diagnostic = hint(&run(&root, &args));
        assert_eq!(diagnostic["cause"], "no_documents");
        let command = diagnostic["next"][1].as_str().unwrap();
        assert!(command.ends_with("--hidden"));
        assert!(!command.contains("--paths"));
    }
    std::fs::write(root.path().join("selected.md"), "## deep\n").unwrap();
    std::fs::write(root.path().join("outside.md"), "# outside\n").unwrap();
    let diagnostic = hint(&run(
        &root,
        &["list", "--heading-level", "1", "--paths", "selected.md"],
    ));
    let command = diagnostic["next"][1].as_str().unwrap();
    assert_eq!(command, "mdrg list --paths 'selected.md'");
    let retried = replay(&root, command);
    assert!(retried.status.success(), "{retried:?}");
    let row: Value = serde_json::from_slice(&retried.stdout).unwrap();
    assert_eq!(row["path"], "selected.md");
}

#[test]
fn no_documents_hints_never_suggest_no_ignore() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join(".ignore"), "*.md\n").unwrap();
    std::fs::write(root.path().join("ignored.md"), "# needle\n").unwrap();
    for args in [
        vec!["search", "--q", "needle"],
        vec!["search", "--q", "needle", "--mode", "exact"],
        vec!["list"],
    ] {
        let diagnostic = hint(&run(&root, &args));
        assert_eq!(diagnostic["cause"], "no_documents");
        let next = diagnostic["next"].as_array().unwrap();
        assert!(
            next.iter()
                .all(|step| !step.as_str().unwrap().contains("--no-ignore"))
        );
        assert!(
            next.iter()
                .any(|step| step.as_str().unwrap().contains("--hidden"))
        );
        assert!(
            next.iter()
                .any(|step| step.as_str().unwrap().contains("ignore rules"))
        );
        let retried = replay(&root, next[1].as_str().unwrap());
        assert_eq!(hint(&retried)["cause"], "no_documents");
    }
}

fn assert_no_documents_preserves_explicit_no_ignore(args: &[&str]) {
    for no_ignore in [false, true] {
        for hidden in [false, true] {
            for unreadable_document in [false, true] {
                let root = tempfile::tempdir().unwrap();
                if unreadable_document {
                    std::fs::write(root.path().join("invalid.md"), b"\xff").unwrap();
                }
                let mut args = args.to_vec();
                if !unreadable_document {
                    args.extend(["--paths", "missing/*"]);
                }
                if no_ignore {
                    args.push("--no-ignore");
                }
                if hidden {
                    args.push("--hidden");
                }
                let output = run(&root, &args);
                assert!(output.status.success(), "{output:?}");
                assert!(output.stdout.is_empty(), "{output:?}");
                let diagnostic: Value = String::from_utf8(output.stderr)
                    .unwrap()
                    .lines()
                    .map(|line| serde_json::from_str::<Value>(line).unwrap())
                    .find_map(|row| row.get("no_results").cloned())
                    .unwrap();
                assert_eq!(diagnostic["cause"], "no_documents");
                for step in diagnostic["next"].as_array().unwrap() {
                    let step = step.as_str().unwrap();
                    assert_eq!(
                        step.contains("--no-ignore"),
                        no_ignore && step.starts_with("mdrg "),
                        "{step}"
                    );
                    assert!(!step.contains("--paths"), "{step}");
                    assert!(step.matches("--hidden").count() <= 1, "{step}");
                }
            }
        }
    }
}

#[test]
fn rank_no_documents_preserves_explicit_no_ignore() {
    assert_no_documents_preserves_explicit_no_ignore(&["search", "--q", "needle"]);
}

#[test]
fn exact_no_documents_preserves_explicit_no_ignore() {
    assert_no_documents_preserves_explicit_no_ignore(&[
        "search", "--q", "needle", "--mode", "exact",
    ]);
}

#[test]
fn list_no_documents_preserves_explicit_no_ignore() {
    assert_no_documents_preserves_explicit_no_ignore(&["list"]);
}

#[test]
fn other_hints_do_not_add_no_ignore_without_user_input() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("note.md"), "# needle\n").unwrap();
    for args in [
        vec!["search", "--q", "absent"],
        vec!["search", "--q", "needle absent", "--mode", "exact"],
        vec!["search", "--q", "  "],
        vec!["search", "--q", "needle", "--paths", "missing/*"],
        vec!["list", "--paths", "missing/*"],
        vec!["list", "--heading-level", "1", "--paths", "note.md"],
    ] {
        let mut args = args;
        if args.contains(&"--heading-level") {
            std::fs::write(root.path().join("note.md"), "## needle\n").unwrap();
        }
        for hidden in [false, true] {
            if hidden {
                args.push("--hidden");
            }
            let diagnostic = hint(&run(&root, &args));
            assert!(
                diagnostic["next"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|step| { !step.as_str().unwrap().contains("--no-ignore") })
            );
        }
    }
}
