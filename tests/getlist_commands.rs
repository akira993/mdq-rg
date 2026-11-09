use serde_json::{Value, json};
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

fn rows(output: &Output) -> Vec<Value> {
    String::from_utf8(output.stdout.clone())
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn failure(root: &Path, range: &str, code: &str, exit: i32) {
    let output = run(root, &["get", range]);
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    assert!(output.stdout.is_empty());
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], code);
}

#[test]
fn get_preserves_lines_and_uses_last_colon_and_start_heading() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("a:b.md"),
        "---\r\ntitle: x\r\n---\r\n# A\r\ntext\r\n## B\r\nlast",
    )
    .unwrap();
    let output = run(root.path(), &["get", "a:b.md:5-7"]);
    assert!(output.status.success());
    assert_eq!(
        rows(&output),
        [json!({"path":"a:b.md","lines":[5,7],"heading_path":"A","text":"text\r\n## B\r\nlast"})]
    );
    let output = run(root.path(), &["get", "a:b.md:1-3"]);
    assert_eq!(rows(&output)[0]["text"], "---\r\ntitle: x\r\n---\r\n");
    assert_eq!(rows(&output)[0]["heading_path"], "(preface)");
}

#[test]
fn get_rejects_bad_ranges_and_missing_lines() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.md"), "one\n").unwrap();
    for range in [
        "a.md",
        "a.md:2-1",
        "a.md:1-x",
        "a.md:+1-2",
        "a.md:1-2-3",
        ":1-1",
    ] {
        failure(root.path(), range, "invalid_range", 2);
    }
    for range in ["a.md:0-1", "a.md:2-2"] {
        failure(root.path(), range, "out_of_range", 1);
    }
    assert_eq!(
        rows(&run(root.path(), &["get", "a.md:1-2"]))[0]["lines"],
        json!([1, 1])
    );
    std::fs::write(root.path().join("empty.md"), "").unwrap();
    failure(root.path(), "empty.md:1-1", "out_of_range", 1);
}

#[test]
fn get_respects_ignore_hidden_permanent_exclusions_and_config() {
    let root = tempfile::tempdir().unwrap();
    for name in ["ignored.md", ".hidden.md", "a.txt", "ok.md"] {
        std::fs::write(root.path().join(name), "body").unwrap();
    }
    std::fs::write(root.path().join(".gitignore"), "ignored.md\n").unwrap();
    for dir in [".git", "node_modules"] {
        std::fs::create_dir(root.path().join(dir)).unwrap();
        std::fs::write(root.path().join(dir).join("a.md"), "body").unwrap();
        let output = run(
            root.path(),
            &["get", &format!("{dir}/a.md:1-1"), "--hidden", "--no-ignore"],
        );
        assert_eq!(output.status.code(), Some(1));
    }
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(root.path())
            .status()
            .unwrap()
            .success()
    );
    failure(root.path(), "ignored.md:1-1", "ignored", 1);
    failure(root.path(), "a.txt:1-1", "unsupported_type", 1);
    failure(root.path(), ".hidden.md:1-1", "hidden", 1);
    failure(root.path(), "missing.md:1-1", "not_found", 1);
    assert!(
        run(root.path(), &["get", "ignored.md:1-1", "--no-ignore"])
            .status
            .success()
    );
    assert!(
        run(root.path(), &["get", ".hidden.md:1-1", "--hidden"])
            .status
            .success()
    );
    std::fs::write(
        root.path().join("mdrg.toml"),
        "hidden = true\nexclude = ['ok.md']\n",
    )
    .unwrap();
    assert!(
        run(root.path(), &["get", ".hidden.md:1-1"])
            .status
            .success()
    );
    failure(root.path(), "ignored.md:1-1", "ignored", 1);
    assert!(
        run(root.path(), &["get", "ignored.md:1-1", "--no-ignore"])
            .status
            .success()
    );
    failure(root.path(), "ok.md:1-1", "not_found", 1);
}

