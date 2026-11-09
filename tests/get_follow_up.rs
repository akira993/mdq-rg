use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BODY: &str = "# Test\ntest second\nthird\n";
const EXCLUDED: &str = "This path contains a permanently excluded name.";
const HOME_NEXT: &str = "The requested document is not read with the current scope. Use a plain text reader such as cat or sed to read it.";

struct Fixture {
    temp: tempfile::TempDir,
    project: PathBuf,
    home: PathBuf,
}

fn write(root: &Path, name: &str, text: &str) {
    let path = root.join(name);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        let home = temp.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        for name in [
            "a.md",
            ".h/doc.md",
            ".h/ignored.md",
            ".h/excluded.md",
            ".h/plain.txt",
            ".h/excluded.txt",
            "excluded/doc.md",
            "excluded/plain.txt",
        ] {
            write(&project, name, BODY);
        }
        write(&project, "empty.md", "");
        write(&project, ".gitignore", ".h/ignored.md\n");
        write(
            &project,
            "mdrg.toml",
            "exclude = ['.h/excluded.*', 'excluded/**']\n",
        );
        assert!(
            Command::new("git")
                .args(["init", "-q"])
                .current_dir(&project)
                .env("HOME", &home)
                .env("XDG_STATE_HOME", temp.path().join("state"))
                .status()
                .unwrap()
                .success()
        );
        write(&project, ".git/x.md", BODY);
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            for (target, link) in [
                ("excluded/doc.md", "alias.md"),
                ("excluded/plain.txt", "alias.txt"),
                ("../a.md", ".h/link.md"),
                (".h/doc.md", "hidden-link.md"),
            ] {
                symlink(target, project.join(link)).unwrap();
            }
        }
        Self {
            temp,
            project,
            home,
        }
    }

    fn base_command(&self) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_mdrg"));
        cmd.current_dir(&self.project)
            .env("HOME", &self.home)
            .env("XDG_STATE_HOME", self.temp.path().join("state"))
            .env("MDRG_NO_USAGE_LOG", "1");
        cmd
    }

    fn command(&self, root: &Path) -> Command {
        let mut cmd = self.base_command();
        cmd.arg("--root").arg(root);
        cmd
    }

    fn run_original(&self, root: Option<&str>, args: &[&str]) -> Output {
        let mut cmd = self.base_command();
        if let Some(root) = root {
            cmd.arg("--root").arg(root);
        }
        cmd.args(args).output().unwrap()
    }

    fn run(&self, root: &Path, args: &[&str]) -> Output {
        self.command(root).args(args).output().unwrap()
    }

    fn replay(&self, next: &str) -> Output {
        Command::new("sh")
            .args(["-c", next])
            .current_dir(&self.project)
            .env("HOME", &self.home)
            .env("XDG_STATE_HOME", self.temp.path().join("state"))
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
}

fn error(output: &Output, code: &str, exit: i32) -> Value {
    assert_eq!(output.status.code(), Some(exit), "{output:?}");
    assert!(output.stdout.is_empty(), "{output:?}");
    let row: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(row["error"]["code"], code, "{row}");
    row["error"].clone()
}

fn success(output: &Output, lines: [usize; 2]) -> Value {
    assert_eq!(output.status.code(), Some(0), "{output:?}");
    assert!(output.stderr.is_empty(), "{output:?}");
    assert_eq!(output.stdout.iter().filter(|b| **b == b'\n').count(), 1);
    let row: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(row["lines"], json!(lines));
    row
}

#[test]
fn a1_clamps_end_and_logs_found_without_extra_fields() {
    let f = Fixture::new();
    let output = f
        .command(&f.project)
        .env_remove("MDRG_NO_USAGE_LOG")
        .args(["get", "a.md:2-8"])
        .output()
        .unwrap();
    let row = success(&output, [2, 3]);
    assert_eq!(
        row,
        json!({"path":"a.md", "lines":[2,3], "heading_path":"Test", "text":"test second\nthird\n"})
    );
    let log: Value = serde_json::from_str(
        &std::fs::read_to_string(f.temp.path().join("state/mdrg/usage.jsonl")).unwrap(),
    )
    .unwrap();
    assert_eq!(log["result"], json!({"found":true}));
    assert_eq!(log["exit"], 0);
}

