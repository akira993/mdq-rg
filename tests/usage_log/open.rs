use super::{open_log, validate_opened_log};
use std::fs::{self, File, Metadata};
use std::os::unix::fs::symlink;
use std::path::PathBuf;

struct Log {
    _state: tempfile::TempDir,
    directory: PathBuf,
    path: PathBuf,
    directory_metadata: Metadata,
    file: File,
}

impl Log {
    fn new() -> Self {
        let state = tempfile::tempdir().unwrap();
        let directory = state.path().join("mdrg");
        fs::create_dir(&directory).unwrap();
        let path = directory.join("usage.jsonl");
        fs::write(&path, "existing\n").unwrap();
        let file = open_log(&path, false).unwrap();
        let directory_metadata = fs::symlink_metadata(&directory).unwrap();
        Self {
            _state: state,
            directory,
            path,
            directory_metadata,
            file,
        }
    }

    fn validate(&self) -> std::io::Result<()> {
        validate_opened_log(
            &self.file,
            &self.path,
            &self.directory,
            &self.directory_metadata,
        )
    }
}

#[test]
fn opened_regular_file_still_matches() {
    let log = Log::new();
    log.validate().unwrap();
}

#[test]
fn opened_file_replaced_with_another_regular_file_is_rejected() {
    let log = Log::new();
    fs::rename(&log.path, log.directory.join("previous")).unwrap();
    fs::write(&log.path, "replacement\n").unwrap();
    assert!(log.validate().is_err());
    assert_eq!(fs::read(&log.path).unwrap(), b"replacement\n");
}

#[test]
fn opened_file_replaced_with_symlink_is_rejected() {
    let log = Log::new();
    let previous = log.directory.join("previous");
    fs::rename(&log.path, &previous).unwrap();
    symlink(&previous, &log.path).unwrap();
    assert!(log.validate().is_err());
    assert_eq!(fs::read(previous).unwrap(), b"existing\n");
}

#[test]
fn hard_link_added_after_open_is_rejected() {
    let log = Log::new();
    fs::hard_link(&log.path, log.directory.join("second")).unwrap();
    assert!(log.validate().is_err());
}

#[test]
fn directory_replaced_while_opened_file_stays_the_same_is_rejected() {
    let log = Log::new();
    let previous = log.directory.with_file_name("previous");
    fs::rename(&log.directory, &previous).unwrap();
    fs::create_dir(&log.directory).unwrap();
    fs::rename(previous.join("usage.jsonl"), &log.path).unwrap();
    assert!(log.validate().is_err());
}

#[test]
fn directory_replaced_with_symlink_to_original_is_rejected() {
    let log = Log::new();
    let previous = log.directory.with_file_name("previous");
    fs::rename(&log.directory, &previous).unwrap();
    symlink(&previous, &log.directory).unwrap();
    assert!(log.validate().is_err());
}

#[cfg(any(
    all(
        target_os = "macos",
        any(target_arch = "aarch64", target_arch = "x86_64")
    ),
    all(
        target_os = "linux",
        any(target_arch = "x86_64", target_arch = "aarch64")
    )
))]
mod flags {
    use super::*;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    #[test]
    fn open_refuses_existing_and_dangling_symlinks() {
        for existing in [true, false] {
            let state = tempfile::tempdir().unwrap();
            let target = state.path().join("target");
            let path = state.path().join("usage.jsonl");
            if existing {
                fs::write(&target, "untouched\n").unwrap();
            }
            symlink(&target, &path).unwrap();
            assert_eq!(
                open_log(&path, true).unwrap_err().kind(),
                std::io::ErrorKind::AlreadyExists
            );
            assert!(open_log(&path, false).is_err());
            if existing {
                assert_eq!(fs::read(target).unwrap(), b"untouched\n");
            } else {
                assert!(!target.exists());
            }
        }
    }

    #[test]
    fn fifo_open_does_not_wait_for_reader() {
        if let Some(path) = std::env::var_os("MDRG_TEST_FIFO") {
            let path = PathBuf::from(path);
            assert_eq!(
                open_log(&path, true).unwrap_err().kind(),
                std::io::ErrorKind::AlreadyExists
            );
            assert!(open_log(&path, false).is_err());
            return;
        }
        let state = tempfile::tempdir().unwrap();
        let path = state.path().join("usage.jsonl");
        assert!(
            Command::new("mkfifo")
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "usage_log::tests::flags::fifo_open_does_not_wait_for_reader",
            ])
            .env("MDRG_TEST_FIFO", &path)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        loop {
            if let Some(status) = child.try_wait().unwrap() {
                assert!(status.success(), "FIFO open failed: {status}");
                break;
            }
            if Instant::now() >= deadline {
                child.kill().unwrap();
                child.wait().unwrap();
                panic!("FIFO open waited for a reader");
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    fn opened_file_replaced_with_fifo_is_rejected() {
        let log = Log::new();
        fs::rename(&log.path, log.directory.join("previous")).unwrap();
        assert!(
            Command::new("mkfifo")
                .arg(&log.path)
                .status()
                .unwrap()
                .success()
        );
        assert!(log.validate().is_err());
    }
}
