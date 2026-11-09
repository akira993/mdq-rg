use crate::{commands, config::Config, usage_log};
use clap::{Parser, Subcommand};
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

pub const SCOPE_HELP: &str = "Scope defaults to the current working directory (cwd). Use --root DIR to select\n\
another directory; its mdrg.toml is used and result paths are relative to it.\n\
For search and list, --paths takes patterns relative to the selected root;\n\
patterns pointing outside it are rejected with an error.\n\n\
search, get and list write one JSON object per line (JSON Lines). When search\n\
or list has no results, stdout is empty; stderr explains the cause and next\n\
actions.";

#[derive(Parser)]
#[command(name = "mdrg", version, about = "Search and read Markdown documents", after_help = SCOPE_HELP)]
pub struct Cli {
    /// Select the working directory (default: cwd).
    #[arg(long, global = true, value_name = "DIR")]
    pub root: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Search Markdown documents.
    Search(commands::search::Args),
    /// Read an original line range.
    Get(commands::get::Args),
    /// List Markdown sections.
    List(commands::list::Args),
}

pub(crate) fn root_too_broad(root: &Path) -> bool {
    std::env::var_os("HOME")
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .filter(|home| home.is_absolute())
        .and_then(|home| home.canonicalize().ok())
        .zip(root.canonicalize().ok())
        .is_some_and(|(home, root)| home.starts_with(root))
}

fn paths_outside_root(path: &Path) -> bool {
    if path.is_absolute() {
        return !std::env::current_dir().is_ok_and(|root| {
            path.ancestors()
                .find_map(|ancestor| {
                    ancestor
                        .canonicalize()
                        .ok()
                        .map(|resolved| (ancestor, resolved))
                })
                .is_some_and(|(ancestor, mut resolved)| {
                    for part in path.strip_prefix(ancestor).unwrap().components() {
                        match part {
                            Component::ParentDir => {
                                resolved.pop();
                            }
                            Component::CurDir => {}
                            _ => resolved.push(part.as_os_str()),
                        }
                    }
                    resolved.starts_with(root)
                })
        });
    }
    let mut depth = 0;
    for part in path.components() {
        match part {
            Component::ParentDir if depth == 0 => return true,
            Component::ParentDir => depth -= 1,
            Component::Normal(_) => depth += 1,
            _ => {}
        }
    }
    false
}

pub fn main() {
    let cli = Cli::parse();
    let start = Instant::now();
    if let Some(root) = &cli.root
        && let Err(error) = std::env::set_current_dir(root)
    {
        crate::search::output::report_stderr(format_args!(
            "{{\"error\":{{\"code\":\"invalid_root\",\"detail\":{},\"next\":[\"Specify an existing directory with --root DIR and retry.\"]}}}}",
            crate::search::output::json_string(&format!(
                "Cannot use --root {}: {error}",
                root.display()
            ))
        ));
        std::process::exit(2);
    }
    if std::env::current_dir().is_ok_and(|root| root_too_broad(&root)) {
        let (command, mut outcome, next) = match &cli.command {
            Command::Search(args) => (
                "search",
                commands::search::initial_outcome(args),
                "Run from a project directory or select a directory below HOME with --root DIR."
                    .to_owned(),
            ),
            Command::Get(args) => (
                "get",
                commands::get::initial_outcome(args),
                commands::get::root_next(args),
            ),
            Command::List(args) => (
                "list",
                commands::list::initial_outcome(args),
                "Run from a project directory or select a directory below HOME with --root DIR."
                    .to_owned(),
            ),
        };
        crate::search::output::report_stderr(format_args!(
            "{{\"error\":{{\"code\":\"root_too_broad\",\"detail\":\"The selected root is HOME or an ancestor of HOME.\",\"next\":[{}]}}}}",
            crate::search::output::json_string(&next)
        ));
        outcome.exit = 2;
        usage_log::record(
            &Config::default(),
            command,
            &outcome,
            start.elapsed().as_secs_f64() * 1000.0,
        );
        std::process::exit(2);
    }
    let paths = match &cli.command {
        Command::Search(args) => &args.paths[..],
        Command::List(args) => &args.paths[..],
        Command::Get(_) => &[],
    };
    for pattern in paths {
        let path = Path::new(pattern);
        if paths_outside_root(path) {
            crate::search::output::report_stderr(format_args!(
                "{{\"error\":{{\"code\":\"paths_outside_root\",\"detail\":{},\"next\":[\"Select a common parent with --root DIR and retry using relative --paths patterns.\"]}}}}",
                crate::search::output::json_string(&format!(
                    "The --paths pattern {pattern:?} points outside the selected root."
                ))
            ));
            std::process::exit(2);
        }
    }
    let config = match Config::load(Path::new(".")) {
        Ok(config) => config,
        Err(error) => {
            crate::search::output::report_stderr(format_args!("{error}"));
            std::process::exit(2);
        }
    };
    let (command, outcome) = match cli.command {
        Command::Search(args) => ("search", commands::search::run(&args, &config)),
        Command::Get(args) => (
            "get",
            commands::get::run(&args, &config, cli.root.as_deref()),
        ),
        Command::List(args) => ("list", commands::list::run(&args, &config)),
    };
    usage_log::record(
        &config,
        command,
        &outcome,
        start.elapsed().as_secs_f64() * 1000.0,
    );
    std::process::exit(outcome.exit);
}