#[test]
fn a2_out_of_range_reports_length_and_replay_preserves_flags() {
    let f = Fixture::new();
    for args in [
        vec!["get", "a.md:0-1"],
        vec!["get", "a.md:4-8"],
        vec!["get", ".h/doc.md:4-8", "--hidden", "--no-ignore"],
    ] {
        let err = error(&f.run(&f.project, &args), "out_of_range", 1);
        assert!(err["detail"].as_str().unwrap().contains("3 lines"));
        let next = err["next"][0].as_str().unwrap();
        for flag in args.iter().skip(2) {
            assert!(next.contains(flag));
        }
        assert_eq!(success(&f.replay(next), [1, 3])["text"], BODY);
    }
    let err = error(
        &f.run(&f.project, &["get", "empty.md:1-2"]),
        "out_of_range",
        1,
    );
    assert_eq!(err["detail"], "The requested document is empty (0 lines).");
    assert_eq!(
        err["next"],
        json!(["There are no lines to read in this document."])
    );
    for root in [None, Some("../project/.")] {
        let err = error(
            &f.run_original(root, &["get", "./a.md:4-8", "--no-ignore"]),
            "out_of_range",
            1,
        );
        let prefix = if root.is_some() {
            "mdrg --root '../project/.'"
        } else {
            "mdrg"
        };
        assert_eq!(
            err["next"],
            json!([format!("{prefix} get './a.md:1-3' --no-ignore")])
        );
        success(&f.replay(err["next"][0].as_str().unwrap()), [1, 3]);
    }
}

#[test]
fn a3_hidden_replays_and_configuration_enables_hidden() {
    let f = Fixture::new();
    for args in [
        vec!["get", ".h/doc.md:1-3"],
        vec!["get", ".h/doc.md:1-3", "--no-ignore"],
    ] {
        let err = error(&f.run(&f.project, &args), "hidden", 1);
        assert!(err["detail"].as_str().unwrap().contains("hidden"));
        let next = err["next"][0].as_str().unwrap();
        assert!(next.contains("--hidden"));
        if args.len() > 2 {
            assert!(next.contains("--no-ignore"));
        }
        assert_eq!(success(&f.replay(next), [1, 3])["text"], BODY);
    }
    for root in [None, Some("../project/.")] {
        let err = error(
            &f.run_original(root, &["get", "./.h/doc.md:0001-0003", "--no-ignore"]),
            "hidden",
            1,
        );
        let prefix = if root.is_some() {
            "mdrg --root '../project/.'"
        } else {
            "mdrg"
        };
        assert_eq!(
            err["next"],
            json!([format!(
                "{prefix} get './.h/doc.md:0001-0003' --hidden --no-ignore"
            )])
        );
        success(&f.replay(err["next"][0].as_str().unwrap()), [1, 3]);
    }
    let configured = Fixture::new();
    write(
        &configured.project,
        "mdrg.toml",
        "exclude = ['.h/excluded.*', 'excluded/**']\nhidden = true\n",
    );
    success(
        &configured.run(&configured.project, &["get", ".h/doc.md:1-3"]),
        [1, 3],
    );
}

#[test]
fn a4_hidden_precedes_ignored_and_does_not_suggest_unignoring() {
    let f = Fixture::new();
    let err = error(
        &f.run(&f.project, &["get", ".h/ignored.md:1-3"]),
        "hidden",
        1,
    );
    let err = error(&f.replay(err["next"][0].as_str().unwrap()), "ignored", 1);
    assert_eq!(
        err["next"],
        json!(["Check whether the exclusion is intentional before changing the ignore rules."])
    );
}

