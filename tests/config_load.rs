use mdq_rg::config::Config;

#[test]
fn missing_and_empty_configuration_use_defaults() {
    let dir = tempfile::tempdir().unwrap();
    for source in [None, Some("")] {
        if let Some(source) = source {
            std::fs::write(dir.path().join("mdrg.toml"), source).unwrap();
        }
        let config = Config::load(dir.path()).unwrap();
        assert!(config.usage_log);
        assert!(!config.hidden);
        assert!(config.roots.is_empty());
        assert!(config.exclude.is_empty());
    }
}

#[test]
fn all_settings_are_loaded() {
    let dir = tempfile::tempdir().unwrap();
    for name in ["docs", ".claude"] {
        std::fs::create_dir(dir.path().join(name)).unwrap();
    }
    std::fs::write(dir.path().join("mdrg.toml"),
        "hidden = true\nusage_log = false\nroots = [\"docs\", \".claude\"]\nexclude = [\"docs/private/*.md\"]\n"
    ).unwrap();
    let config = Config::load(dir.path()).unwrap();
    assert!(!config.usage_log);
    assert!(config.hidden);
    assert_eq!(config.roots, ["docs", ".claude"]);
    assert_eq!(config.exclude, ["docs/private/*.md"]);
}

#[test]
fn unknown_keys_bad_types_and_bad_toml_are_errors() {
    let dir = tempfile::tempdir().unwrap();
    for source in [
        "unknown = true",
        "hidden = 1",
        "no_ignore = 'true'",
        "usage_log = []",
        "roots = 'docs'",
        "roots = [1]",
        "exclude = [true]",
        "[",
    ] {
        std::fs::write(dir.path().join("mdrg.toml"), source).unwrap();
        let error = Config::load(dir.path()).unwrap_err();
        assert!(error.contains("mdrg.toml"), "{source}: {error}");
    }
    std::fs::write(dir.path().join("mdrg.toml"), [0xff]).unwrap();
    assert!(Config::load(dir.path()).is_err());
}

#[test]
fn invalid_configuration_exits_two_before_running_each_command() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("mdrg.toml"), "unknown = true").unwrap();
    for args in [
        vec!["search", "--q", "x"],
        vec!["get", "a.md:1-2"],
        vec!["list"],
    ] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_mdrg"))
            .env("MDRG_NO_USAGE_LOG", "1")
            .current_dir(dir.path())
            .args(args)
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains("TOML parse error"));
    }
}

#[test]
fn removed_no_ignore_setting_is_an_unknown_key_for_both_booleans() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.md"), "needle\n").unwrap();
    for value in [true, false] {
        std::fs::write(
            dir.path().join("mdrg.toml"),
            format!("no_ignore = {value}\n"),
        )
        .unwrap();
        let error = Config::load(dir.path()).unwrap_err();
        assert_eq!(
            error,
            format!(
                "{}: TOML parse error at line 1, column 1",
                dir.path().join("mdrg.toml").display()
            )
        );
        for args in [
            vec!["search", "--q", "needle", "--no-ignore"],
            vec!["get", "a.md:1-1", "--no-ignore"],
            vec!["list", "--no-ignore"],
        ] {
            let output = std::process::Command::new(env!("CARGO_BIN_EXE_mdrg"))
                .env("MDRG_NO_USAGE_LOG", "1")
                .current_dir(dir.path())
                .args(&args)
                .output()
                .unwrap();
            assert_eq!(
                output.status.code(),
                Some(2),
                "{value}, {args:?}: {output:?}"
            );
            assert!(output.stdout.is_empty());
            assert_eq!(
                output.stderr,
                b"./mdrg.toml: TOML parse error at line 1, column 1\n"
            );
        }
    }
}

#[test]
fn configuration_errors_hide_source_and_values_but_keep_location() {
    let dir = tempfile::tempdir().unwrap();
    for (source, location) in [
        (
            "# comment\nFAKE_SECRET_LINE: abc123\nsecond line\n",
            "line 2, column 19",
        ),
        ("FAKE_SECRET_KEY = true\n", "line 1, column 1"),
        ("hidden = 'FAKE_SECRET_VALUE'\n", "line 1, column 10"),
        (
            "exclude = ['FAKE_SECRET_DUPLICATE']\nexclude = []\n",
            "line 2, column 1",
        ),
    ] {
        std::fs::write(dir.path().join("mdrg.toml"), source).unwrap();
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_mdrg"))
            .env("MDRG_NO_USAGE_LOG", "1")
            .current_dir(dir.path())
            .args(["search", "--q", "needle"])
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let stderr = String::from_utf8(output.stderr).unwrap();
        assert_eq!(
            stderr,
            format!("./mdrg.toml: TOML parse error at {location}\n")
        );
        assert!(!stderr.contains("FAKE_SECRET"), "{stderr}");
    }
}

#[cfg(unix)]
#[test]
fn linked_configuration_error_hides_outside_file_contents() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let target = outside.path().join("private.txt");
    std::fs::write(&target, "FAKE_SECRET_LINE: abc123\nsecond line\n").unwrap();
    std::os::unix::fs::symlink(&target, dir.path().join("mdrg.toml")).unwrap();
    std::fs::write(dir.path().join("a.md"), "# T\nneedle\n").unwrap();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mdrg"))
        .env("MDRG_NO_USAGE_LOG", "1")
        .current_dir(dir.path())
        .args(["search", "--q", "needle"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!stderr.contains("FAKE_SECRET_LINE"), "{stderr}");
    assert!(!stderr.contains("abc123"), "{stderr}");
    assert!(!stderr.contains("second line"), "{stderr}");
    assert_eq!(
        stderr,
        "./mdrg.toml: TOML parse error at line 1, column 19\n"
    );

    std::fs::write(&target, "hidden = true\n").unwrap();
    assert!(Config::load(dir.path()).unwrap().hidden);
}

#[test]
fn removed_analysis_settings_are_unknown_keys_and_exit_two() {
    let dir = tempfile::tempdir().unwrap();
    for source in [
        "ja_analysis = false\n",
        "ja_analysis = true\n",
        "lang = 'auto'\n",
        "lang = 'ja'\n",
    ] {
        std::fs::write(dir.path().join("mdrg.toml"), source).unwrap();
        for args in [
            vec!["search", "--q", "x"],
            vec!["get", "a.md:1-2"],
            vec!["list"],
        ] {
            let output = std::process::Command::new(env!("CARGO_BIN_EXE_mdrg"))
                .env("MDRG_NO_USAGE_LOG", "1")
                .current_dir(dir.path())
                .args(args)
                .output()
                .unwrap();
            assert_eq!(output.status.code(), Some(2), "{source}");
            assert!(output.stdout.is_empty());
            assert_eq!(
                String::from_utf8(output.stderr).unwrap(),
                "./mdrg.toml: TOML parse error at line 1, column 1\n"
            );
        }
    }
}
