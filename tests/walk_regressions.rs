use mdq_rg::walk::{WalkOptions, collect};
use serde_json::Value;
use std::path::Path;
use std::process::{Command, Output};

fn run(root: &Path, args: &[&str]) -> Output {
    Command::new(
        std::env::var_os("MDRG_WALK_PROBE_BINARY")
            .unwrap_or_else(|| env!("CARGO_BIN_EXE_mdrg").into()),
    )
    .current_dir(root)
    .env("MDRG_NO_USAGE_LOG", "1")
    .args(args)
    .output()
    .unwrap()
}

fn paths(root: &Path, options: WalkOptions) -> Vec<String> {
    collect(root, &options).into_iter().map(|p| p.0).collect()
}

#[test]
fn cwd_below_permanent_exclusion_keeps_own_documents() {
    let temp = tempfile::tempdir().unwrap();
    let mut valid = true;
    for parent in ["node_modules", ".git"] {
        let root = temp.path().join(parent).join("pkg");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("r.md"), "# T\nneedle\n").unwrap();
        for args in [
            vec!["search", "--q", "needle"],
            vec!["get", "r.md:1-2"],
            vec!["list"],
        ] {
            let out = run(&root, &args);
            println!(
                "{parent} {args:?}: exit={:?} stdout={} stderr={}",
                out.status.code(),
                String::from_utf8_lossy(&out.stdout).trim_end(),
                String::from_utf8_lossy(&out.stderr).trim_end()
            );
            valid &= out.status.success() && !out.stdout.is_empty();
        }
    }
    assert!(valid);
}

#[test]
fn rgignore_applies_unless_no_ignore() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.md"), "needle").unwrap();
    std::fs::write(root.path().join(".rgignore"), "a.md\n").unwrap();
    assert!(paths(root.path(), WalkOptions::default()).is_empty());
    assert_eq!(
        paths(
            root.path(),
            WalkOptions {
                no_ignore: true,
                ..Default::default()
            }
        ),
        ["a.md"]
    );
}

#[test]
fn gitignore_requires_repository() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.md"), "needle").unwrap();
    std::fs::write(root.path().join(".gitignore"), "a.md\n").unwrap();
    assert_eq!(paths(root.path(), WalkOptions::default()), ["a.md"]);
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(root.path())
            .status()
            .unwrap()
            .success()
    );
    assert!(paths(root.path(), WalkOptions::default()).is_empty());
}

#[test]
fn roots_normalize_names_excludes_duplicates_and_get_roundtrip() {
    let root = tempfile::tempdir().unwrap();
    std::fs::create_dir(root.path().join("docs")).unwrap();
    std::fs::write(root.path().join("docs/a.md"), "# T\nneedle\n").unwrap();
    std::fs::write(root.path().join("docs/secret.md"), "secret").unwrap();
    let mut cases = vec!["['docs/..']", "['docs','docs/../docs']", "['docs/../docs']"];
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink("docs", root.path().join("alias")).unwrap();
        cases.push("['alias']");
    }
    for roots in cases {
        std::fs::write(
            root.path().join("mdrg.toml"),
            format!("roots = {roots}\nexclude = ['docs/secret.md']\n"),
        )
        .unwrap();
        let out = run(root.path(), &["list"]);
        assert!(out.status.success(), "{out:?}");
        let rows: Vec<Value> = String::from_utf8(out.stdout)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(rows.len(), 1, "{roots}: {rows:?}");
        assert_eq!(rows[0]["path"], "docs/a.md");
        assert!(run(root.path(), &["get", "docs/a.md:1-2"]).status.success());
    }
}

#[test]
fn huge_globs_do_not_abort_paths_or_excludes() {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("a.md"), "needle").unwrap();
    let glob = "*".repeat(200_000);
    let cli_glob = "*".repeat(100_000);
    let out = run(
        root.path(),
        &["search", "--q", "needle", "--paths", &cli_glob],
    );
    println!(
        "huge paths: exit={:?} stdout={} stderr={}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).trim_end(),
        String::from_utf8_lossy(&out.stderr).trim_end()
    );
    assert!(out.status.success() && !out.stdout.is_empty(), "{out:?}");
    std::fs::write(
        root.path().join("mdrg.toml"),
        format!("exclude = ['{glob}']\n"),
    )
    .unwrap();
    let out = run(root.path(), &["list"]);
    assert!(out.status.success() && out.stdout.is_empty(), "{out:?}");
}

