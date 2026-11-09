use crate::config::Config;
use crate::search::output::report_stderr;
use crate::text::QueryTerms;
use crate::usage_log::{LogValue, Outcome};
use crate::walk::WalkOptions;
use rayon::prelude::*;
use std::io::{self, Write};
use std::time::Instant;

#[derive(Clone, Copy, clap::ValueEnum)]
pub enum Mode {
    Rank,
    Exact,
}

#[derive(clap::Args)]
#[command(after_help = format!("{}\n\n{}", crate::cli::SCOPE_HELP, "Both rank and exact return sections, limited by --top-k and the --max-tokens\noutput budget. For every matching line use rg -n -F 'text'; to count matching\nlines use rg -c -F 'text'.\n\nRead JSON Lines one line at a time:\n  mdrg search --q 'text' | python3 -c 'import sys,json; [print(json.loads(line)) for line in sys.stdin]'"))]
pub struct Args {
    #[arg(long, value_name = "TEXT", allow_hyphen_values = true)]
    pub q: String,
    #[arg(long, value_enum, default_value = "rank")]
    pub mode: Mode,
    #[arg(long, default_value_t = 5, value_name = "N", value_parser = clap::value_parser!(u64).range(1..))]
    pub top_k: u64,
    #[arg(long, default_value_t = 1300, value_name = "N")]
    pub max_tokens: usize,
    /// Patterns relative to the selected root; outside paths are rejected.
    #[arg(long, num_args = 1.., value_name = "GLOB")]
    pub paths: Vec<String>,
    #[arg(long)]
    pub hidden: bool,
    #[arg(long)]
    pub no_ignore: bool,
    #[arg(long)]
    pub timing: bool,
}

pub fn run(args: &Args, config: &Config) -> Outcome {
    let mut outcome = Outcome {
        exit: 0,
        args: vec![
            ("q", LogValue::Str(args.q.clone())),
            (
                "mode",
                LogValue::Str(
                    match args.mode {
                        Mode::Rank => "rank",
                        Mode::Exact => "exact",
                    }
                    .into(),
                ),
            ),
            ("top_k", LogValue::Int(args.top_k as i64)),
            ("max_tokens", LogValue::Int(args.max_tokens as i64)),
        ],
        result: Vec::new(),
    };
    match search(args, config) {
        Ok(result) => outcome.result = result,
        Err(error) => {
            report_stderr(format_args!("{error}"));
            outcome.exit = 1;
        }
    }
    outcome
}

fn search(args: &Args, config: &Config) -> io::Result<Vec<(&'static str, LogValue)>> {
    let total_start = Instant::now();
    let now = Instant::now();
    let (mut files, walk_skipped) = crate::walk::collect_with_skipped(
        std::path::Path::new("."),
        &WalkOptions {
            quiet: false,
            paths: Vec::new(),
            hidden: args.hidden || config.hidden,
            no_ignore: args.no_ignore,
            roots: config.roots.clone(),
            exclude: config.exclude.clone(),
        },
    );
    let available = files.len();
    if !args.paths.is_empty() {
        let paths: Vec<_> = args
            .paths
            .iter()
            .map(|p| crate::walk::Glob::new(p))
            .collect();
        files.retain(|(path, _)| paths.iter().any(|p| p.matches(path)));
    }
    let scan = now.elapsed();
    let now = Instant::now();
    let contents: Vec<_> = files
        .par_iter()
        .filter_map(|(relative, path)| match crate::walk::read_document(path) {
            Ok(text) => Some((relative, text)),
            Err(reason) => {
                crate::search::output::report_skipped(relative, reason);
                None
            }
        })
        .collect();
    let read = now.elapsed();
    let bytes: usize = contents.iter().map(|(_, text)| text.len()).sum();
    let skipped = files.len() - contents.len();
    let now = Instant::now();
    let sections: Vec<_> = contents
        .par_iter()
        .flat_map_iter(|(path, text)| crate::markdown::split(path, text))
        .collect();
    let split = now.elapsed();
    let now = Instant::now();
    let n = sections.len();
    let cause = if available == 0 {
        Some("no_documents")
    } else if files.is_empty() {
        Some("paths_matched_nothing")
    } else if contents.is_empty() {
        Some("no_documents")
    } else {
        None
    };
    let (lines, tokenize, rank, output_start) = if matches!(args.mode, Mode::Rank) {
        let query = QueryTerms::new(&args.q);
        let (docs, n, avgdl) = crate::search::tokenize(sections, &query.expanded);
        let tokenize = now.elapsed();
        let now = Instant::now();
        let scores = crate::search::rank::rank(&docs, &query.expanded, n, avgdl);
        let rank = now.elapsed();
        let output_start = Instant::now();
        let empty = query.expanded.is_empty();
        if let Some(cause) = cause.or(if empty {
            Some("empty_query")
        } else if scores.is_empty() {
            Some("no_match")
        } else {
            None
        }) {
            crate::search::hints::report(
                cause,
                None,
                &args.q,
                &args.paths,
                args.hidden,
                args.no_ignore,
            );
        }
        (
            crate::search::output::output_lines(
                &docs,
                &scores,
                &query.surface,
                args.top_k as usize,
                args.max_tokens,
            ),
            tokenize,
            rank,
            output_start,
        )
    } else {
        let query = crate::search::exact::ExactQuery::new(&args.q);
        let tokenize = now.elapsed();
        let now = Instant::now();
        let (hits, words) = crate::search::exact::search(&sections, &query);
        let rank = now.elapsed();
        let output_start = Instant::now();
        if let Some(cause) = cause.or(if query.is_empty() {
            Some("empty_query")
        } else if hits.is_empty() {
            Some("no_match")
        } else {
            None
        }) {
            crate::search::hints::report(
                cause,
                (cause == "no_match").then_some(&words),
                &args.q,
                &args.paths,
                args.hidden,
                args.no_ignore,
            );
        }
        (
            crate::search::exact::output_lines(
                &sections,
                &hits,
                &query,
                args.top_k as usize,
                args.max_tokens,
            ),
            tokenize,
            rank,
            output_start,
        )
    };
    let mut stdout = io::BufWriter::new(io::stdout().lock());
    let written = (|| -> io::Result<()> {
        for line in &lines {
            writeln!(stdout, "{line}")?;
        }
        stdout.flush()
    })();
    if let Err(error) = written {
        if error.kind() != io::ErrorKind::BrokenPipe {
            return Err(error);
        }
        // Discard buffered output after the consumer closes the pipe.
        let _ = stdout.into_parts();
    }
    let output = output_start.elapsed();
    if args.timing {
        let skipped = if skipped > 0 {
            format!(" skipped={skipped}")
        } else {
            String::new()
        };
        report_stderr(format_args!(
            "files={} bytes={} sections={n}{skipped} scan_ms={:.3} read_ms={:.3} split_ms={:.3} tokenize_ms={:.3} rank_ms={:.3} output_ms={:.3} total_ms={:.3}",
            contents.len(),
            bytes,
            scan.as_secs_f64() * 1000.0,
            read.as_secs_f64() * 1000.0,
            split.as_secs_f64() * 1000.0,
            tokenize.as_secs_f64() * 1000.0,
            rank.as_secs_f64() * 1000.0,
            output.as_secs_f64() * 1000.0,
            total_start.elapsed().as_secs_f64() * 1000.0
        ));
    }
    Ok(vec![
        ("hits", LogValue::Int(lines.len() as i64)),
        ("skipped", LogValue::Int((skipped + walk_skipped) as i64)),
    ])
}