#[test]
fn a5_and_a9_excluded_missing_and_symlink_paths_do_not_disclose_reasons() {
    let f = Fixture::new();
    let absent = error(
        &f.run(&f.project, &["get", ".h/missing.md:1-3"]),
        "not_found",
        1,
    );
    for name in [
        ".h/excluded.md",
        ".h/excluded.txt",
        ".git/x.md",
        "excluded/doc.md",
        "excluded/plain.txt",
    ] {
        let actual = error(
            &f.run(&f.project, &["get", &format!("{name}:1-3")]),
            "not_found",
            1,
        );
        assert_eq!(actual, absent);
    }
    #[cfg(unix)]
    for name in [".h/link.md", "hidden-link.md", "alias.md", "alias.txt"] {
        for flags in [vec![], vec!["--hidden"], vec!["--hidden", "--no-ignore"]] {
            let range = format!("{name}:1-3");
            let mut args = vec!["get", &range];
            args.extend(flags);
            assert_eq!(error(&f.run(&f.project, &args), "not_found", 1), absent);
        }
    }
    let err = error(
        &f.run(&f.project, &["get", ".h/plain.txt:1-3"]),
        "unsupported_type",
        1,
    );
    assert!(err["detail"].as_str().unwrap().contains("Markdown"));
    assert!(
        err["next"][0]
            .as_str()
            .unwrap()
            .contains("plain text reader")
    );
}

#[test]
fn a6_rejects_home_and_ancestors_before_configuration_or_walking() {
    let f = Fixture::new();
    write(&f.home, "notes.md", BODY);
    write(&f.home, "project/a.md", BODY);
    write(&f.home, "project/node_modules/x.md", BODY);
    write(&f.home, "mdrg.toml", "this is invalid TOML");
    write(f.temp.path(), "notes.md", BODY);
    let mut roots = vec![f.home.clone(), f.temp.path().to_owned()];
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        for (name, target) in [
            ("home-link", &f.home),
            ("temp-link", &f.temp.path().to_owned()),
        ] {
            let link = f.temp.path().join(name);
            symlink(target, &link).unwrap();
            roots.push(link);
        }
    }
    for root in &roots {
        for args in [
            vec!["search", "--q", "test"],
            vec!["get", "project/a.md:1-3", "--hidden", "--no-ignore"],
            vec!["list"],
        ] {
            let absolute = format!("{}:1-3", f.home.join("project/a.md").display());
            let mut args = args;
            if args[0] == "get" {
                args[1] = &absolute;
            }
            let err = error(&f.run(root, &args), "root_too_broad", 2);
            if args[0] == "get" {
                let next = err["next"][0].as_str().unwrap();
                assert!(next.contains("--hidden") && next.contains("--no-ignore"));
                success(&f.replay(next), [1, 3]);
            } else {
                assert!(err["next"][0].as_str().unwrap().contains("below HOME"));
            }
        }
    }
    for home in [
        format!("{}/", f.home.display()),
        f.temp.path().join("home-link").display().to_string(),
    ] {
        if Path::new(&home).exists() {
            error(
                &f.command(&f.home)
                    .env("HOME", home)
                    .arg("list")
                    .output()
                    .unwrap(),
                "root_too_broad",
                2,
            );
        }
    }
    let output = Command::new(env!("CARGO_BIN_EXE_mdrg"))
        .current_dir(&f.home)
        .env("HOME", &f.home)
        .env("XDG_STATE_HOME", f.temp.path().join("state"))
        .env("MDRG_NO_USAGE_LOG", "1")
        .arg("list")
        .output()
        .unwrap();
    error(&output, "root_too_broad", 2);
    let missing = f.temp.path().join("missing");
    let output = f.run(&missing, &["list", "--bad"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("--bad"));
    error(&f.run(&missing, &["list"]), "invalid_root", 2);
    error(
        &f.run(&f.home, &["get", "missing.txt:bad"]),
        "root_too_broad",
        2,
    );
    error(
        &f.run(&f.home, &["list", "--paths", "../../*"]),
        "root_too_broad",
        2,
    );
    let output = f
        .command(&f.home)
        .env_remove("MDRG_NO_USAGE_LOG")
        .arg("list")
        .output()
        .unwrap();
    error(&output, "root_too_broad", 2);
    let log = std::fs::read_to_string(f.temp.path().join("state/mdrg/usage.jsonl")).unwrap();
    assert_eq!(log.lines().count(), 1);
    let row: Value = serde_json::from_str(&log).unwrap();
    assert_eq!(row["exit"], 2);
    assert_eq!(row["command"], "list");
    assert_eq!(
        row["args"],
        json!({"paths":"", "hidden":false, "no_ignore":false})
    );
    assert_eq!(row["result"], json!({}));
}

