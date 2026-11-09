use crate::config::Config;
use crate::search::output::{json_string, report_stderr};
use std::fmt::Write as _;
use std::fs::{DirBuilder, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(unix)]
use std::fs::{File, Metadata};
#[cfg(unix)]
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
#[cfg(unix)]
use std::path::Path;

#[cfg(all(
    target_os = "macos",
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
// O_NONBLOCK | O_NOFOLLOW: https://github.com/apple-oss-distributions/xnu/blob/main/bsd/sys/fcntl.h
const LOG_OPEN_FLAGS: i32 = 0x0004 | 0x0100;
#[cfg(all(target_os = "linux", target_arch = "x86_64"))]
// O_NONBLOCK | O_NOFOLLOW: https://github.com/torvalds/linux/blob/master/include/uapi/asm-generic/fcntl.h
const LOG_OPEN_FLAGS: i32 = (1 << 11) | (1 << 17);
#[cfg(all(target_os = "linux", target_arch = "aarch64"))]
// O_NONBLOCK: include/uapi/asm-generic/fcntl.h; O_NOFOLLOW: https://github.com/torvalds/linux/blob/master/arch/arm64/include/uapi/asm/fcntl.h
const LOG_OPEN_FLAGS: i32 = (1 << 11) | (1 << 15);
#[cfg(all(
    unix,
    not(any(
        all(
            target_os = "macos",
            any(target_arch = "aarch64", target_arch = "x86_64")
        ),
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))
))]
const LOG_OPEN_FLAGS: i32 = 0;

pub enum LogValue {
    Str(String),
    Int(i64),
    Bool(bool),
}

pub struct Outcome {
    pub exit: i32,
    pub args: Vec<(&'static str, LogValue)>,
    pub result: Vec<(&'static str, LogValue)>,
}

pub fn record(config: &Config, command: &str, outcome: &Outcome, elapsed_ms: f64) {
    if !config.usage_log || std::env::var_os("MDRG_NO_USAGE_LOG").is_some_and(|v| v == "1") {
        return;
    }
    if let Err(error) = append(command, outcome, elapsed_ms) {
        report_stderr(format_args!(
            "{{\"warning\":{{\"code\":\"usage_log_failed\",\"detail\":{}}}}}",
            json_string(&error.to_string())
        ));
    }
}

fn append(command: &str, outcome: &Outcome, elapsed_ms: f64) -> io::Result<()> {
    let state = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|path| path.is_absolute())
                .map(|home| home.join(".local/state"))
        })
        .ok_or_else(|| {
            io::Error::new(io::ErrorKind::NotFound, "No state directory is available")
        })?;
    let cwd = std::env::current_dir()?;
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_secs();
    let line = format!(
        "{{\"ts\":{ts},\"version\":{},\"command\":{},\"cwd\":{},\"args\":{},\"result\":{},\"elapsed_ms\":{},\"exit\":{}}}\n",
        json_string(env!("CARGO_PKG_VERSION")),
        json_string(command),
        json_string(&cwd.to_string_lossy()),
        object(&outcome.args),
        object(&outcome.result),
        number(elapsed_ms),
        outcome.exit
    );
    let directory = state.join("mdrg");
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    builder.mode(0o700);
    builder.create(&directory)?;
    let path = directory.join("usage.jsonl");
    #[cfg(unix)]
    let mut file = {
        let directory_metadata = std::fs::symlink_metadata(&directory)?;
        if !directory_metadata.is_dir() {
            return Err(io::Error::other("Usage log directory is not a directory"));
        }
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if !metadata.is_file() || metadata.nlink() != 1 => {
                return Err(io::Error::other(
                    "Usage log is not a single-link regular file",
                ));
            }
            Err(error) if error.kind() != io::ErrorKind::NotFound => return Err(error),
            _ => {}
        }
        let file = match open_log(&path, false) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::NotFound => match open_log(&path, true) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    open_log(&path, false)?
                }
                Err(error) => return Err(error),
            },
            Err(error) => return Err(error),
        };
        validate_opened_log(&file, &path, &directory, &directory_metadata)?;
        file
    };
    #[cfg(not(unix))]
    let mut file = OpenOptions::new().append(true).create(true).open(&path)?;
    if file.write(line.as_bytes())? != line.len() {
        return Err(io::Error::new(
            io::ErrorKind::WriteZero,
            "Incomplete usage log write",
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn open_log(path: &Path, create_new: bool) -> io::Result<File> {
    OpenOptions::new()
        .append(true)
        .mode(0o600)
        .custom_flags(LOG_OPEN_FLAGS)
        .create_new(create_new)
        .open(path)
}

#[cfg(unix)]
fn validate_opened_log(
    file: &File,
    path: &Path,
    directory: &Path,
    directory_metadata: &Metadata,
) -> io::Result<()> {
    let opened = file.metadata()?;
    let current = std::fs::symlink_metadata(path)?;
    let current_directory = std::fs::symlink_metadata(directory)?;
    if !opened.is_file()
        || !current.is_file()
        || opened.nlink() != 1
        || current.nlink() != 1
        || opened.dev() != current.dev()
        || opened.ino() != current.ino()
        || !current_directory.is_dir()
        || directory_metadata.dev() != current_directory.dev()
        || directory_metadata.ino() != current_directory.ino()
    {
        return Err(io::Error::other(
            "Usage log path changed or is not a single-link regular file",
        ));
    }
    Ok(())
}

fn number(value: f64) -> String {
    if value.is_finite() {
        value.to_string()
    } else {
        "null".into()
    }
}

fn object(values: &[(&str, LogValue)]) -> String {
    let mut output = String::from("{");
    for (index, (key, value)) in values.iter().enumerate() {
        if index != 0 {
            output.push(',');
        }
        let value = match value {
            LogValue::Str(value) => json_string(value),
            LogValue::Int(value) => value.to_string(),
            LogValue::Bool(value) => value.to_string(),
        };
        write!(output, "{}:{value}", json_string(key)).unwrap();
    }
    output.push('}');
    output
}

#[cfg(all(test, unix))]
#[path = "../tests/usage_log/open.rs"]
mod tests;
