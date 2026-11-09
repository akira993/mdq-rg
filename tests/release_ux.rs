use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mdrg"))
        .current_dir(root)
        .env("MDRG_NO_USAGE_LOG", "1")
        .args(args)
        .output()
        .unwrap()
}

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("docs")).unwrap();
    std::fs::write(root.path().join("README.md"), "# Parent\nneedle\n").unwrap();
    std::fs::write(root.path().join("docs/README.md"), "# Child\nneedle\n").unwrap();
    std::fs::write(root.path().join("docs/b.md"), "# Second\nneedle\n").unwrap();
    root
}

fn error(output: &Output, code: &str, exit: i32) -> Value {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    assert_eq!(String::from_utf8_lossy(&output.stderr).lines().count(), 1);
    let value: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(value["error"]["code"], code, "{value}");
    assert_eq!(value["error"].as_object().unwrap().len(), 3);
    assert!(!value["error"]["detail"].as_str().unwrap().is_empty());
    let next = value["error"]["next"].as_array().unwrap();
    assert!(!next.is_empty());
    assert!(
        next.iter()
            .all(|action| action.as_str().is_some_and(|action| !action.is_empty()))
    );
    if code == "outside_root" {
        assert_eq!(next.len(), 1);
    }
    value
}

#[test]
fn new_errors_share_code_detail_and_string_array_next() {
    let root = fixture();
    std::fs::write(root.path().join("plain.rs"), "body\n").unwrap();
    std::fs::write(root.path().join("ignored.md"), "body\n").unwrap();
    std::fs::write(root.path().join(".ignore"), "ignored.md\n").unwrap();
    let outside = format!("{}:1-1", root.path().join("README.md").display());
    for (args, code, exit) in [
        (vec!["--root", "missing", "list"], "invalid_root", 2),
        (
            vec!["search", "--q", "needle", "--paths", "../*.md"],
            "paths_outside_root",
            2,
        ),
        (vec!["--root", "docs", "get", &outside], "outside_root", 1),
        (vec!["get", "plain.rs:1-1"], "unsupported_type", 1),
        (vec!["get", "ignored.md:1-1"], "ignored", 1),
    ] {
        error(&run(root.path(), &args), code, exit);
    }
}

#[test]
fn help_explains_scope_paths_json_lines_and_empty_results() {
    let root = fixture();
    for args in [
        vec!["--help"],
        vec!["search", "--help"],
        vec!["get", "--help"],
        vec!["list", "--help"],
    ] {
        let output = run(root.path(), &args);
        assert!(output.status.success() && output.stderr.is_empty());
        let help = String::from_utf8(output.stdout).unwrap();
        for required in [
            "cwd",
            "--root",
            "--paths",
            "relative to",
            "outside",
            "JSON Lines",
            "search, get and list write",
            "stdout",
            "stderr",
            "next",
        ] {
            assert!(
                help.contains(required),
                "{args:?}: missing {required}: {help}"
            );
        }
    }
}

#[test]
fn root_before_or_after_command_matches_running_in_selected_directory() {
    let root = fixture();
    let docs = root.path().join("docs");
    for args in [
        vec!["search", "--q", "needle", "--top-k", "2"],
        vec!["search", "--q", "needle", "--mode", "exact"],
        vec!["get", "README.md:1-1"],
        vec!["list", "--limit", "1"],
    ] {
        let expected = run(&docs, &args);
        for selected in ["docs".to_owned(), docs.to_string_lossy().into_owned()] {
            for position in [0, 1] {
                let mut with_root = args.clone();
                with_root.splice(position..position, ["--root", selected.as_str()]);
                let actual = run(root.path(), &with_root);
                assert_eq!(actual.status.code(), expected.status.code(), "{actual:?}");
                assert_eq!(actual.stdout, expected.stdout);
                assert_eq!(actual.stderr, expected.stderr);
            }
        }
    }
}

#[test]
fn root_uses_selected_configuration_and_relative_result_paths() {
    let root = fixture();
    std::fs::write(root.path().join("mdrg.toml"), "invalid = true\n").unwrap();
    std::fs::write(root.path().join("docs/mdrg.toml"), "exclude = ['b.md']\n").unwrap();
    for args in [
        vec!["--root", "docs", "list"],
        vec!["search", "--root", "docs", "--q", "needle"],
        vec!["get", "--root", "docs", "README.md:1-1"],
    ] {
        let output = run(root.path(), &args);
        assert!(
            output.status.success() && output.stderr.is_empty(),
            "{output:?}"
        );
        let rows: Vec<Value> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["path"], "README.md");
    }
}

