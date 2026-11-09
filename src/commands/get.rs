use crate::config::Config;
use crate::search::output::{json_string, report_stderr};
use crate::usage_log::{LogValue, Outcome};
use crate::{markdown, walk};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};

#[derive(clap::Args)]
#[command(after_help = crate::cli::SCOPE_HELP)]
pub struct Args {
    #[arg(value_name = "PATH:START-END")]
    pub range: String,
    #[arg(long)]
    pub hidden: bool,
    #[arg(long)]
    pub no_ignore: bool,
}

fn normalize(path: &Path) -> PathBuf {
    let mut result = PathBuf::new();
    for part in path.components() {
        match part {
            Component::CurDir => {}
            Component::ParentDir => {
                result.pop();
            }
            _ => result.push(part.as_os_str()),
        }
    }
    result
}

pub(crate) fn resolve(candidate: &Path) -> PathBuf {
    // Resolve existing ancestors too, including missing paths below symlinked directories.
    let mut ancestor = candidate;
    let mut suffix = Vec::new();
    loop {
        if let Ok(mut resolved) = ancestor.canonicalize() {
            for part in suffix.iter().rev() {
                resolved.push(part);
            }
            return normalize(&resolved);
        }
        if let (Some(parent), Some(part)) = (ancestor.parent(), ancestor.file_name()) {
            suffix.push(part.to_owned());
            ancestor = parent;
        } else {
            return normalize(candidate);
        }
    }
}

fn command(args: &Args, range: &str, root: Option<&Path>, hidden: bool) -> String {
    let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
    let mut command = "mdrg".to_owned();
    if let Some(root) = root {
        command.push_str(&format!(" --root {}", quote(&root.to_string_lossy())));
    }
    command.push_str(&format!(" get {}", quote(range)));
    if args.hidden || hidden {
        command.push_str(" --hidden");
    }
    if args.no_ignore {
        command.push_str(" --no-ignore");
    }
    command
}

pub(crate) fn root_next(args: &Args) -> String {
    let (name, range) = args
        .range
        .rsplit_once(':')
        .map_or((args.range.as_str(), None), |(name, range)| {
            (name, Some(range))
        });
    let root = std::env::current_dir().unwrap_or_default();
    let resolved = resolve(&root.join(name));
    parent_next(args, &resolved, range)
}

fn parent_next(args: &Args, resolved: &Path, range: Option<&str>) -> String {
    let parent = resolved.parent().unwrap_or(resolved);
    if walk::forbidden(resolved) {
        "This path contains a permanently excluded name.".to_owned()
    } else if crate::cli::root_too_broad(parent) {
        "The requested document is not read with the current scope. Use a plain text reader such as cat or sed to read it.".to_owned()
    } else {
        let range = range.map_or_else(
            || resolved.to_string_lossy().into_owned(),
            |range| format!("{}:{range}", resolved.display()),
        );
        command(args, &range, Some(parent), false)
    }
}

fn error(outcome: &mut Outcome, code: &str, detail: &str, exit: i32) {
    report_stderr(format_args!(
        "{{\"error\":{{\"code\":{},\"detail\":{}}}}}",
        json_string(code),
        json_string(detail)
    ));
    outcome.exit = exit;
}

fn error_with_next(outcome: &mut Outcome, code: &str, detail: &str, next: &str) {
    report_stderr(format_args!(
        "{{\"error\":{{\"code\":{},\"detail\":{},\"next\":[{}]}}}}",
        json_string(code),
        json_string(detail),
        json_string(next)
    ));
    outcome.exit = 1;
}

pub(crate) fn initial_outcome(args: &Args) -> Outcome {
    Outcome {
        exit: 0,
        args: vec![
            ("range", LogValue::Str(args.range.clone())),
            ("hidden", LogValue::Bool(args.hidden)),
            ("no_ignore", LogValue::Bool(args.no_ignore)),
        ],
        result: vec![("found", LogValue::Bool(false))],
    }
}

