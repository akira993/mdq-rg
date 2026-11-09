use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

const EXCLUDED_ADVICE: &str = "This path contains a permanently excluded name.";

fn fixture() -> tempfile::TempDir {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("work")).unwrap();
    root
}

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mdrg"))
        .current_dir(root)
        .env("MDRG_NO_USAGE_LOG", "1")
        .args(args)
        .output()
        .unwrap()
}

fn failure(output: &Output, code: &str) -> Value {
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let value: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(value["error"]["code"], code, "{value}");
    value
}

fn replay(root: &Path, advice: &str) -> Output {
    Command::new("sh")
        .args(["-c", advice])
        .current_dir(root)
        .env("MDRG_NO_USAGE_LOG", "1")
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
        .output()
        .unwrap()
}

#[test]
fn outside_permanently_excluded_document_has_no_read_command() {
    let root = fixture();
    let work = root.path().join("work");
    for name in [".ssh", ".env", ".aws", ".gnupg", ".git", "node_modules"] {
        let directory = root.path().join(name).join("nested");
        std::fs::create_dir_all(&directory).unwrap();
        let document = directory.join("note.md");
        std::fs::write(&document, "PUBLIC_TEST_MARKER\n").unwrap();
        let range = format!("{}:1-1", document.display());
        let value = failure(
            &run(&work, &["get", &range, "--hidden", "--no-ignore"]),
            "outside_root",
        );
        assert_eq!(value["error"]["next"], serde_json::json!([EXCLUDED_ADVICE]));
        let attempted = replay(&work, value["error"]["next"][0].as_str().unwrap());
        assert!(!attempted.status.success());
        assert!(attempted.stdout.is_empty(), "{attempted:?}");
    }
}

#[test]
fn outside_hidden_directory_keeps_replayable_advice() {
    let root = fixture();
    let directory = root.path().join(".cfg");
    std::fs::create_dir(&directory).unwrap();
    let document = directory.join("note.md");
    std::fs::write(&document, "ordinary hidden directory\n").unwrap();
    let range = format!("{}:1-1", document.display());
    let value = failure(
        &run(&root.path().join("work"), &["get", &range]),
        "outside_root",
    );
    let result = replay(
        &root.path().join("work"),
        value["error"]["next"][0].as_str().unwrap(),
    );
    assert!(
        result.status.success() && result.stderr.is_empty(),
        "{result:?}"
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&result.stdout).unwrap()["text"],
        "ordinary hidden directory\n"
    );
}

#[cfg(unix)]
#[test]
fn non_markdown_symlink_to_hidden_directory_reports_type() {
    let root = fixture();
    std::fs::create_dir(root.path().join(".cfg")).unwrap();
    std::fs::write(root.path().join(".cfg/note.txt"), "body\n").unwrap();
    std::os::unix::fs::symlink(".cfg/note.txt", root.path().join("alias.txt")).unwrap();
    failure(
        &run(root.path(), &["get", "alias.txt:1-1"]),
        "unsupported_type",
    );
    failure(
        &run(root.path(), &["get", "alias.txt:1-1", "--hidden"]),
        "unsupported_type",
    );
}

#[cfg(target_os = "macos")]
#[test]
fn absolute_non_markdown_through_tmp_alias_reports_unsupported_type() {
    let root = tempfile::Builder::new()
        .prefix("mdrg-review-")
        .tempdir_in("/tmp")
        .unwrap();
    std::fs::write(root.path().join("note.txt"), "body\n").unwrap();
    let range = format!("{}/note.txt:1-1", root.path().display());
    assert!(range.starts_with("/tmp/"));
    failure(&run(root.path(), &["get", &range]), "unsupported_type");
}

#[test]
fn help_says_outside_patterns_are_rejected_with_error() {
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
        assert!(
            help.contains("patterns pointing outside it are rejected with an error."),
            "{help}"
        );
    }
}

fn assert_replay_preserves_flags(name: &str, flags: &[&str]) {
    let root = fixture();
    let directory = root.path().join("target");
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(directory.join(name), "replay body\n").unwrap();
    std::fs::write(
        directory.join(".ignore"),
        "ignored.md\n.hidden-ignored.md\n",
    )
    .unwrap();
    let range = format!("{}:1-1", directory.join(name).display());
    let mut args = vec!["get", &range];
    args.extend_from_slice(flags);
    let value = failure(&run(&root.path().join("work"), &args), "outside_root");
    let advice = value["error"]["next"][0].as_str().unwrap();
    for flag in flags {
        assert!(advice.contains(flag), "{advice}");
    }
    let result = replay(&root.path().join("work"), advice);
    assert!(
        result.status.success() && result.stderr.is_empty(),
        "{result:?}"
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&result.stdout).unwrap()["text"],
        "replay body\n"
    );
}

#[test]
fn outside_replay_preserves_hidden_flag() {
    assert_replay_preserves_flags(".hidden.md", &["--hidden"]);
}

#[test]
fn outside_replay_preserves_no_ignore_flag() {
    assert_replay_preserves_flags("ignored.md", &["--no-ignore"]);
}

#[test]
fn outside_replay_preserves_both_visibility_flags() {
    assert_replay_preserves_flags(".hidden-ignored.md", &["--hidden", "--no-ignore"]);
}

#[test]
fn excluded_non_markdown_remains_not_found() {
    let root = fixture();
    std::fs::write(root.path().join("blocked.txt"), "body\n").unwrap();
    std::fs::write(root.path().join("mdrg.toml"), "exclude = ['blocked.txt']\n").unwrap();
    failure(&run(root.path(), &["get", "blocked.txt:1-1"]), "not_found");
}

#[cfg(unix)]
#[test]
fn symlink_to_permanently_excluded_non_markdown_is_not_found() {
    let root = fixture();
    std::fs::create_dir(root.path().join(".ssh")).unwrap();
    std::fs::write(root.path().join(".ssh/note.txt"), "body\n").unwrap();
    std::os::unix::fs::symlink(".ssh/note.txt", root.path().join("alias.txt")).unwrap();
    failure(
        &run(root.path(), &["get", "alias.txt:1-1", "--hidden"]),
        "not_found",
    );
}

#[test]
fn hidden_flag_does_not_allow_permanently_excluded_non_markdown() {
    let root = fixture();
    std::fs::create_dir(root.path().join(".ssh")).unwrap();
    std::fs::write(root.path().join(".ssh/note.txt"), "body\n").unwrap();
    failure(
        &run(root.path(), &["get", ".ssh/note.txt:1-1", "--hidden"]),
        "not_found",
    );
}
