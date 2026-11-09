use mdq_rg::config::Config;
use std::process::Command;

#[test]
fn invalid_roots_fail_loading_and_exit_two() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(".git")).unwrap();
    std::fs::create_dir(dir.path().join("node_modules")).unwrap();
    std::fs::create_dir(dir.path().join("docs")).unwrap();
    for root in [
        "missing".to_owned(),
        "../".into(),
        ".git".into(),
        "node_modules".into(),
        "node_modules/../docs".into(),
        dir.path().display().to_string(),
    ] {
        std::fs::write(
            dir.path().join("mdrg.toml"),
            format!("roots = [{root:?}]\n"),
        )
        .unwrap();
        assert!(Config::load(dir.path()).is_err(), "{root}");
        let out = Command::new(env!("CARGO_BIN_EXE_mdrg"))
            .args(["list"])
            .current_dir(dir.path())
            .env("MDRG_NO_USAGE_LOG", "1")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{root}: {out:?}");
    }
}

#[test]
fn empty_root_is_invalid_and_dot_selects_working_directory() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.md"), "needle").unwrap();
    for (root, exit) in [("", 2), (".", 0)] {
        std::fs::write(
            dir.path().join("mdrg.toml"),
            format!("roots = [{root:?}]\n"),
        )
        .unwrap();
        assert_eq!(Config::load(dir.path()).is_ok(), exit == 0, "{root:?}");
        let out = Command::new(env!("CARGO_BIN_EXE_mdrg"))
            .arg("list")
            .current_dir(dir.path())
            .env("MDRG_NO_USAGE_LOG", "1")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(exit), "{root:?}: {out:?}");
        assert_eq!(!out.stdout.is_empty(), exit == 0);
    }
}

#[cfg(unix)]
#[test]
fn roots_reject_aliases_to_permanent_exclusions_with_exit_two() {
    let dir = tempfile::tempdir().unwrap();
    for target in [".git", "node_modules", ".ssh", ".aws", ".gnupg", ".env"] {
        std::fs::create_dir(dir.path().join(target)).unwrap();
        let alias = format!("alias-{target}");
        std::os::unix::fs::symlink(target, dir.path().join(&alias)).unwrap();
        std::fs::write(
            dir.path().join("mdrg.toml"),
            format!("roots = [{alias:?}]\n"),
        )
        .unwrap();
        assert!(Config::load(dir.path()).is_err(), "{target}");
        let out = Command::new(env!("CARGO_BIN_EXE_mdrg"))
            .arg("list")
            .current_dir(dir.path())
            .env("MDRG_NO_USAGE_LOG", "1")
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{target}: {out:?}");
        assert!(out.stdout.is_empty());
    }
}

#[cfg(unix)]
#[test]
fn roots_resolve_from_config_directory_and_reject_escape_alias() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), dir.path().join("escape")).unwrap();
    std::fs::write(dir.path().join("mdrg.toml"), "roots = ['escape']\n").unwrap();
    assert!(Config::load(dir.path()).is_err());
}

#[test]
fn explicit_root_includes_gitignored_directory_for_all_commands() {
    let dir = tempfile::tempdir().unwrap();
    let init = Command::new("git")
        .arg("init")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(init.status.success(), "{init:?}");
    std::fs::write(dir.path().join(".gitignore"), "build/\n").unwrap();
    std::fs::create_dir(dir.path().join("build")).unwrap();
    let body = "# Build\nneedle\n";
    std::fs::write(dir.path().join("build/b.md"), body).unwrap();

    for explicit_root in [false, true] {
        if explicit_root {
            std::fs::write(dir.path().join("mdrg.toml"), "roots = [\"build\"]\n").unwrap();
        }
        for args in [
            vec!["list"],
            vec!["search", "--q", "needle"],
            vec!["get", "build/b.md:1-2"],
        ] {
            let out = Command::new(env!("CARGO_BIN_EXE_mdrg"))
                .args(&args)
                .current_dir(dir.path())
                .env("MDRG_NO_USAGE_LOG", "1")
                .output()
                .unwrap();
            let stdout = String::from_utf8(out.stdout.clone()).unwrap();
            assert_eq!(
                stdout.contains("build/b.md"),
                explicit_root,
                "roots={explicit_root}, {args:?}: {out:?}"
            );
            if explicit_root {
                assert_eq!(out.status.code(), Some(0), "{args:?}: {out:?}");
                if args[0] == "get" {
                    let row: serde_json::Value = serde_json::from_str(&stdout).unwrap();
                    assert_eq!(row["text"], body);
                }
            } else if args[0] == "get" {
                assert_eq!(out.status.code(), Some(1), "{out:?}");
                assert!(stdout.is_empty(), "{out:?}");
            } else {
                assert_eq!(out.status.code(), Some(0), "{args:?}: {out:?}");
            }
        }
    }
}