#[test]
fn get_checks_absolute_relative_and_symlink_boundaries() {
    let root = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.md"), "inside").unwrap();
    std::fs::write(outside.path().join("a.md"), "outside").unwrap();
    failure(root.path(), "../missing.md:1-1", "outside_root", 1);
    failure(
        root.path(),
        &format!("{}/a.md:1-1", outside.path().display()),
        "outside_root",
        1,
    );
    assert!(
        run(
            root.path(),
            &["get", &format!("{}/a.md:1-1", root.path().display())]
        )
        .status
        .success()
    );
    assert!(run(root.path(), &["get", "./a.md:1-1"]).status.success());
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink(outside.path(), root.path().join("escape")).unwrap();
        symlink(outside.path().join("a.md"), root.path().join("link.md")).unwrap();
        failure(root.path(), "escape/a.md:1-1", "outside_root", 1);
        failure(root.path(), "escape/missing.md:1-1", "outside_root", 1);
        failure(root.path(), "link.md:1-1", "outside_root", 1);
        symlink(root.path(), outside.path().join("alias")).unwrap();
        let output = run(&outside.path().join("alias"), &["get", "a.md:1-1"]);
        assert!(output.status.success(), "{output:?}");
    }
}

#[test]
fn list_orders_filters_limits_and_handles_empty_documents() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("z.md"), "---\nx: y\n---\n").unwrap();
    std::fs::write(root.path().join("empty.md"), "").unwrap();
    std::fs::write(
        root.path().join("a.md"),
        "preface\n# A\nbody\n## B\nbody\n# C\n",
    )
    .unwrap();
    let output = run(root.path(), &["list"]);
    assert!(output.status.success());
    let data = rows(&output);
    assert_eq!(data.len(), 6);
    assert_eq!(
        data[0],
        json!({"path":"a.md","lines":[1,1],"heading_path":"(preface)","level":0})
    );
    assert_eq!(
        data[2],
        json!({"path":"a.md","lines":[4,5],"heading_path":"A > B","level":2})
    );
    for row in &data[4..] {
        assert_eq!(row["lines"], json!([0, 0]));
    }
    assert_eq!(
        rows(&run(root.path(), &["list", "--heading-level", "1"])).len(),
        5
    );
    assert_eq!(
        rows(&run(root.path(), &["list", "--limit", "2"])),
        data[..2]
    );
    let output = run(root.path(), &["list", "--limit", "0"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert_eq!(
        rows(&run(root.path(), &["list", "--paths", "a*"])),
        data[..4]
    );
    for row in &data[..4] {
        let range = format!(
            "{}:{}-{}",
            row["path"].as_str().unwrap(),
            row["lines"][0],
            row["lines"][1]
        );
        let output = run(root.path(), &["get", &range]);
        assert!(output.status.success());
        assert_eq!(rows(&output)[0]["heading_path"], row["heading_path"]);
        assert!(!rows(&output)[0]["text"].as_str().unwrap().is_empty());
    }
}

#[test]
fn list_skips_invalid_documents_and_get_returns_error() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("bad.md"), [0xff]).unwrap();
    std::fs::write(root.path().join("nul.md"), b"a\0b").unwrap();
    std::fs::write(root.path().join("ok.md"), "body").unwrap();
    let output = run(root.path(), &["list"]);
    assert!(output.status.success());
    assert_eq!(rows(&output).len(), 1);
    let errors: Vec<Value> = String::from_utf8(output.stderr)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(
        errors,
        [
            json!({"skipped":{"path":"bad.md","reason":"invalid_utf8"}}),
            json!({"skipped":{"path":"nul.md","reason":"contains_nul"}})
        ]
    );
    failure(root.path(), "bad.md:1-1", "read_error", 1);
    failure(root.path(), "nul.md:1-1", "read_error", 1);
}

#[cfg(unix)]
#[test]
fn unreadable_document_is_skipped_by_list_and_errors_in_get() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("unreadable.md");
    std::fs::write(&path, "body").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o0)).unwrap();
    assert!(
        std::fs::read(&path).is_err(),
        "This test requires an unprivileged process."
    );
    failure(root.path(), "unreadable.md:1-1", "read_error", 1);
    let output = run(root.path(), &["list"]);
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    let errors: Vec<Value> = String::from_utf8(output.stderr)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        errors[0],
        json!({"skipped":{"path":"unreadable.md","reason":"read_error"}})
    );
    assert_eq!(errors[1]["no_results"]["cause"], "no_documents");
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}

#[test]
fn list_reports_no_documents_and_unmatched_paths() {
    let root = tempfile::tempdir().unwrap();
    let output = run(root.path(), &["list"]);
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stderr).unwrap()["no_results"]["cause"],
        "no_documents"
    );
    std::fs::write(root.path().join("a.md"), "body").unwrap();
    let output = run(root.path(), &["list", "--paths", "missing*"]);
    assert!(output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stderr).unwrap()["no_results"]["cause"],
        "paths_matched_nothing"
    );
}

