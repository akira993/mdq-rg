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

pub fn run(args: &Args, config: &Config) -> Outcome {
    let mut outcome = Outcome {
        exit: 0,
        args: vec![
            ("range", LogValue::Str(args.range.clone())),
            ("hidden", LogValue::Bool(args.hidden)),
            ("no_ignore", LogValue::Bool(args.no_ignore)),
        ],
        result: vec![("found", LogValue::Bool(false))],
    };
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
    // Resolve the closest existing ancestor as well, so missing outside paths
    // and paths below symlinked directories receive the same boundary check.
    let mut ancestor = candidate.as_path();
    let mut suffix = Vec::new();
    let resolved = loop {
        if let Ok(mut resolved) = ancestor.canonicalize() {
            for part in suffix.iter().rev() {
                resolved.push(part);
            }
            break normalize(&resolved);
        }
        if let (Some(parent), Some(part)) = (ancestor.parent(), ancestor.file_name()) {
            suffix.push(part.to_owned());
            ancestor = parent;
        } else {
            break normalize(&candidate);
        }
    };
    if !resolved.starts_with(&root) {
        let next = if walk::forbidden(&resolved) {
            "This path is inside a permanently excluded directory; mdrg does not read it."
                .to_owned()
        } else {
            let parent = resolved.parent().unwrap_or(&resolved);
            let range = format!("{}:{start}-{end}", resolved.display());
            let quote = |value: &str| format!("'{}'", value.replace('\'', "'\\''"));
            let mut command = format!(
                "mdrg --root {} get {}",
                quote(&parent.to_string_lossy()),
                quote(&range)
            );
            if args.hidden {
                command.push_str(" --hidden");
            }
            if args.no_ignore {
                command.push_str(" --no-ignore");
            }
            command
        };
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
            .any(|start| {
                resolved.strip_prefix(start).is_ok_and(|tail| {
                    args.hidden
                        || config.hidden
                        || !tail
                            .components()
                            .any(|part| part.as_os_str().to_string_lossy().starts_with('.'))
                })
            })
        {
            error_with_next(
                &mut outcome,
                "unsupported_type",
                "Only Markdown (.md) documents are supported.",
                "Use a plain text reader such as cat or sed to read this file.",
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
    if start == 0 || end > lines.len() {
        error(
            &mut outcome,
            "out_of_range",
            "The requested lines are outside the document.",
            1,
        );
        return outcome;
    }
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
