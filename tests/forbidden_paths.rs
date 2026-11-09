use mdq_rg::config::Config;
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

fn assert_permanent_exclusion(name: &str) {
    let root = tempfile::tempdir().unwrap();
    std::fs::write(root.path().join("kept.md"), "needle\n").unwrap();
    let mut excluded = Vec::new();
    let mut expected = vec!["kept.md".to_owned()];
    for prefix in ["", "docs/"] {
        for (directory, blocked) in [(name.to_owned(), true), (format!("{name}.local"), false)] {
            let path = format!("{prefix}{directory}/note.md");
            let file = root.path().join(&path);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(file, "needle\n").unwrap();
            if blocked {
                excluded.push(path);
            } else {
                expected.push(path);
            }
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;

        std::fs::create_dir(root.path().join("links")).unwrap();
        symlink(
            format!("../{name}.local"),
            root.path().join("links").join(name),
        )
        .unwrap();
        symlink(name, root.path().join("directory-alias")).unwrap();
        symlink(format!("{name}/note.md"), root.path().join("file-alias.md")).unwrap();
        excluded.extend([
            format!("links/{name}/note.md"),
            "directory-alias/note.md".to_owned(),
            "file-alias.md".to_owned(),
        ]);
    }
    expected.sort();
    for mut args in [
        vec!["search", "--q", "needle"],
        vec!["search", "--q", "needle", "--mode", "exact"],
        vec!["list"],
    ] {
        args.extend(["--hidden", "--no-ignore"]);
        let output = run(root.path(), &args);
        assert_eq!(
            output.status.code(),
            Some(0),
            "{name}, {args:?}: {output:?}"
        );
        assert!(output.stderr.is_empty(), "{output:?}");
        let mut paths: Vec<String> = String::from_utf8(output.stdout)
            .unwrap()
            .lines()
            .map(|line| {
                serde_json::from_str::<Value>(line).unwrap()["path"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect();
        paths.sort();
        assert_eq!(paths, expected, "{name}, {args:?}");
    }
    for path in &excluded {
        let spec = format!("{path}:1-1");
        let output = run(root.path(), &["get", &spec, "--hidden", "--no-ignore"]);
        assert_eq!(output.status.code(), Some(1), "{spec}: {output:?}");
        assert!(output.stdout.is_empty());
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"]["code"],
            "not_found"
        );
    }
    for path in &expected {
        let spec = format!("{path}:1-1");
        let output = run(root.path(), &["get", &spec, "--hidden", "--no-ignore"]);
        assert_eq!(output.status.code(), Some(0), "{spec}: {output:?}");
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stdout).unwrap()["text"],
            "needle\n"
        );
    }
    let named_roots = vec![
        (name.to_owned(), "invalid root"),
        (format!("docs/{name}"), "invalid root"),
        (excluded[0].clone(), "invalid root"),
        (format!("{name}/../docs"), "invalid root"),
    ];
    #[cfg(unix)]
    let named_roots = {
        let mut roots = named_roots;
        roots.extend([
            (format!("links/{name}"), "invalid root"),
            (
                "directory-alias".to_owned(),
                "outside the allowed directories",
            ),
            (
                "file-alias.md".to_owned(),
                "outside the allowed directories",
            ),
        ]);
        roots
    };
    for (named_root, expected_error) in named_roots {
        std::fs::write(
            root.path().join("mdrg.toml"),
            format!("roots = [{named_root:?}]\n"),
        )
        .unwrap();
        let error = Config::load(root.path()).unwrap_err();
        assert!(error.contains(expected_error), "{named_root}: {error}");
        for mut args in [
            vec!["search", "--q", "needle"],
            vec!["search", "--q", "needle", "--mode", "exact"],
            vec!["get", "kept.md:1-1"],
            vec!["list"],
        ] {
            args.extend(["--hidden", "--no-ignore"]);
            let output = run(root.path(), &args);
            assert_eq!(
                output.status.code(),
                Some(2),
                "{named_root}, {args:?}: {output:?}"
            );
            assert!(output.stdout.is_empty());
            assert!(
                String::from_utf8(output.stderr)
                    .unwrap()
                    .contains(expected_error)
            );
        }
    }
}

#[test]
fn ssh_paths_are_always_excluded() {
    assert_permanent_exclusion(".ssh");
}

#[test]
fn aws_paths_are_always_excluded() {
    assert_permanent_exclusion(".aws");
}

#[test]
fn gnupg_paths_are_always_excluded() {
    assert_permanent_exclusion(".gnupg");
}

#[test]
fn env_paths_are_always_excluded() {
    assert_permanent_exclusion(".env");
}

#[test]
fn uppercase_ssh_paths_are_always_excluded() {
    assert_permanent_exclusion(".SSH");
}

#[test]
fn mixed_case_ssh_paths_are_always_excluded() {
    assert_permanent_exclusion(".Ssh");
}

#[test]
fn uppercase_aws_paths_are_always_excluded() {
    assert_permanent_exclusion(".AWS");
}

#[test]
fn mixed_case_aws_paths_are_always_excluded() {
    assert_permanent_exclusion(".Aws");
}

#[test]
fn uppercase_gnupg_paths_are_always_excluded() {
    assert_permanent_exclusion(".GNUPG");
}

#[test]
fn mixed_case_gnupg_paths_are_always_excluded() {
    assert_permanent_exclusion(".Gnupg");
}

#[test]
fn uppercase_env_paths_are_always_excluded() {
    assert_permanent_exclusion(".ENV");
}

#[test]
fn mixed_case_env_paths_are_always_excluded() {
    assert_permanent_exclusion(".Env");
}

#[test]
fn uppercase_git_paths_are_always_excluded() {
    assert_permanent_exclusion(".GIT");
}

#[test]
fn mixed_case_git_paths_are_always_excluded() {
    assert_permanent_exclusion(".Git");
}

#[test]
fn uppercase_node_modules_paths_are_always_excluded() {
    assert_permanent_exclusion("NODE_MODULES");
}

#[test]
fn mixed_case_node_modules_paths_are_always_excluded() {
    assert_permanent_exclusion("Node_Modules");
}

#[test]
fn long_s_spellings_are_excluded_like_their_ascii_names() {
    for name in [
        ".\u{17f}sh",
        ".s\u{17f}h",
        ".\u{17f}\u{17f}h",
        ".aw\u{17f}",
        ".AW\u{17f}",
    ] {
        assert_permanent_exclusion(name);
    }
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("node_module\u{17f}");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join("note.md"), "needle\n").unwrap();
    let output = run(root.path(), &["list", "--hidden", "--no-ignore"]);
    assert!(output.stdout.is_empty(), "{output:?}");
}

#[test]
fn sharp_s_spellings_are_excluded_like_their_ascii_names() {
    for name in [".\u{df}h", ".\u{1e9e}h", ".\u{df}H", ".\u{1e9e}H"] {
        assert_permanent_exclusion(name);
    }
}

#[test]
fn other_case_mappings_stay_searchable() {
    for name in [".g\u{131}t", ".\u{131}env", "ſsh", "stra\u{df}e"] {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join(name);
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(directory.join("note.md"), "needle\n").unwrap();
        let output = run(root.path(), &["list", "--hidden"]);
        assert_eq!(output.status.code(), Some(0), "{name}: {output:?}");
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("note.md"),
            "{name}: {output:?}"
        );
    }
}
