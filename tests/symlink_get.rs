#![cfg(unix)]

use serde_json::{Value, json};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

const BODY: &str = "# Test\ntest second\nthird\n";

struct Fixture {
    temp: tempfile::TempDir,
    project: PathBuf,
    home: PathBuf,
    outside: PathBuf,
}

fn write(root: &Path, name: &str, bytes: impl AsRef<[u8]>) {
    let path = root.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
}

fn link(root: &Path, target: impl AsRef<Path>, name: &str) {
    let path = root.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    symlink(target, path).unwrap();
}

// Every invocation has a real deadline, including undisclosed cycles.
fn output(mut command: Command) -> Output {
    let start = Instant::now();
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if start.elapsed() >= Duration::from_secs(5) {
            child.kill().unwrap();
            let result = child.wait_with_output().unwrap();
            panic!("command exceeded five seconds: {result:?}");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

impl Fixture {
    fn new() -> Self {
        // Keep the system TMPDIR, including macOS /var -> /private/var aliases.
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        let home = temp.path().join("home");
        let outside = temp.path().join("out");
        std::fs::create_dir_all(&home).unwrap();
        for name in [
            "a.md",
            "docs/doc.md",
            ".h/doc.md",
            "ignored.md",
            "excluded/doc.md",
            "excluded/plain.txt",
            "node_modules/doc.md",
            "plain.txt",
        ] {
            write(&project, name, BODY);
        }
        write(&project, "empty.md", "");
        write(&project, "bad.md", [0xff]);
        write(&project, ".gitignore", "ignored.md\nignored-link.md\n");
        write(
            &project,
            "mdrg.toml",
            "exclude = ['excluded/**', 'excluded-link.md']\n",
        );
        assert!(
            Command::new("git")
                .args(["init", "-q"])
                .current_dir(&project)
                .env("HOME", &home)
                .status()
                .unwrap()
                .success()
        );
        write(&outside, "doc.md", BODY);
        for (target, name) in [
            ("a.md", "link.md"),
            ("a.md", "alias.txt"),
            ("a.md", "ignored-link.md"),
            ("a.md", "excluded-link.md"),
            ("../a.md", ".h/link.md"),
            ("../a.md", "node_modules/link.md"),
            ("../a.md", ".ssh/link.md"),
            ("link.md", "chain.md"),
            ("docs", "alias"),
            ("node_modules", "keys"),
            (".h/doc.md", "hidden-link.md"),
            ("ignored.md", "ignored-target.md"),
            ("excluded/doc.md", "excluded-target.md"),
            ("excluded/plain.txt", "excluded-text.txt"),
            ("node_modules/doc.md", "forbidden-target.md"),
            ("plain.txt", "plain-link.md"),
            ("empty.md", "empty-link.md"),
            ("bad.md", "bad-link.md"),
            ("missing.md", "broken.md"),
        ] {
            link(&project, target, name);
        }
        for (target, name) in [
            (outside.join("doc.md"), ".ssh/out.md"),
            (outside.join("missing.md"), "broken-out.md"),
            (outside.join("doc.md"), "outside.md"),
            (outside.clone(), "outside"),
            (outside.join("cycle-a.md"), "out-cycle.md"),
        ] {
            link(&project, target, name);
        }
        for directory in [
            &project,
            &project.join("node_modules"),
            &project.join("excluded"),
            &outside,
        ] {
            link(directory, "cycle-b.md", "cycle-a.md");
            link(directory, "cycle-a.md", "cycle-b.md");
        }
        Self {
            temp,
            project,
            home,
            outside,
        }
    }

    fn command(&self, root: &Path) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mdrg"));
        command
            .current_dir(&self.project)
            .arg("--root")
            .arg(root)
            .env("HOME", &self.home)
            .env("XDG_STATE_HOME", self.temp.path().join("state"))
            .env("MDRG_NO_USAGE_LOG", "1");
        command
    }

    fn get(&self, root: &Path, range: &str, flags: &[&str]) -> Output {
        let mut command = self.command(root);
        command.args(["get", range]).args(flags);
        output(command)
    }

    fn pair(
        &self,
        alias: &str,
        target: &str,
        flags: &[&str],
        code: Option<&str>,
        exit: i32,
    ) -> Output {
        self.pair_at(&self.project, alias, target, flags, code, exit)
    }

    fn pair_at(
        &self,
        root: &Path,
        alias: &str,
        target: &str,
        flags: &[&str],
        code: Option<&str>,
        exit: i32,
    ) -> Output {
        let alias = self.get(root, alias, flags);
        let target = self.get(root, target, flags);
        assert_eq!(
            alias.status.code(),
            target.status.code(),
            "{alias:?} / {target:?}"
        );
        assert_eq!(alias.stdout, target.stdout, "{alias:?} / {target:?}");
        assert_eq!(alias.stderr, target.stderr, "{alias:?} / {target:?}");
        assert_eq!(alias.status.code(), Some(exit), "{alias:?}");
        if let Some(code) = code {
            error(&alias, code, exit);
        } else {
            assert!(alias.stderr.is_empty(), "{alias:?}");
            assert_eq!(
                alias
                    .stdout
                    .split(|b| *b == b'\n')
                    .filter(|line| !line.is_empty())
                    .count(),
                1
            );
            let row: Value = serde_json::from_slice(&alias.stdout).unwrap();
            assert_eq!(row.as_object().unwrap().len(), 4);
            assert_eq!(row["path"], target_path(&target));
        }
        alias
    }

    fn replay(&self, result: &Output) -> Output {
        let row: Value = serde_json::from_slice(&result.stderr).unwrap();
        let next = row["error"]["next"][0].as_str().unwrap().replacen(
            "mdrg",
            env!("CARGO_BIN_EXE_mdrg"),
            1,
        );
        let mut command = Command::new("sh");
        command
            .args(["-c", &next])
            .current_dir(&self.project)
            .env("HOME", &self.home)
            .env("XDG_STATE_HOME", self.temp.path().join("state"))
            .env("MDRG_NO_USAGE_LOG", "1");
        output(command)
    }
}

fn target_path(result: &Output) -> Value {
    serde_json::from_slice::<Value>(&result.stdout).unwrap()["path"].clone()
}

fn error(result: &Output, code: &str, exit: i32) -> Value {
    assert_eq!(result.status.code(), Some(exit), "{result:?}");
    assert!(result.stdout.is_empty(), "{result:?}");
    assert_eq!(result.stderr.iter().filter(|b| **b == b'\n').count(), 1);
    let row: Value = serde_json::from_slice(&result.stderr).unwrap();
    assert_eq!(row["error"]["code"], code, "{row}");
    row["error"].clone()
}

fn depth(result: &Output) {
    let err = error(result, "read_error", 1);
    assert!(err["detail"].as_str().unwrap().contains("depth"));
    assert_eq!(err["next"].as_array().unwrap().len(), 1);
    assert!(err["next"][0].as_str().unwrap().contains("retry"));
}

#[test]
fn non_symlink_missing_parent_traversal_keeps_legacy_success() {
    let f = Fixture::new();
    let result = f.pair("nodir/../a.md:1-3", "a.md:1-3", &[], None, 0);
    assert_eq!(target_path(&result), "a.md");
}

#[test]
fn non_symlink_file_case_mismatch_is_not_found_on_case_insensitive_filesystems_too() {
    // Case-sensitive filesystems reject the name naturally; this guards the
    // canonicalization regression on case-insensitive filesystems such as APFS.
    let f = Fixture::new();
    assert_eq!(f.get(&f.project, "a.md:1-3", &[]).status.code(), Some(0));
    f.pair("A.md:1-3", "missing.md:1-3", &[], Some("not_found"), 1);
}

#[test]
fn non_symlink_directory_case_mismatch_is_not_found_on_case_insensitive_filesystems_too() {
    // Keep both spellings meaningful on case-insensitive filesystems, while
    // asserting the same not_found contract on case-sensitive filesystems.
    let f = Fixture::new();
    let root = f.temp.path().join("case-project");
    write(&root, "Docs/d.md", BODY);
    assert_eq!(f.get(&root, "Docs/d.md:1-3", &[]).status.code(), Some(0));
    f.pair_at(
        &root,
        "docs/d.md:1-3",
        "missing.md:1-3",
        &[],
        Some("not_found"),
        1,
    );
}

#[test]
fn a1_regular_chain_directory_and_extension_match_final_target() {
    let f = Fixture::new();
    for alias in ["link.md", "chain.md", "alias.txt"] {
        let result = f.pair(&format!("{alias}:1-3"), "a.md:1-3", &[], None, 0);
        assert_eq!(target_path(&result), "a.md");
        let row: Value = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(
            row,
            json!({"path":"a.md", "lines":[1,3], "heading_path":"Test", "text":BODY})
        );
    }
    let result = f.pair("alias/doc.md:1-3", "docs/doc.md:1-3", &[], None, 0);
    assert_eq!(target_path(&result), "docs/doc.md");
    f.pair(
        "plain-link.md:1-3",
        "plain.txt:1-3",
        &[],
        Some("unsupported_type"),
        1,
    );
}

#[test]
fn a1_directory_alias_missing_parent_traversal_matches_direct_path() {
    let f = Fixture::new();
    let result = f.pair(
        "alias/nodir/../doc.md:1-3",
        "docs/nodir/../doc.md:1-3",
        &[],
        None,
        0,
    );
    assert_eq!(target_path(&result), "docs/doc.md");
}

#[test]
fn a2_exponential_link_expansion_is_bounded_within_five_seconds() {
    let f = Fixture::new();
    link(&f.project, ".", "L0");
    for i in 1..=20 {
        link(&f.project, format!("L{0}/L{0}", i - 1), &format!("L{i}"));
    }
    depth(&f.get(&f.project, "L20/a.md:1-3", &[]));
    link(&f.project, "../L20", "node_modules/growth");
    undisclosed_cycles(&f, &["node_modules/growth/a.md:1-3"]);
}

#[test]
fn a2_twenty_thousand_links_are_bounded_within_five_seconds() {
    let f = Fixture::new();
    for i in 0..20_000 {
        let target = if i == 19_999 {
            "a.md".to_owned()
        } else {
            format!("long-{}.md", i + 1)
        };
        link(&f.project, target, &format!("long-{i}.md"));
    }
    depth(&f.get(&f.project, "long-0.md:1-3", &[]));
    link(&f.project, "../long-0.md", ".ssh/long.md");
    undisclosed_cycles(&f, &[".ssh/long.md:1-3"]);
}

#[test]
fn a2_over_limit_missing_target_chain_reports_depth_or_stays_undisclosed() {
    let f = Fixture::new();
    // Forty exceeds macOS's 32-link limit; fifty also exceeds Linux's 40.
    let lengths = if cfg!(target_os = "macos") {
        vec![40, 50]
    } else {
        vec![50]
    };
    for count in lengths {
        for i in 0..count {
            let target = if i == count - 1 {
                "missing.md".to_owned()
            } else {
                format!("missing-{count}-{}.md", i + 1)
            };
            link(&f.project, target, &format!("missing-{count}-{i}.md"));
        }
        depth(&f.get(&f.project, &format!("missing-{count}-0.md:1-3"), &[]));
        let name = format!("excluded/depth-{count}.md");
        link(&f.project, format!("../missing-{count}-0.md"), &name);
        undisclosed_cycles(&f, &[&format!("{name}:1-3")]);
    }
}

#[test]
fn a2_broken_cycles_and_fifty_link_depth_finish_within_five_seconds() {
    let f = Fixture::new();
    f.pair("broken.md:1-3", "missing.md:1-3", &[], Some("not_found"), 1);
    for name in ["cycle-a.md", "cycle-b.md"] {
        let result = f.get(&f.project, &format!("{name}:1-3"), &[]);
        let err = error(&result, "read_error", 1);
        assert!(err["detail"].as_str().unwrap().contains("cycle"));
        assert_eq!(err["next"].as_array().unwrap().len(), 1);
        let next = err["next"][0].as_str().unwrap();
        assert!(next.contains("cycle") && next.contains("retry"));
    }
    for i in 1..=50 {
        let target = if i == 50 {
            "a.md".to_owned()
        } else {
            format!("deep-{}.md", i + 1)
        };
        link(&f.project, target, &format!("deep-{i}.md"));
    }
    let err = error(&f.get(&f.project, "deep-1.md:1-3", &[]), "read_error", 1);
    assert!(err["detail"].as_str().unwrap().contains("depth"));
    assert_eq!(err["next"].as_array().unwrap().len(), 1);
    assert!(err["next"][0].as_str().unwrap().contains("retry"));
}

fn undisclosed_cycles(f: &Fixture, names: &[&str]) {
    for name in names {
        let result = f.pair(name, "missing.md:1-3", &[], Some("not_found"), 1);
        assert!(error(&result, "not_found", 1).get("next").is_none());
    }
}

#[test]
fn a2_cycles_in_node_modules_do_not_disclose() {
    undisclosed_cycles(
        &Fixture::new(),
        &["node_modules/cycle-a.md:1-3", "node_modules/cycle-b.md:1-3"],
    );
}

#[test]
fn a2_cycles_through_keys_do_not_disclose() {
    undisclosed_cycles(&Fixture::new(), &["keys/cycle-a.md:1-3"]);
}

#[test]
fn a2_cycles_outside_and_out_cycle_do_not_disclose() {
    let f = Fixture::new();
    let outside = format!("{}:1-3", f.outside.join("cycle-a.md").display());
    undisclosed_cycles(&f, &["out-cycle.md:1-3", &outside]);
}

#[test]
fn a2_cycles_in_excluded_do_not_disclose() {
    undisclosed_cycles(
        &Fixture::new(),
        &["excluded/cycle-a.md:1-3", "excluded/cycle-b.md:1-3"],
    );
    let f = Fixture::new();
    link(&f.project, "dir-cycle-b", "dir-cycle-a");
    link(&f.project, "dir-cycle-a", "dir-cycle-b");
    write(
        &f.project,
        "mdrg.toml",
        "exclude = ['dir-cycle-b/doc.md']\n",
    );
    undisclosed_cycles(&f, &["dir-cycle-a/doc.md:1-3"]);
    link(&f.project, "..", ".ssh/back");
    undisclosed_cycles(&f, &[".ssh/back/cycle-a.md:1-3"]);
    link(&f.outside, &f.project, "back");
    link(
        &f.project,
        f.outside.join("back/cycle-a.md"),
        "out-back-cycle.md",
    );
    undisclosed_cycles(&f, &["out-back-cycle.md:1-3"]);
}

#[test]
fn a2_permission_000_directory_requires_non_root_and_matches_direct_target() {
    assert_ne!(
        Command::new("id").arg("-u").output().unwrap().stdout,
        b"0\n",
        "this permission-000 test requires a non-root user; root bypasses directory permissions"
    );
    let f = Fixture::new();
    write(&f.project, "locked/a.md", BODY);
    link(&f.project, "locked/a.md", "locked-link.md");
    let locked = f.project.join("locked");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    let alias = f.get(&f.project, "locked-link.md:1-3", &[]);
    let direct = f.get(&f.project, "locked/a.md:1-3", &[]);
    std::fs::set_permissions(locked, std::fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(alias.status.code(), direct.status.code());
    assert_eq!(alias.stdout, direct.stdout);
    assert_eq!(alias.stderr, direct.stderr);
    error(&alias, "not_found", 1);
}

#[test]
fn a3_outside_broken_forbidden_and_home_pairs_match_with_replay() {
    let f = Fixture::new();
    let document = format!("{}:1-3", f.outside.join("doc.md").display());
    let missing = format!("{}:1-3", f.outside.join("missing.md").display());
    for alias in ["outside.md:1-3", "outside/doc.md:1-3"] {
        let result = f.pair(alias, &document, &[], Some("outside_root"), 1);
        assert_eq!(f.replay(&result).status.code(), Some(0));
    }
    for alias in ["outside/missing.md:1-3", "broken-out.md:1-3"] {
        f.pair(alias, &missing, &[], Some("outside_root"), 1);
    }
    write(&f.outside, "node_modules/x.md", BODY);
    link(&f.project, f.outside.join("node_modules/x.md"), "nm-out.md");
    f.pair(
        "nm-out.md:1-3",
        &format!("{}:1-3", f.outside.join("node_modules/x.md").display()),
        &[],
        Some("outside_root"),
        1,
    );
    write(&f.home, "notes.md", BODY);
    write(&f.home, "project/a.md", BODY);
    link(&f.home, "../notes.md", "project/notes-link.md");
    link(&f.home, "project/a.md", "a-link.md");
    f.pair_at(
        &f.home.join("project"),
        "notes-link.md:1-3",
        &format!("{}:1-3", f.home.join("notes.md").display()),
        &[],
        Some("outside_root"),
        1,
    );
    f.pair_at(
        &f.home,
        "a-link.md:1-3",
        "project/a.md:1-3",
        &[],
        Some("root_too_broad"),
        2,
    );
    link(&f.home, "project/missing.md", "broken-home.md");
    f.pair_at(
        &f.home,
        "broken-home.md:1-3",
        "project/missing.md:1-3",
        &[],
        Some("root_too_broad"),
        2,
    );
}

#[test]
fn a4_hidden_alias_and_target_match_including_guidance_and_configuration() {
    let f = Fixture::new();
    for flags in [vec![], vec!["--hidden"]] {
        f.pair(".h/link.md:1-3", "a.md:1-3", &flags, None, 0);
    }
    let result = f.pair(
        "hidden-link.md:1-3",
        ".h/doc.md:1-3",
        &[],
        Some("hidden"),
        1,
    );
    assert_eq!(f.replay(&result).status.code(), Some(0));
    f.pair(
        "hidden-link.md:1-3",
        ".h/doc.md:1-3",
        &["--hidden"],
        None,
        0,
    );
    write(
        &f.project,
        "mdrg.toml",
        "exclude = ['excluded/**', 'excluded-link.md']\nhidden = true\n",
    );
    f.pair("hidden-link.md:1-3", ".h/doc.md:1-3", &[], None, 0);
}

#[test]
fn a5_ignored_alias_and_target_match_without_suggesting_unignore() {
    let f = Fixture::new();
    f.pair("ignored-link.md:1-3", "a.md:1-3", &[], None, 0);
    let result = f.pair(
        "ignored-target.md:1-3",
        "ignored.md:1-3",
        &[],
        Some("ignored"),
        1,
    );
    assert_eq!(
        error(&result, "ignored", 1)["next"],
        json!(["Check whether the exclusion is intentional before changing the ignore rules."])
    );
    f.pair(
        "ignored-target.md:1-3",
        "ignored.md:1-3",
        &["--no-ignore"],
        None,
        0,
    );
}

#[test]
fn a6_excluded_and_forbidden_names_apply_to_final_target_only() {
    let f = Fixture::new();
    for alias in [
        "excluded-link.md:1-3",
        "node_modules/link.md:1-3",
        ".ssh/link.md:1-3",
    ] {
        f.pair(alias, "a.md:1-3", &[], None, 0);
    }
    f.pair(
        ".ssh/out.md:1-3",
        &format!("{}:1-3", f.outside.join("doc.md").display()),
        &[],
        Some("outside_root"),
        1,
    );
    for flags in [vec![], vec!["--hidden", "--no-ignore"]] {
        for (alias, target) in [
            ("excluded-target.md:1-3", "excluded/doc.md:1-3"),
            ("excluded-text.txt:1-3", "excluded/plain.txt:1-3"),
            ("forbidden-target.md:1-3", "node_modules/doc.md:1-3"),
        ] {
            let result = f.pair(alias, target, &flags, Some("not_found"), 1);
            assert!(error(&result, "not_found", 1).get("next").is_none());
        }
    }
}

#[test]
fn a7_roots_apply_to_final_target_and_cycles_outside_roots_do_not_disclose() {
    let f = Fixture::new();
    let root = f.temp.path().join("roots-project");
    write(&root, "a.md", BODY);
    write(&root, "docs/doc.md", BODY);
    write(&root, "mdrg.toml", "roots = ['docs']\n");
    link(&root, "docs/doc.md", "link.md");
    link(&root, "../a.md", "docs/link.md");
    link(&root, "cycle-b.md", "cycle-a.md");
    link(&root, "cycle-a.md", "cycle-b.md");
    f.pair_at(&root, "link.md:1-3", "docs/doc.md:1-3", &[], None, 0);
    f.pair_at(
        &root,
        "docs/link.md:1-3",
        "a.md:1-3",
        &[],
        Some("not_found"),
        1,
    );
    for name in ["cycle-a.md:1-3", "cycle-b.md:1-3"] {
        let result = f.pair_at(&root, name, "missing.md:1-3", &[], Some("not_found"), 1);
        assert!(error(&result, "not_found", 1).get("next").is_none());
    }
}

#[test]
fn a8_range_empty_invalid_utf8_and_invalid_range_match_including_guidance() {
    let f = Fixture::new();
    f.pair("link.md:2-8", "a.md:2-8", &[], None, 0);
    for range in ["0-1", "4-8"] {
        let result = f.pair(
            &format!("link.md:{range}"),
            &format!("a.md:{range}"),
            &[],
            Some("out_of_range"),
            1,
        );
        assert_eq!(f.replay(&result).status.code(), Some(0));
    }
    f.pair(
        "hidden-link.md:4-8",
        ".h/doc.md:4-8",
        &["--hidden", "--no-ignore"],
        Some("out_of_range"),
        1,
    );
    for range in ["1-2", "1-3"] {
        f.pair(
            &format!("empty-link.md:{range}"),
            &format!("empty.md:{range}"),
            &[],
            Some("out_of_range"),
            1,
        );
    }
    let result = f.pair("bad-link.md:1-3", "bad.md:1-3", &[], Some("read_error"), 1);
    assert!(error(&result, "read_error", 1).get("next").is_none());
    f.pair("link.md:bad", "a.md:bad", &[], Some("invalid_range"), 2);
}

#[test]
fn a9_usage_log_keeps_requested_range_and_matches_found_and_exit() {
    let f = Fixture::new();
    for (range, code) in [("link.md:1-3", 0), ("hidden-link.md:1-3", 1)] {
        let mut command = f.command(&f.project);
        command.env_remove("MDRG_NO_USAGE_LOG").args(["get", range]);
        let result = output(command);
        assert_eq!(result.status.code(), Some(code));
        if code == 0 {
            assert_eq!(target_path(&result), "a.md");
        }
        let log = std::fs::read_to_string(f.temp.path().join("state/mdrg/usage.jsonl")).unwrap();
        let rows: Vec<Value> = log
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        assert_eq!(rows.len(), if code == 0 { 1 } else { 2 });
        let row = rows.last().unwrap();
        assert_eq!(row["args"]["range"], range);
        assert_eq!(row["result"]["found"], code == 0);
        assert_eq!(row["exit"], code);
    }
}

#[test]
fn a10_list_rank_and_exact_do_not_emit_link_aliases() {
    let f = Fixture::new();
    for flags in [vec![], vec!["--hidden", "--no-ignore"]] {
        for args in [
            vec!["list"],
            vec!["search", "--q", "test", "--top-k", "100"],
            vec!["search", "--mode", "exact", "--q", "test", "--top-k", "100"],
        ] {
            let mut command = f.command(&f.project);
            command.args(&args).args(&flags);
            let result = output(command);
            assert_eq!(result.status.code(), Some(0));
            if let Some(before) = std::env::var_os("MDRG_A10_BEFORE") {
                let mut old = Command::new(before);
                old.current_dir(&f.project)
                    .arg("--root")
                    .arg(&f.project)
                    .env("HOME", &f.home)
                    .env("XDG_STATE_HOME", f.temp.path().join("state"))
                    .env("MDRG_NO_USAGE_LOG", "1")
                    .args(&args)
                    .args(&flags);
                let old = output(old);
                assert_eq!(result.status.code(), old.status.code());
                assert_eq!(result.stdout, old.stdout);
                assert_eq!(result.stderr, old.stderr);
            }
            let rows: Vec<Value> = String::from_utf8(result.stdout)
                .unwrap()
                .lines()
                .map(|line| serde_json::from_str(line).unwrap())
                .collect();
            assert!(!rows.is_empty());
            for row in rows {
                assert!(
                    ["a.md", "docs/doc.md", ".h/doc.md", "ignored.md", "empty.md"]
                        .contains(&row["path"].as_str().unwrap()),
                    "{row}"
                );
            }
        }
    }
}