#[cfg(unix)]
#[test]
fn a6_unreadable_directory_is_not_visited() {
    use std::os::unix::fs::PermissionsExt;
    let f = Fixture::new();
    if Command::new("id")
        .arg("-u")
        .env("HOME", &f.home)
        .env("XDG_STATE_HOME", f.temp.path().join("state"))
        .output()
        .unwrap()
        .stdout
        == b"0\n"
    {
        use std::io::Write;
        std::io::stderr()
            .lock()
            .write_all(b"SKIP: permission-000 check requires a non-root user.\n")
            .unwrap();
        return;
    }
    let blocked = f.home.join("blocked");
    std::fs::create_dir(&blocked).unwrap();
    struct Restore(PathBuf);
    impl Drop for Restore {
        fn drop(&mut self) {
            std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
    }
    let _restore = Restore(blocked.clone());
    std::fs::set_permissions(&blocked, std::fs::Permissions::from_mode(0o0)).unwrap();
    assert!(std::fs::read_dir(&blocked).is_err());
    for args in [
        vec!["search", "--q", "test"],
        vec!["get", "missing.md:1-3"],
        vec!["list"],
    ] {
        error(&f.run(&f.home, &args), "root_too_broad", 2);
    }
}

#[test]
fn a7_home_advice_descendants_and_invalid_home_values() {
    let f = Fixture::new();
    write(&f.home, "notes.md", BODY);
    write(f.temp.path(), "notes.md", BODY);
    write(&f.home, "project/a.md", BODY);
    write(&f.home, "project/node_modules/x.md", BODY);
    for root in [&f.home, f.temp.path()] {
        let err = error(&f.run(root, &["get", "notes.md:1-3"]), "root_too_broad", 2);
        assert_eq!(err["next"], json!([HOME_NEXT]));
    }
    let err = error(
        &f.run(&f.home, &["get", "project/node_modules/x.md:1-3"]),
        "root_too_broad",
        2,
    );
    assert_eq!(err["next"], json!([EXCLUDED]));
    let root = f.home.join("project");
    success(&f.run(&root, &["get", "a.md:1-3"]), [1, 3]);
    for args in [vec!["search", "--q", "test"], vec!["list"]] {
        assert!(f.run(&root, &args).status.success());
    }
    let missing_home = f.temp.path().join("missing-home");
    for home in [None, Some(""), Some("rel"), missing_home.to_str()] {
        let mut cmd = f.command(f.temp.path());
        if let Some(home) = home {
            cmd.env("HOME", home);
        } else {
            cmd.env_remove("HOME");
        }
        success(
            &cmd.args(["get", "notes.md:1-3", "--no-ignore"])
                .output()
                .unwrap(),
            [1, 3],
        );
    }
    let bad = error(
        &f.run(&f.home, &["get", "project/a.md:bad"]),
        "root_too_broad",
        2,
    );
    let next = bad["next"][0].as_str().unwrap();
    assert!(next.ends_with("a.md:bad'"));
    error(&f.replay(next), "invalid_range", 2);
}

#[test]
fn root_too_broad_get_without_colon_does_not_add_one() {
    let f = Fixture::new();
    write(&f.home, "project/a.md", BODY);
    let err = error(
        &f.run(&f.home, &["get", "project/a.md"]),
        "root_too_broad",
        2,
    );
    let parent = f.home.join("project").canonicalize().unwrap();
    assert_eq!(
        err["next"],
        json!([format!(
            "mdrg --root '{}' get '{}'",
            parent.display(),
            parent.join("a.md").display()
        )])
    );
    error(
        &f.replay(err["next"][0].as_str().unwrap()),
        "invalid_range",
        2,
    );
}

#[test]
fn a7_existing_relative_home_does_not_restrict_scope() {
    let f = Fixture::new();
    write(f.temp.path(), "notes.md", BODY);
    for home in ["home", "."] {
        assert!(f.temp.path().join(home).is_dir());
        success(
            &f.base_command()
                .current_dir(f.temp.path())
                .env("HOME", home)
                .args(["get", "notes.md:1-3", "--no-ignore"])
                .output()
                .unwrap(),
            [1, 3],
        );
    }
}

#[test]
fn a8_permanently_excluded_outside_advice_is_true_for_a_selected_root() {
    let f = Fixture::new();
    let outside = f.temp.path().join("out/node_modules");
    write(&outside, "x.md", BODY);
    let range = format!("{}:1-3", outside.join("x.md").display());
    for home in [&f.home, &outside] {
        let output = f
            .command(&f.project)
            .env("HOME", home)
            .args(["get", &range])
            .output()
            .unwrap();
        assert_eq!(error(&output, "outside_root", 1)["next"], json!([EXCLUDED]));
    }
    success(&f.run(&outside, &["get", "x.md:1-3"]), [1, 3]);
    success(
        &Command::new(env!("CARGO_BIN_EXE_mdrg"))
            .current_dir(&outside)
            .env("HOME", &f.home)
            .env("XDG_STATE_HOME", f.temp.path().join("state"))
            .env("MDRG_NO_USAGE_LOG", "1")
            .args(["get", "x.md:1-3"])
            .output()
            .unwrap(),
        [1, 3],
    );
}

#[test]
fn a10_absolute_paths_keep_zero_results_with_relative_pattern_advice() {
    let f = Fixture::new();
    let pattern = format!("{}/*.md", f.project.display());
    for args in [
        vec!["search", "--q", "test", "--paths", &pattern],
        vec!["list", "--paths", &pattern],
    ] {
        let output = f.run(&f.project, &args);
        assert_eq!(output.status.code(), Some(0));
        assert!(output.stdout.is_empty());
        let row: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(row["no_results"]["cause"], "paths_matched_nothing");
        let next = row["no_results"]["next"].to_string();
        assert!(next.contains("relative") && next.contains("*.md"), "{row}");
    }
}

#[test]
fn a10_mixed_paths_advice_preserves_relative_patterns_and_order() {
    let f = Fixture::new();
    let absolute = format!("{}/missing-absolute/*.md", f.project.display());
    for paths in [
        [absolute.as_str(), "./missing-relative/*.md"],
        ["./missing-relative/*.md", absolute.as_str()],
    ] {
        for mut args in [vec!["search", "--q", "test"], vec!["list"]] {
            args.push("--paths");
            args.extend(paths);
            let output = f.run(&f.project, &args);
            assert_eq!(output.status.code(), Some(0));
            assert!(output.stdout.is_empty());
            let row: Value = serde_json::from_slice(&output.stderr).unwrap();
            assert_eq!(row["no_results"]["cause"], "paths_matched_nothing");
            let converted = if paths[0] == absolute {
                "'missing-absolute/*.md' './missing-relative/*.md'"
            } else {
                "'./missing-relative/*.md' 'missing-absolute/*.md'"
            };
            assert_eq!(
                row["no_results"]["next"][0],
                format!("Use --paths patterns relative to the selected root: --paths {converted}.")
            );
        }
    }
}

#[test]
fn a13_outside_advice_does_not_select_home() {
    let f = Fixture::new();
    write(&f.home, "notes.md", BODY);
    write(&f.home, "project/a.md", BODY);
    let range = format!("{}:1-3", f.home.join("notes.md").display());
    assert_eq!(
        error(
            &f.run(&f.home.join("project"), &["get", &range]),
            "outside_root",
            1
        )["next"],
        json!([HOME_NEXT])
    );
}
