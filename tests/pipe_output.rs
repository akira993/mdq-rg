use std::fs;
use std::io::{BufRead, BufReader};
use std::process::{Command, Stdio};

#[cfg(unix)]
fn closed_stderr(args: &[&str], invalid_document: bool, expected_exit: i32, expected_rows: usize) {
    use std::io::Write;
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("a.md"), "# Topic\nneedle\n").unwrap();
    if invalid_document {
        fs::write(root.path().join("bad.md"), [0xff]).unwrap();
    }
    let mut child = Command::new("sh")
        .args([
            "-c",
            "read gate; exec \"$@\"",
            "sh",
            env!("CARGO_BIN_EXE_mdrg"),
        ])
        .args(args)
        .current_dir(root.path())
        .env("MDRG_NO_USAGE_LOG", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stderr.take());
    child.stdin.take().unwrap().write_all(b"go\n").unwrap();
    let output = child.wait_with_output().unwrap();
    assert_eq!(
        output.status.code(),
        Some(expected_exit),
        "{args:?}: {output:?}"
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    let rows: Vec<serde_json::Value> = stdout
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows.len(), expected_rows, "{args:?}: {rows:?}");
    if let Some(row) = rows.first() {
        assert_eq!(row["path"], "a.md");
    }
}

#[cfg(unix)]
#[test]
fn search_with_closed_stderr_preserves_results_and_exit() {
    closed_stderr(&["search", "--q", "needle", "--timing"], false, 0, 1);
}

#[cfg(unix)]
#[test]
fn search_no_match_with_closed_stderr_is_success() {
    closed_stderr(&["search", "--q", "absent"], false, 0, 0);
}

#[cfg(unix)]
#[test]
fn search_skipped_document_with_closed_stderr_preserves_results() {
    closed_stderr(&["search", "--q", "needle"], true, 0, 1);
}

#[cfg(unix)]
#[test]
fn get_failure_with_closed_stderr_keeps_runtime_exit() {
    closed_stderr(&["get", "missing.md:1-1"], false, 1, 0);
}

#[cfg(unix)]
#[test]
fn list_skipped_document_with_closed_stderr_preserves_results() {
    closed_stderr(&["list"], true, 0, 1);
}

fn closed_pipe(args: &[&str], body: String) {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fs::write(root.path().join("a.md"), body).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_mdrg"))
        .args(args)
        .current_dir(root.path())
        .env("XDG_STATE_HOME", state.path())
        .env_remove("MDRG_NO_USAGE_LOG")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut reader = BufReader::with_capacity(1, child.stdout.take().unwrap());
    let mut first = String::new();
    assert!(reader.read_line(&mut first).unwrap() > 0);
    drop(reader);
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let log = fs::read_to_string(state.path().join("mdrg/usage.jsonl")).unwrap();
    assert_eq!(log.lines().count(), 1);
    let entry: serde_json::Value = serde_json::from_str(log.trim()).unwrap();
    assert_eq!(entry["exit"], 0);
    assert_eq!(entry["command"], args[0]);
}

#[test]
fn list_closed_pipe_is_success_and_logged() {
    closed_pipe(&["list"], "# Topic\nneedle\n".repeat(10_000));
}

#[test]
fn search_closed_pipe_is_success_and_logged() {
    closed_pipe(
        &[
            "search",
            "--q",
            "needle",
            "--top-k",
            "10000",
            "--max-tokens",
            "10000000",
        ],
        "# Topic\nneedle\n".repeat(10_000),
    );
}

#[test]
fn get_closed_pipe_is_success_and_logged() {
    let root = tempfile::tempdir().unwrap();
    let state = tempfile::tempdir().unwrap();
    fs::write(root.path().join("a.md"), "needle".repeat(100_000)).unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_mdrg"))
        .args(["get", "a.md:1-1"])
        .current_dir(root.path())
        .env("XDG_STATE_HOME", state.path())
        .env_remove("MDRG_NO_USAGE_LOG")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    let log = fs::read_to_string(state.path().join("mdrg/usage.jsonl")).unwrap();
    assert_eq!(log.lines().count(), 1);
    let entry: serde_json::Value = serde_json::from_str(log.trim()).unwrap();
    assert_eq!(entry["exit"], 0);
    assert_eq!(entry["command"], "get");
}

#[cfg(target_os = "linux")]
#[test]
fn non_pipe_output_errors_fail_and_are_logged() {
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("a.md"), "needle\n").unwrap();
    for args in [
        vec!["list"],
        vec!["search", "--q", "needle"],
        vec!["get", "a.md:1-1"],
    ] {
        let state = tempfile::tempdir().unwrap();
        let output = Command::new(env!("CARGO_BIN_EXE_mdrg"))
            .args(&args)
            .current_dir(root.path())
            .env("XDG_STATE_HOME", state.path())
            .env_remove("MDRG_NO_USAGE_LOG")
            .stdout(
                fs::OpenOptions::new()
                    .write(true)
                    .open("/dev/full")
                    .unwrap(),
            )
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(1), "{args:?}: {output:?}");
        assert!(!output.stderr.is_empty());
        let log = fs::read_to_string(state.path().join("mdrg/usage.jsonl")).unwrap();
        assert_eq!(log.lines().count(), 1);
        let entry: serde_json::Value = serde_json::from_str(log.trim()).unwrap();
        assert_eq!(entry["exit"], 1);
        assert_eq!(entry["command"], args[0]);
    }
}