#[test]
fn root_loads_parent_hidden_configuration_from_child() {
    let root = fixture();
    let child = root.path().join("docs");
    std::fs::write(root.path().join("mdrg.toml"), "hidden = true\n").unwrap();
    std::fs::create_dir(root.path().join(".claude")).unwrap();
    std::fs::write(root.path().join(".claude/hidden.md"), "# Hidden\nneedle\n").unwrap();
    assert!(!child.join("mdrg.toml").exists());

    for args in [
        vec!["list", "--paths", ".claude/**", "--limit", "1"],
        vec![
            "search",
            "--q",
            "needle",
            "--paths",
            ".claude/**",
            "--top-k",
            "1",
        ],
    ] {
        let mut selected = vec!["--root", ".."];
        selected.extend_from_slice(&args);
        let output = run(&child, &selected);
        assert!(
            output.status.success() && output.stderr.is_empty(),
            "{selected:?}: {output:?}"
        );
        let rows: Vec<Value> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(rows.len(), 1, "{selected:?}: {rows:?}");
        assert_eq!(rows[0]["path"], ".claude/hidden.md");

        let local = run(&child, &args);
        assert!(
            local.status.success() && local.stdout.is_empty(),
            "{args:?}: {local:?}"
        );
    }
}

#[test]
fn invalid_root_reports_usage_error_and_next_action() {
    let root = fixture();
    for name in ["does-not-exist", "README.md"] {
        let output = run(root.path(), &["--root", name, "list"]);
        let value = error(&output, "invalid_root", 2);
        assert!(
            value["error"]["next"][0]
                .as_str()
                .unwrap()
                .contains("--root")
        );
    }
}

#[test]
fn paths_outside_root_rejects_all_results_for_search_and_list() {
    let root = fixture();
    let docs = root.path().join("docs");
    let absolute = format!("{}/*.md", root.path().display());
    for command in [vec!["search", "--q", "needle"], vec!["list"]] {
        let unresolved_absolute = format!("{}/missing/../../*.md", docs.display());
        for pattern in [
            "../*.md",
            "nested/../../*.md",
            absolute.as_str(),
            unresolved_absolute.as_str(),
        ] {
            for mixed in [false, true] {
                let mut args = command.clone();
                args.push("--paths");
                if mixed {
                    args.push("*.md");
                }
                args.push(pattern);
                let output = run(&docs, &args);
                let value = error(&output, "paths_outside_root", 2);
                let next = value["error"]["next"][0].as_str().unwrap();
                assert!(next.contains("--root") && next.contains("relative"));
                assert!(
                    value["error"]["detail"]
                        .as_str()
                        .unwrap()
                        .contains("outside")
                );
            }
        }
        let mut args = command;
        args.extend(["--paths", "missing/*.md"]);
        let output = run(&docs, &args);
        assert!(output.status.success() && output.stdout.is_empty());
        let value: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(value["no_results"]["cause"], "paths_matched_nothing");
        assert!(value["no_results"]["next"].is_array());
    }
}

#[test]
fn absolute_paths_inside_root_preserve_unmatched_pattern_diagnostics() {
    let root = fixture();
    for directory in [
        root.path().to_path_buf(),
        root.path().canonicalize().unwrap(),
        root.path().join("missing/.."),
    ] {
        let pattern = format!("{}/*.md", directory.display());
        for mut args in [vec!["search", "--q", "needle"], vec!["list"]] {
            args.extend(["--paths", &pattern]);
            let output = run(root.path(), &args);
            assert!(
                output.status.success() && output.stdout.is_empty(),
                "{output:?}"
            );
            let value: Value = serde_json::from_slice(&output.stderr).unwrap();
            assert_eq!(value["no_results"]["cause"], "paths_matched_nothing");
        }
    }
}

