use super::Document;
use super::excerpt::excerpt;
use crate::walk::SkipReason;

pub fn report_stderr(message: std::fmt::Arguments<'_>) {
    use std::io::Write;
    let _ = writeln!(std::io::stderr().lock(), "{message}");
}

pub fn report_skipped(path: &str, reason: SkipReason) {
    let reason = match reason {
        SkipReason::InvalidUtf8 => "invalid_utf8",
        SkipReason::ContainsNul => "contains_nul",
        SkipReason::ReadError => "read_error",
    };
    report_stderr(format_args!(
        "{{\"skipped\":{{\"path\":{},\"reason\":{}}}}}",
        json_string(path),
        json_string(reason)
    ));
}

pub fn json_string(text: &str) -> String {
    let mut result = String::from("\"");
    for c in text.chars() {
        match c {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            '\u{00}'..='\u{1f}' => {
                use std::fmt::Write;
                write!(result, "\\u{:04x}", c as u32).unwrap();
            }
            _ => result.push(c),
        }
    }
    result.push('"');
    result
}

pub fn line_tokens(line: &str) -> usize {
    let ascii = line.chars().filter(char::is_ascii).count();
    line.chars().count() - ascii + ascii.div_ceil(4)
}

pub fn output_lines(
    docs: &[Document],
    scores: &[(usize, f64)],
    query: &[String],
    top_k: usize,
    budget: usize,
) -> Vec<String> {
    let mut used = 0;
    let mut lines = Vec::new();
    for (i, score) in scores.iter().take(top_k) {
        let s = &docs[*i].section;
        let line = format!(
            "{{\"path\":{},\"lines\":[{},{}],\"heading_path\":{},\"score\":{},\"snippet\":{}}}",
            json_string(&s.path),
            s.start_line,
            s.end_line,
            json_string(&s.heading_path),
            score,
            json_string(&excerpt(s, query))
        );
        used += line_tokens(&line);
        if !lines.is_empty() && used > budget {
            break;
        }
        lines.push(line);
    }
    lines
}
