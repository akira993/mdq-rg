use serde_json::Value;
use std::fs;
use std::path::Path;
use std::process::{Command, Output};
use tempfile::TempDir;

fn command(work: &Path, state: &Path) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mdrg"));
    command
        .current_dir(work)
        .env("XDG_STATE_HOME", state)
        .env_remove("MDRG_NO_USAGE_LOG");
    command
}

fn run(command: &mut Command) -> Output {
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "status={:?}, stderr={}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

fn rows(state: &Path) -> Vec<Value> {
    fs::read_to_string(state.join("mdrg/usage.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn usage_one_line_per_command_and_no_document_body() {
    let work = TempDir::new().unwrap();
    let state = TempDir::new().unwrap();
    fs::write(
        work.path().join("note.md"),
        "# Greeting\nhello PRIVATE_DOCUMENT_BODY_524\n",
    )
    .unwrap();
    let output = run(command(work.path(), state.path()).args(["search", "--q", "hello"]));
    assert!(String::from_utf8_lossy(&output.stdout).contains("PRIVATE_DOCUMENT_BODY_524"));
    assert_eq!(rows(state.path()).len(), 1);
    let row = &rows(state.path())[0];
    assert_eq!(row["command"], "search");
    assert_eq!(row["args"]["q"], "hello");
    assert_eq!(row["result"]["hits"], 1);
    assert_eq!(row["exit"], 0);
    assert_eq!(row["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(
        row["cwd"],
        work.path().canonicalize().unwrap().to_str().unwrap()
    );
    assert!(row["ts"].as_u64().unwrap() > 0);
    assert!(row["elapsed_ms"].as_f64().unwrap() >= 0.0);
    let raw = fs::read_to_string(state.path().join("mdrg/usage.jsonl")).unwrap();
    assert!(!raw.contains("PRIVATE_DOCUMENT_BODY_524"));
    let mut cursor = 0;
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
        cursor += raw[cursor..].find(&format!("\"{key}\":")).unwrap() + key.len() + 3;
    }
    for (index, args) in [["get", "note.md:1-2"].as_slice(), ["list"].as_slice()]
        .iter()
        .enumerate()
    {
        let output = command(work.path(), state.path())
            .args(*args)
            .output()
            .unwrap();
        let rows = rows(state.path());
        assert_eq!(rows.len(), index + 2);
        assert_eq!(rows[index + 1]["command"], args[0]);
        assert_eq!(rows[index + 1]["exit"], output.status.code().unwrap());
    }
}

#[test]
fn usage_disabled_by_config_creates_no_directory() {
    let work = TempDir::new().unwrap();
    let state = TempDir::new().unwrap();
    fs::write(work.path().join("mdrg.toml"), "usage_log = false\n").unwrap();
    run(command(work.path(), state.path()).args(["search", "--q", "hello"]));
    assert!(!state.path().join("mdrg").exists());
}

#[test]
fn usage_disabled_by_environment_creates_no_directory() {
    let work = TempDir::new().unwrap();
    let state = TempDir::new().unwrap();
    run(command(work.path(), state.path())
        .env("MDRG_NO_USAGE_LOG", "1")
        .args(["search", "--q", "hello"]));
    assert!(!state.path().join("mdrg").exists());
}

#[test]
fn usage_environment_only_one_disables_logging() {
    let work = TempDir::new().unwrap();
    let state = TempDir::new().unwrap();
    run(command(work.path(), state.path())
        .env("MDRG_NO_USAGE_LOG", "true")
        .args(["search", "--q", "hello"]));
    assert_eq!(rows(state.path()).len(), 1);
}

#[test]
fn usage_falls_back_to_home_for_missing_or_relative_xdg() {
    let work = TempDir::new().unwrap();
    for xdg in [None, Some("relative-state"), Some("")] {
        let home = TempDir::new().unwrap();
        let mut cmd = command(work.path(), home.path());
        cmd.env("HOME", home.path())
            .args(["search", "--q", "hello"]);
        if let Some(xdg) = xdg {
            cmd.env("XDG_STATE_HOME", xdg);
        } else {
            cmd.env_remove("XDG_STATE_HOME");
        }
        run(&mut cmd);
        assert_eq!(rows(&home.path().join(".local/state")).len(), 1);
    }
    assert!(!work.path().join("relative-state").exists());
}

fn assert_warning(output: &Output) {
    let stderr = String::from_utf8(output.stderr.clone()).unwrap();
    let warnings: Vec<Value> = stderr
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|value| value.get("warning").is_some())
        .collect();
    assert_eq!(warnings.len(), 1, "stderr={stderr}");
    assert_eq!(warnings[0]["warning"]["code"], "usage_log_failed");
    assert!(
        !warnings[0]["warning"]["detail"]
            .as_str()
            .unwrap()
            .is_empty()
    );
}

#[test]
fn usage_missing_state_location_warns_without_failing() {
    let work = TempDir::new().unwrap();
    let mut cmd = command(work.path(), work.path());
    cmd.env_remove("XDG_STATE_HOME")
        .env_remove("HOME")
        .args(["search", "--q", "hello"]);
    assert_warning(&run(&mut cmd));
    assert!(!work.path().join("mdrg").exists());
}

#[test]
fn usage_unwritable_location_warns_without_failing() {
    let work = TempDir::new().unwrap();
    let state = TempDir::new().unwrap();
    let blocked = state.path().join("blocked");
    fs::write(&blocked, "not a directory").unwrap();
    assert_warning(&run(
        command(work.path(), &blocked).args(["search", "--q", "hello"])
    ));
}

#[cfg(unix)]
#[test]
fn usage_symlink_file_warns_without_changing_target() {
    use std::os::unix::fs::symlink;

    let work = TempDir::new().unwrap();
    fs::write(work.path().join("note.md"), "# Greeting\nhello\n").unwrap();
    for existing in [true, false] {
        let state = TempDir::new().unwrap();
        let target = state.path().join("target.jsonl");
        if existing {
            fs::write(&target, "do not append\n").unwrap();
        }
        fs::create_dir(state.path().join("mdrg")).unwrap();
        symlink(&target, state.path().join("mdrg/usage.jsonl")).unwrap();
        let output = run(command(work.path(), state.path()).args(["search", "--q", "hello"]));
        assert_warning(&output);
        assert!(!output.stdout.is_empty());
        if existing {
            assert_eq!(fs::read(&target).unwrap(), b"do not append\n");
        } else {
            assert!(!target.exists());
        }
    }
}

#[cfg(unix)]
#[test]
fn usage_symlink_directory_warns_without_changing_target() {
    use std::os::unix::fs::symlink;

    let work = TempDir::new().unwrap();
    fs::write(work.path().join("note.md"), "# Greeting\nhello\n").unwrap();
    for existing in [true, false] {
        let state = TempDir::new().unwrap();
        let target = TempDir::new().unwrap();
        let file = target.path().join("usage.jsonl");
        if existing {
            fs::write(&file, "do not append\n").unwrap();
        }
        symlink(target.path(), state.path().join("mdrg")).unwrap();
        let output = run(command(work.path(), state.path()).args(["search", "--q", "hello"]));
        assert_warning(&output);
        assert!(!output.stdout.is_empty());
        if existing {
            assert_eq!(fs::read(&file).unwrap(), b"do not append\n");
        } else {
            assert!(!file.exists());
        }
    }
}

#[cfg(unix)]
#[test]
fn usage_hard_link_warns_without_changing_target() {
    let work = TempDir::new().unwrap();
    let state = TempDir::new().unwrap();
    let target = state.path().join("target.jsonl");
    fs::write(&target, "do not append\n").unwrap();
    fs::create_dir(state.path().join("mdrg")).unwrap();
    fs::hard_link(&target, state.path().join("mdrg/usage.jsonl")).unwrap();
    assert_warning(&run(
        command(work.path(), state.path()).args(["search", "--q", "hello"])
    ));
    assert_eq!(fs::read(&target).unwrap(), b"do not append\n");
}

#[cfg(unix)]
#[test]
fn usage_non_regular_file_warns_without_failing() {
    let work = TempDir::new().unwrap();
    let state = TempDir::new().unwrap();
    let path = state.path().join("mdrg/usage.jsonl");
    fs::create_dir_all(&path).unwrap();
    assert_warning(&run(
        command(work.path(), state.path()).args(["search", "--q", "hello"])
    ));
    assert!(path.is_dir());
    assert_eq!(fs::read_dir(path).unwrap().count(), 0);
}

#[cfg(unix)]
#[test]
fn usage_regular_file_appends_one_line_with_private_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let work = TempDir::new().unwrap();
    let state = TempDir::new().unwrap();
    fs::write(work.path().join("note.md"), "# Greeting\nhello\n").unwrap();
    for count in 1..=2 {
        let output = run(command(work.path(), state.path()).args(["search", "--q", "hello"]));
        assert!(output.stderr.is_empty(), "{output:?}");
        assert_eq!(rows(state.path()).len(), count);
        assert_eq!(
            fs::metadata(state.path().join("mdrg/usage.jsonl"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
    }
}

#[cfg(unix)]
#[test]
fn usage_permissions_are_private_and_existing_modes_are_preserved() {
    use std::os::unix::fs::PermissionsExt;
    let work = TempDir::new().unwrap();
    let state = TempDir::new().unwrap();
    run(command(work.path(), state.path()).args(["search", "--q", "hello"]));
    let directory = state.path().join("mdrg");
    let file = directory.join("usage.jsonl");
    assert_eq!(
        fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o750)).unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).unwrap();
    run(command(work.path(), state.path()).args(["search", "--q", "hello"]));
    assert_eq!(
        fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
        0o750
    );
    assert_eq!(
        fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert_eq!(rows(state.path()).len(), 2);
}

#[test]
fn usage_eight_processes_append_complete_json_lines() {
    let work = TempDir::new().unwrap();
    let state = TempDir::new().unwrap();
    let query = "hello\"\\\n\t".repeat(1024);
    let children: Vec<_> = (0..8)
        .map(|_| {
            command(work.path(), state.path())
                .args(["search", "--q", &query])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .spawn()
                .unwrap()
        })
        .collect();
    for mut child in children {
        assert!(child.wait().unwrap().success());
    }
    let rows = rows(state.path());
    assert_eq!(rows.len(), 8);
    assert!(rows.iter().all(|row| row["args"]["q"] == query));
}

#[test]
fn usage_keeps_git_worktree_unchanged() {
    let work = TempDir::new().unwrap();
    let state = TempDir::new().unwrap();
    run(Command::new("git")
        .current_dir(work.path())
        .args(["init", "--quiet"]));
    fs::write(work.path().join("note.md"), "# Greeting\nhello\n").unwrap();
    let status = || {
        run(Command::new("git")
            .current_dir(work.path())
            .args(["status", "--porcelain"]))
        .stdout
    };
    let before = status();
    run(command(work.path(), state.path()).args(["search", "--q", "hello"]));
    assert_eq!(status(), before);
    assert_eq!(rows(state.path()).len(), 1);
}

#[test]
fn usage_record_cost() {
    const ITERATIONS: usize = 1000;
    if std::env::var_os("MDRG_USAGE_BENCH_CHILD").is_some() {
        use mdq_rg::usage_log::{LogValue, Outcome, record};
        let config = mdq_rg::config::Config::default();
        let outcome = Outcome {
            exit: 0,
            args: vec![("q", LogValue::Str("hello".into()))],
            result: vec![("hits", LogValue::Int(1))],
        };
        let start = std::time::Instant::now();
        for _ in 0..ITERATIONS {
            record(&config, "search", &outcome, 0.0);
        }
        let average_ms = start.elapsed().as_secs_f64() * 1000.0 / ITERATIONS as f64;
        println!("usage_record_average_ms={average_ms:.6} iterations={ITERATIONS}");
        assert!(average_ms < 1.0, "average record cost was {average_ms} ms");
        return;
    }
    let state = TempDir::new().unwrap();
    let output = run(Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "usage_record_cost", "--nocapture"])
        .env("MDRG_USAGE_BENCH_CHILD", "1")
        .env("XDG_STATE_HOME", state.path())
        .env_remove("MDRG_NO_USAGE_LOG"));
    println!("{}", String::from_utf8_lossy(&output.stdout));
    assert_eq!(rows(state.path()).len(), ITERATIONS);
}

#[cfg(unix)]
fn assert_unreadable_skipped(args: &[&str], directory: bool) {
    use std::os::unix::fs::PermissionsExt;

    struct RestorePermissions(std::path::PathBuf, fs::Permissions);
    impl Drop for RestorePermissions {
        fn drop(&mut self) {
            fs::set_permissions(&self.0, self.1.clone()).unwrap();
        }
    }

    let work = TempDir::new().unwrap();
    let state = TempDir::new().unwrap();
    fs::write(work.path().join("a.md"), "# T\nneedle\n").unwrap();
    let locked = work
        .path()
        .join(if directory { "locked" } else { "locked.md" });
    if directory {
        fs::create_dir(&locked).unwrap();
        fs::write(locked.join("x.md"), "# L\nneedle\n").unwrap();
    } else {
        fs::write(&locked, "# L\nneedle\n").unwrap();
    }
    let restore = RestorePermissions(locked.clone(), fs::metadata(&locked).unwrap().permissions());
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
    if directory {
        assert!(
            fs::read_dir(&locked).is_err(),
            "directory must be unreadable"
        );
    } else {
        assert!(fs::read(&locked).is_err(), "file must be unreadable");
    }
    let output = run(command(work.path(), state.path()).args(args));
    drop(restore);
    let stderr = String::from_utf8(output.stderr).unwrap();
    let skipped: Vec<Value> = stderr
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|value| value.get("skipped").is_some())
        .collect();
    assert_eq!(skipped.len(), 1, "stderr={stderr}");
    assert_eq!(
        skipped[0]["skipped"]["path"],
        if directory { "locked" } else { "locked.md" }
    );
    assert_eq!(skipped[0]["skipped"]["reason"], "read_error");
    assert_eq!(rows(state.path())[0]["result"]["skipped"], skipped.len());
    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8(output.stdout).unwrap().contains("a.md"));
}

#[cfg(unix)]
#[test]
fn usage_search_counts_unreadable_directory() {
    assert_unreadable_skipped(&["search", "--q", "needle"], true);
}

#[cfg(unix)]
#[test]
fn usage_list_counts_unreadable_directory() {
    assert_unreadable_skipped(&["list"], true);
}

#[cfg(unix)]
#[test]
fn usage_search_counts_unreadable_file_once() {
    assert_unreadable_skipped(&["search", "--q", "needle"], false);
}

#[cfg(unix)]
#[test]
fn usage_list_counts_unreadable_file_once() {
    assert_unreadable_skipped(&["list"], false);
}