#[test]
fn paths_keep_in_root_parents_and_dots_in_names() {
    let root = fixture();
    std::fs::create_dir(root.path().join("notes..")).unwrap();
    std::fs::write(root.path().join("notes../a.md"), "# Notes\nneedle\n").unwrap();
    for command in [vec!["search", "--q", "needle"], vec!["list"]] {
        let mut args = command.clone();
        args.extend(["--paths", "notes../*.md"]);
        let output = run(root.path(), &args);
        assert!(
            output.status.success() && output.stderr.is_empty(),
            "{output:?}"
        );
        let rows: Vec<Value> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["path"], "notes../a.md");
        for pattern in ["notes../../*.md", "missing/../*.md", "./docs/../*.md"] {
            let mut args = command.clone();
            args.extend(["--paths", pattern]);
            let output = run(root.path(), &args);
            assert!(
                output.status.success() && output.stdout.is_empty(),
                "{output:?}"
            );
            let value: Value = serde_json::from_slice(&output.stderr).unwrap();
            assert_eq!(value["no_results"]["cause"], "paths_matched_nothing");
        }
    }
}

#[test]
fn outside_get_next_replays_from_launch_directory_with_root_override() {
    let root = fixture();
    let range = format!("{}:1-1", root.path().join("README.md").display());
    let output = run(root.path(), &["--root", "docs", "get", &range]);
    let value = error(&output, "outside_root", 1);
    let next = value["error"]["next"][0].as_str().unwrap();
    let replay = Command::new("sh")
        .args(["-c", next])
        .env(
            "PATH",
            format!(
                "{}:{}",
                Path::new(env!("CARGO_BIN_EXE_mdrg"))
                    .parent()
                    .unwrap()
                    .display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("MDRG_NO_USAGE_LOG", "1")
        .current_dir(root.path())
        .output()
        .unwrap();
    let expected = run(
        root.path(),
        &[
            "--root",
            root.path().to_str().unwrap(),
            "get",
            "README.md:1-1",
        ],
    );
    assert!(
        replay.status.success() && replay.stderr.is_empty(),
        "{next}: {replay:?}"
    );
    assert_eq!(replay.stdout, expected.stdout);
    assert_eq!(String::from_utf8_lossy(&replay.stdout).lines().count(), 1);
    let row: Value = serde_json::from_slice(&replay.stdout).unwrap();
    assert_eq!(row["path"], "README.md");
    assert_eq!(row["text"], "# Parent\n");
}

#[test]
fn outside_get_suggests_a_replayable_parent_root() {
    let root = fixture();
    let docs = root.path().join("docs");
    let range = format!("{}:1-1", root.path().join("README.md").display());
    let output = run(&docs, &["get", &range]);
    let value = error(&output, "outside_root", 1);
    assert!(
        value["error"]["detail"]
            .as_str()
            .unwrap()
            .contains("selected root")
    );
    let next = value["error"]["next"][0].as_str().unwrap();
    let replay = Command::new("sh")
        .args(["-c", next])
        .env(
            "PATH",
            format!(
                "{}:{}",
                Path::new(env!("CARGO_BIN_EXE_mdrg"))
                    .parent()
                    .unwrap()
                    .display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .env("MDRG_NO_USAGE_LOG", "1")
        .current_dir(&docs)
        .output()
        .unwrap();
    assert!(
        replay.status.success() && replay.stderr.is_empty(),
        "{next}: {replay:?}"
    );
    let parent = run(&docs, &["--root", "..", "get", "README.md:1-1"]);
    assert_eq!(replay.stdout, parent.stdout);
    assert!(String::from_utf8_lossy(&parent.stdout).contains("# Parent"));
}

#[test]
fn search_json_lines_are_independent_and_help_shows_line_reader() {
    let root = fixture();
    for mode in ["rank", "exact"] {
        let output = run(
            root.path(),
            &[
                "search", "--q", "needle", "--paths", "docs/**", "--top-k", "2", "--mode", mode,
            ],
        );
        assert!(output.status.success() && output.stderr.is_empty());
        let rows: Vec<Value> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(rows.len(), 2);
        for row in rows {
            assert!(row["path"].is_string() && row["snippet"].is_string());
        }
    }
    let help = String::from_utf8(run(root.path(), &["search", "--help"]).stdout).unwrap();
    assert!(help.contains("for line in sys.stdin") && help.contains("json.loads(line)"));
}

#[test]
fn non_markdown_get_explains_type_and_missing_keeps_not_found() {
    let root = fixture();
    std::fs::write(root.path().join("plain.rs"), "body\n").unwrap();
    let value = error(
        &run(root.path(), &["get", "plain.rs:1-1"]),
        "unsupported_type",
        1,
    );
    assert!(
        value["error"]["detail"]
            .as_str()
            .unwrap()
            .contains("Markdown")
    );
    assert!(
        value["error"]["next"][0]
            .as_str()
            .unwrap()
            .contains("plain text")
    );
    for range in ["missing.md:1-1", "missing.rs:1-1"] {
        let output = run(root.path(), &["get", range]);
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let value: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(value["error"]["code"], "not_found");
    }
}

#[test]
fn non_markdown_diagnostics_preserve_hidden_and_permanent_exclusions() {
    let root = fixture();
    for name in [".hidden.txt", "node_modules/plain.txt"] {
        let file = root.path().join(name);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, "body\n").unwrap();
        let output = run(root.path(), &["get", &format!("{name}:1-1")]);
        assert_eq!(output.status.code(), Some(1));
        let value: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(value["error"]["code"], "not_found");
        assert!(output.stdout.is_empty());
    }
    std::fs::write(root.path().join("mdrg.toml"), "roots = ['docs']\n").unwrap();
    std::fs::write(root.path().join("outside.txt"), "body\n").unwrap();
    let output = run(root.path(), &["get", "outside.txt:1-1"]);
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"]["code"],
        "not_found"
    );
}

#[test]
fn usage_errors_create_no_logs_and_runtime_errors_keep_existing_log_shape() {
    let root = fixture();
    let state = tempfile::tempdir().unwrap();
    for args in [
        vec!["--root", "missing", "list"],
        vec!["search", "--q", "needle", "--paths", "../*.md"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_mdrg"))
            .current_dir(root.path())
            .env_remove("MDRG_NO_USAGE_LOG")
            .env("XDG_STATE_HOME", state.path())
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(!state.path().join("mdrg").exists());
    }
    std::fs::write(root.path().join("docs/plain.rs"), "body\n").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_mdrg"))
        .current_dir(root.path())
        .env_remove("MDRG_NO_USAGE_LOG")
        .env("XDG_STATE_HOME", state.path())
        .args(["--root", "docs", "get", "plain.rs:1-1"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let log = std::fs::read_to_string(state.path().join("mdrg/usage.jsonl")).unwrap();
    let row: Value = serde_json::from_str(&log).unwrap();
    assert_eq!(
        row["cwd"],
        root.path()
            .join("docs")
            .canonicalize()
            .unwrap()
            .to_str()
            .unwrap()
    );
    assert_eq!(
        row["args"],
        serde_json::json!({"range":"plain.rs:1-1", "hidden":false, "no_ignore":false})
    );
    assert_eq!(row["result"], serde_json::json!({"found":false}));
    assert_eq!(row["exit"], 1);
}

#[test]
fn ignored_get_explains_exclusion_without_changing_ignore_rules() {
    let root = fixture();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(root.path())
            .status()
            .unwrap()
            .success()
    );
    std::fs::write(root.path().join("ignored.md"), "body\n").unwrap();
    for ignore in [".gitignore", ".ignore", ".rgignore"] {
        std::fs::write(root.path().join(ignore), "ignored.md\n").unwrap();
        let value = error(&run(root.path(), &["get", "ignored.md:1-1"]), "ignored", 1);
        assert!(
            value["error"]["detail"]
                .as_str()
                .unwrap()
                .contains("ignore rule")
        );
        let next = value["error"]["next"][0].as_str().unwrap();
        assert!(next.contains("intentional") && !next.contains("--no-ignore"));
        assert_eq!(
            std::fs::read_to_string(root.path().join(ignore)).unwrap(),
            "ignored.md\n"
        );
        assert!(
            run(root.path(), &["get", "ignored.md:1-1", "--no-ignore"])
                .status
                .success()
        );
        std::fs::write(root.path().join(ignore), "").unwrap();
    }
}

#[test]
fn exact_help_explains_sections_limits_and_all_line_tools() {
    let root = fixture();
    let help = String::from_utf8(run(root.path(), &["search", "--help"]).stdout).unwrap();
    for required in [
        "exact",
        "sections",
        "--top-k",
        "--max-tokens",
        "rg -n -F",
        "rg -c -F",
    ] {
        assert!(help.contains(required), "missing {required}: {help}");
    }
}
