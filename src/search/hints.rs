use super::output::{json_string, report_stderr};
use std::collections::BTreeMap;

pub fn report(
    cause: &str,
    words: Option<&BTreeMap<String, usize>>,
    query: &str,
    paths: &[String],
    hidden: bool,
    no_ignore: bool,
) {
    let (detail, next) = match cause {
        "no_documents" => (
            "No readable Markdown documents found.",
            vec![
                "Check the current working directory.",
                "mdrg search --q <TEXT> --hidden",
                "Check the ignore rules for the target documents.",
            ],
        ),
        "paths_matched_nothing" => (
            "No documents matched --paths.",
            vec!["Check the --paths patterns.", "mdrg search --q <TEXT>"],
        ),
        "empty_query" => (
            "The query contains no searchable terms.",
            vec!["mdrg search --q <WORD>"],
        ),
        _ if words.is_some() => (
            "No section contains the query.",
            vec![
                "Check the per-word section counts in words.",
                "mdrg search --q <TEXT> --mode rank",
            ],
        ),
        _ => (
            "No section matches the query terms.",
            vec![
                "Try a different word.",
                "mdrg search --q <TEXT> --mode exact",
            ],
        ),
    };
    let quoted = shell_quote(query);
    let next = next
        .into_iter()
        .map(|command| {
            json_string(&scoped_command(
                &command
                    .replace("<TEXT>", &quoted)
                    .replace("<WORD>", "'word'"),
                cause,
                paths,
                hidden,
                no_ignore,
            ))
        })
        .collect::<Vec<_>>()
        .join(",");
    let words = words
        .map(|words| {
            format!(
                ",\"words\":{{{}}}",
                words
                    .iter()
                    .map(|(word, count)| format!("{}:{count}", json_string(word)))
                    .collect::<Vec<_>>()
                    .join(",")
            )
        })
        .unwrap_or_default();
    report_stderr(format_args!(
        "{{\"no_results\":{{\"cause\":{},\"detail\":{},\"next\":[{}]{}}}}}",
        json_string(cause),
        json_string(detail),
        next,
        words
    ));
}

pub fn scoped_command(
    command: &str,
    cause: &str,
    paths: &[String],
    hidden: bool,
    no_ignore: bool,
) -> String {
    if cause == "paths_matched_nothing"
        && command == "Check the --paths patterns."
        && paths
            .iter()
            .any(|pattern| std::path::Path::new(pattern).is_absolute())
    {
        let relative: Vec<_> = paths
            .iter()
            .filter_map(|pattern| {
                let path = std::path::Path::new(pattern);
                if path.is_absolute() {
                    let root = std::env::current_dir().ok()?;
                    crate::commands::get::resolve(path)
                        .strip_prefix(root)
                        .ok()
                        .map(|p| shell_quote(&p.to_string_lossy()))
                } else {
                    Some(shell_quote(pattern))
                }
            })
            .collect();
        if !relative.is_empty() {
            return format!(
                "Use --paths patterns relative to the selected root: --paths {}.",
                relative.join(" ")
            );
        }
    }
    let mut command = command.to_owned();
    if !command.starts_with("mdrg ") {
        return command;
    }
    if cause == "no_documents" {
        if no_ignore {
            command.push_str(" --no-ignore");
        }
        return command;
    }
    if cause != "paths_matched_nothing" && !paths.is_empty() {
        command.push_str(" --paths");
        for path in paths {
            command.push(' ');
            command.push_str(&shell_quote(path));
        }
    }
    if hidden {
        command.push_str(" --hidden");
    }
    if no_ignore {
        command.push_str(" --no-ignore");
    }
    command
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}