#[cfg(unix)]
#[test]
fn list_unmatched_paths_reports_unreadable_directory_once() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let blocked = root.path().join("blocked");
    std::fs::create_dir(&blocked).unwrap();
    std::fs::write(root.path().join("a.md"), "body").unwrap();
    std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o0)).unwrap();
    assert!(std::fs::read_dir(&blocked).is_err());
    let output = run(root.path(), &["list", "--paths", "missing*"]);
    std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    let errors: Vec<Value> = String::from_utf8(output.stderr)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(
        errors,
        [
            json!({"skipped":{"path":"blocked","reason":"read_error"}}),
            json!({"no_results":{
                "cause":"paths_matched_nothing",
                "detail":"No documents matched --paths.",
                "next":["Check the --paths patterns.", "mdrg list"]
            }})
        ]
    );
}

#[test]
fn zero_result_limits_are_usage_errors_but_zero_token_budget_keeps_first_hit() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.md"), "# A\nneedle\n").unwrap();
    for args in [
        vec!["search", "--q", "needle", "--top-k", "0"],
        vec!["list", "--limit", "0"],
    ] {
        let output = run(root.path(), &args);
        assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
        assert!(output.stdout.is_empty());
        assert!(!output.stderr.is_empty());
    }
    let output = run(
        root.path(),
        &["search", "--q", "needle", "--max-tokens", "0"],
    );
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(rows(&output).len(), 1);
}

#[test]
fn list_reports_no_match_when_heading_filter_removes_all_sections() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.md"), "## A\nbody\n").unwrap();
    let output = run(root.path(), &["list", "--heading-level", "1"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["no_results"]["cause"], "no_match");
    assert!(
        error["no_results"]["next"]
            .as_array()
            .unwrap()
            .iter()
            .any(|hint| hint.as_str().unwrap().contains("--heading-level"))
    );
}

#[test]
fn list_reports_no_documents_when_all_documents_are_invalid() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("bad.md"), [0xff]).unwrap();
    let output = run(root.path(), &["list"]);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty());
    let errors: Vec<Value> = String::from_utf8(output.stderr)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(errors.len(), 2);
    assert_eq!(errors[0]["skipped"]["reason"], "invalid_utf8");
    assert_eq!(errors[1]["no_results"]["cause"], "no_documents");
}

#[test]
fn list_unmatched_paths_hint_explains_how_to_fix_paths() {
    let root = tempfile::tempdir().unwrap();
    let no_documents: Value = serde_json::from_slice(&run(root.path(), &["list"]).stderr).unwrap();
    std::fs::write(root.path().join("a.md"), "body").unwrap();
    let output = run(root.path(), &["list", "--paths", "missing*"]);
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["no_results"]["cause"], "paths_matched_nothing");
    assert_ne!(
        error["no_results"]["next"],
        no_documents["no_results"]["next"]
    );
    assert!(
        error["no_results"]["next"]
            .as_array()
            .unwrap()
            .iter()
            .any(|hint| hint.as_str().unwrap().contains("--paths"))
    );
}

#[test]
fn configured_roots_and_flags_select_the_same_documents_for_get_and_list() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join(".docs")).unwrap();
    for name in [
        ".docs/ok.md",
        ".docs/excluded.md",
        "outside.md",
        "ignored.md",
        ".hidden.md",
    ] {
        std::fs::write(root.path().join(name), "# Title\nbody\n").unwrap();
    }
    std::fs::write(root.path().join(".gitignore"), "ignored.md\n").unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(root.path())
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(rows(&run(root.path(), &["list"])).len(), 1);
    assert_eq!(
        rows(&run(root.path(), &["list", "--hidden", "--no-ignore"])).len(),
        5
    );
    std::fs::write(
        root.path().join("mdrg.toml"),
        "roots = ['.docs']\nexclude = ['*excluded.md']\n",
    )
    .unwrap();
    let output = run(root.path(), &["list"]);
    assert_eq!(
        rows(&output),
        [json!({"path":".docs/ok.md","lines":[1,2],"heading_path":"Title","level":1})]
    );
    assert!(
        run(root.path(), &["get", ".docs/ok.md:1-2"])
            .status
            .success()
    );
    failure(root.path(), "outside.md:1-1", "not_found", 1);
    failure(root.path(), ".docs/excluded.md:1-1", "not_found", 1);
}