#[cfg(unix)]
#[test]
fn unreadable_directory_reports_relative_path_and_get_stays_quiet() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let blocked = root.path().join("blocked");
    std::fs::create_dir(&blocked).unwrap();
    std::fs::write(root.path().join("a.md"), "needle").unwrap();
    std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o0)).unwrap();
    assert!(std::fs::read_dir(&blocked).is_err());
    let list = run(root.path(), &["list"]);
    let get = run(root.path(), &["get", "a.md:1-1"]);
    std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(
        serde_json::from_slice::<Value>(&list.stderr).unwrap()["skipped"]["path"],
        "blocked"
    );
    assert!(get.status.success() && get.stderr.is_empty(), "{get:?}");
}

#[test]
fn huge_globs_collect_six_hundred_documents_under_budget() {
    let root = tempfile::tempdir().unwrap();
    for i in 0..600 {
        std::fs::write(root.path().join(format!("{i}.md")), "needle").unwrap();
    }
    let glob = "*".repeat(200_000);
    for exclude in [false, true] {
        let options = if exclude {
            WalkOptions {
                exclude: vec![glob.clone()],
                ..Default::default()
            }
        } else {
            WalkOptions {
                paths: vec![glob.clone()],
                ..Default::default()
            }
        };
        let start = std::time::Instant::now();
        let found = collect(root.path(), &options);
        let elapsed = start.elapsed();
        println!("600 documents exclude={exclude}: {elapsed:?}");
        assert_eq!(found.len(), if exclude { 0 } else { 600 });
        assert!(
            elapsed < std::time::Duration::from_millis(100),
            "{elapsed:?}"
        );
    }
    let cli_glob = "*".repeat(100_000);
    let out = run(
        root.path(),
        &["search", "--q", "needle", "--paths", &cli_glob, "--timing"],
    );
    assert!(out.status.success(), "{out:?}");
    println!(
        "600 documents CLI paths: {}",
        String::from_utf8_lossy(&out.stderr).trim_end()
    );
    std::fs::write(
        root.path().join("mdrg.toml"),
        format!("exclude = ['{glob}']\n"),
    )
    .unwrap();
    let out = run(root.path(), &["search", "--q", "needle", "--timing"]);
    assert!(out.status.success() && out.stdout.is_empty(), "{out:?}");
    println!(
        "600 documents CLI exclude: {}",
        String::from_utf8_lossy(&out.stderr).trim_end()
    );
}

#[test]
fn unclosed_classes_search_six_hundred_documents_under_budget() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let root = tempfile::tempdir().unwrap();
    for i in 0..600 {
        std::fs::write(root.path().join(format!("{i}.md")), "needle").unwrap();
    }
    let glob = "[".repeat(50_000);
    // Retry short scans to allow for unrelated scheduling and filesystem delays.
    let mut fastest = f64::INFINITY;
    for _ in 0..3 {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mdrg"))
            .current_dir(root.path())
            .env("MDRG_NO_USAGE_LOG", "1")
            .args(["search", "--q", "needle", "--paths", &glob, "--timing"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let start = Instant::now();
        while child.try_wait().unwrap().is_none() {
            if start.elapsed() > Duration::from_secs(10) {
                child.kill().unwrap();
                let _ = child.wait();
                panic!("search did not finish within 10 seconds");
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let out = child.wait_with_output().unwrap();
        assert!(out.status.success() && out.stdout.is_empty(), "{out:?}");
        let stderr = String::from_utf8(out.stderr).unwrap();
        assert!(stderr.contains("\"cause\":\"paths_matched_nothing\""));
        let scan_ms: f64 = stderr
            .split_whitespace()
            .find_map(|field| field.strip_prefix("scan_ms="))
            .unwrap()
            .parse()
            .unwrap();
        fastest = fastest.min(scan_ms);
    }
    println!("600 documents, 50000 unclosed classes: scan_ms={fastest}");
    assert!(fastest < 100.0, "scan_ms={fastest}");
}