pub fn run(args: &Args, config: &Config, original_root: Option<&Path>) -> Outcome {
    let mut outcome = initial_outcome(args);
    let parsed = args.range.rsplit_once(':').and_then(|(path, range)| {
        let (start, end) = range.split_once('-')?;
        if path.is_empty()
            || start.is_empty()
            || end.is_empty()
            || !start.bytes().all(|b| b.is_ascii_digit())
            || !end.bytes().all(|b| b.is_ascii_digit())
        {
            return None;
        }
        let start = start.parse::<usize>().ok()?;
        let end = end.parse::<usize>().ok()?;
        (start <= end).then_some((path, start, end))
    });
    let Some((name, start, end)) = parsed else {
        error(
            &mut outcome,
            "invalid_range",
            "Expected PATH:START-END with START <= END.",
            2,
        );
        return outcome;
    };
    let Ok(root) = std::env::current_dir().and_then(|p| p.canonicalize()) else {
        error(
            &mut outcome,
            "read_error",
            "Cannot resolve the working directory.",
            1,
        );
        return outcome;
    };
    let candidate = root.join(name);
    let resolved = resolve(&candidate);
    if !resolved.starts_with(&root) {
        let next = parent_next(
            args,
            &resolved,
            Some(args.range.rsplit_once(':').unwrap().1),
        );
        error_with_next(
            &mut outcome,
            "outside_root",
            "The requested path is outside the selected root.",
            &next,
        );
        return outcome;
    }
    let lexical = normalize(&candidate);
    let relative = lexical
        .strip_prefix(&root)
        .unwrap_or_else(|_| resolved.strip_prefix(&root).unwrap())
        .to_string_lossy()
        .replace('\\', "/");
    let options = walk::WalkOptions {
        quiet: true,
        hidden: args.hidden || config.hidden,
        no_ignore: args.no_ignore,
        roots: config.roots.clone(),
        exclude: config.exclude.clone(),
        ..Default::default()
    };
    let Some((path, file)) = walk::collect(&root, &options)
        .into_iter()
        .find(|(path, _)| *path == relative)
    else {
        if lexical.is_file()
            && lexical
                .extension()
                .is_none_or(|extension| extension != "md")
            && !walk::forbidden(Path::new(&relative))
            && !walk::forbidden(resolved.strip_prefix(&root).unwrap())
            && !config
                .exclude
                .iter()
                .any(|pattern| walk::fnmatch(pattern, &relative))
            && !config.exclude.iter().any(|pattern| {
                walk::fnmatch(
                    pattern,
                    &resolved.strip_prefix(&root).unwrap().to_string_lossy(),
                )
            })
            && (if config.roots.is_empty() {
                vec![root.clone()]
            } else {
                config
                    .roots
                    .iter()
                    .filter_map(|name| root.join(name).canonicalize().ok())
                    .collect()
            })
            .iter()
            .any(|start| resolved.starts_with(start))
        {
            error_with_next(
                &mut outcome,
                "unsupported_type",
                "Only Markdown (.md) documents are supported.",
                "Use a plain text reader such as cat or sed to read this file.",
            );
            return outcome;
        }
        if !options.hidden
            && !walk::collect(
                &root,
                &walk::WalkOptions {
                    no_ignore: true,
                    ..options.clone()
                },
            )
            .iter()
            .any(|(path, _)| *path == relative)
            && walk::collect(
                &root,
                &walk::WalkOptions {
                    hidden: true,
                    no_ignore: true,
                    ..options.clone()
                },
            )
            .iter()
            .any(|(path, _)| *path == relative)
        {
            error_with_next(
                &mut outcome,
                "hidden",
                "The requested document is excluded because its path is hidden.",
                &command(args, &args.range, original_root, true),
            );
            return outcome;
        }
        if !args.no_ignore
            && walk::collect(
                &root,
                &walk::WalkOptions {
                    no_ignore: true,
                    ..options
                },
            )
            .iter()
            .any(|(path, _)| *path == relative)
        {
            error_with_next(
                &mut outcome,
                "ignored",
                "The requested document is excluded by an ignore rule.",
                "Check whether the exclusion is intentional before changing the ignore rules.",
            );
            return outcome;
        }
        error(
            &mut outcome,
            "not_found",
            "The requested document is not in the selected documents.",
            1,
        );
        return outcome;
    };
    let text = match walk::read_document(&file) {
        Ok(text) => text,
        Err(_) => {
            error(
                &mut outcome,
                "read_error",
                "Cannot read the requested document as text.",
                1,
            );
            return outcome;
        }
    };
    let lines: Vec<_> = text.split_inclusive('\n').collect();
    if start == 0 || start > lines.len() {
        let (detail, next) = if lines.is_empty() {
            (
                "The requested document is empty (0 lines).".to_owned(),
                "There are no lines to read in this document.".to_owned(),
            )
        } else {
            (
                format!(
                    "The requested lines are outside the document ({} lines).",
                    lines.len()
                ),
                command(
                    args,
                    &format!("{name}:1-{}", lines.len()),
                    original_root,
                    false,
                ),
            )
        };
        error_with_next(&mut outcome, "out_of_range", &detail, &next);
        return outcome;
    }
    let end = end.min(lines.len());
    let sections = markdown::split(&path, &text);
    let heading = sections
        .iter()
        .find(|s| s.start_line <= start && start <= s.end_line)
        .map_or("(preface)", |s| s.heading_path.as_str());
    if let Err(error) = writeln!(
        io::stdout().lock(),
        "{{\"path\":{},\"lines\":[{},{}],\"heading_path\":{},\"text\":{}}}",
        json_string(&path),
        start,
        end,
        json_string(heading),
        json_string(&lines[start - 1..end].concat())
    ) {
        outcome.exit = super::output_error(error);
    }
    outcome.result = vec![("found", LogValue::Bool(true))];
    outcome
}
