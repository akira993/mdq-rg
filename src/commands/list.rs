use crate::config::Config;
use crate::search::output::{json_string, report_skipped, report_stderr};
use crate::usage_log::{LogValue, Outcome};
use crate::{markdown, walk};
use std::io::{self, Write};

#[derive(clap::Args)]
#[command(after_help = crate::cli::SCOPE_HELP)]
pub struct Args {
    /// Patterns relative to the selected root; outside paths are rejected.
    #[arg(long, num_args = 1.., value_name = "GLOB")]
    pub paths: Vec<String>,
    #[arg(long, value_name = "N")]
    pub heading_level: Option<usize>,
    #[arg(long, value_name = "N", value_parser = clap::value_parser!(u64).range(1..))]
    pub limit: Option<u64>,
    #[arg(long)]
    pub hidden: bool,
    #[arg(long)]
    pub no_ignore: bool,
}

pub fn run(args: &Args, config: &Config) -> Outcome {
    let mut outcome = Outcome {
        exit: 0,
        args: vec![
            ("paths", LogValue::Str(args.paths.join(","))),
            ("hidden", LogValue::Bool(args.hidden)),
            ("no_ignore", LogValue::Bool(args.no_ignore)),
        ],
        result: Vec::new(),
    };
    if let Some(level) = args.heading_level {
        outcome
            .args
            .push(("heading_level", LogValue::Int(level as i64)));
    }
    if let Some(limit) = args.limit {
        outcome.args.push(("limit", LogValue::Int(limit as i64)));
    }
    let Ok(root) = std::env::current_dir() else {
        report_stderr(format_args!(
            "{{\"error\":{{\"code\":\"read_error\",\"detail\":\"Cannot resolve the working directory.\"}}}}"
        ));
        outcome.exit = 1;
        return outcome;
    };
    let options = walk::WalkOptions {
        quiet: false,
        hidden: args.hidden || config.hidden,
        no_ignore: args.no_ignore,
        paths: args.paths.clone(),
        roots: config.roots.clone(),
        exclude: config.exclude.clone(),
    };
    let (files, mut skipped) = walk::collect_with_skipped(&root, &options);
    let empty_cause = if files.is_empty() {
        let cause = if !args.paths.is_empty()
            && !walk::collect(
                &root,
                &walk::WalkOptions {
                    quiet: true,
                    paths: Vec::new(),
                    ..options
                },
            )
            .is_empty()
        {
            "paths_matched_nothing"
        } else {
            "no_documents"
        };
        Some(cause)
    } else {
        None
    };
    let mut count = 0;
    let mut readable = 0;
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    let mut failed = false;
    'files: for (path, file) in files {
        if count >= args.limit.unwrap_or(u64::MAX) {
            break;
        }
        let text = match walk::read_document(&file) {
            Ok(text) => text,
            Err(reason) => {
                report_skipped(&path, reason);
                skipped += 1;
                continue;
            }
        };
        readable += 1;
        let mut sections = markdown::split(&path, &text);
        if sections.is_empty() {
            sections.push(markdown::Section {
                path: path.clone(),
                start_line: 0,
                end_line: 0,
                heading_path: "(preface)".into(),
                text: String::new(),
                level: 0,
            });
        }
        for section in sections {
            if count >= args.limit.unwrap_or(u64::MAX) {
                break;
            }
            if args
                .heading_level
                .is_some_and(|level| section.level > level)
            {
                continue;
            }
            if let Err(error) = writeln!(
                stdout,
                "{{\"path\":{},\"lines\":[{},{}],\"heading_path\":{},\"level\":{}}}",
                json_string(&path),
                section.start_line,
                section.end_line,
                json_string(&section.heading_path),
                section.level
            ) {
                outcome.exit = super::output_error(error);
                failed = true;
                break 'files;
            }
            count += 1;
        }
    }
    if count == 0 && !failed {
        let cause = empty_cause.unwrap_or(if readable == 0 {
            "no_documents"
        } else {
            "no_match"
        });
        let (detail, next) = match cause {
            "paths_matched_nothing" => (
                "No documents matched --paths.",
                vec!["Check the --paths patterns.", "mdrg list"],
            ),
            "no_match" => (
                "No sections matched --heading-level.",
                vec!["Check --heading-level.", "mdrg list"],
            ),
            _ => (
                "No readable Markdown documents found.",
                vec![
                    "Check the current working directory.",
                    "mdrg list --hidden",
                    "Check the ignore rules for the target documents.",
                ],
            ),
        };
        report_stderr(format_args!(
            "{{\"no_results\":{{\"cause\":{},\"detail\":{},\"next\":[{}]}}}}",
            json_string(cause),
            json_string(detail),
            next.into_iter()
                .map(|command| {
                    json_string(&crate::search::hints::scoped_command(
                        command,
                        cause,
                        &args.paths,
                        args.hidden,
                        args.no_ignore,
                    ))
                })
                .collect::<Vec<_>>()
                .join(",")
        ));
    }
    if !failed && let Err(error) = stdout.flush() {
        outcome.exit = super::output_error(error);
    }
    let _ = stdout.into_parts();
    outcome.result = vec![
        ("count", LogValue::Int(count as i64)),
        ("skipped", LogValue::Int(skipped as i64)),
    ];
    outcome
}
