#[derive(Clone, Debug)]
pub struct Section {
    pub path: String,
    pub start_line: usize,
    pub end_line: usize,
    pub heading_path: String,
    pub text: String,
    pub level: usize,
}

pub fn split(path: &str, text: &str) -> Vec<Section> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = text.lines().collect();
    let mut begin = 0;
    if lines.first().is_some_and(|s| s.trim_end() == "---")
        && let Some(end) = lines.iter().skip(1).position(|s| s.trim_end() == "---")
        && lines[1..end + 1]
            .iter()
            .map(|line| line.trim())
            .find(|line| !line.is_empty())
            .is_some_and(|line| {
                line.split_once(':').is_some_and(|(key, _)| {
                    !key.is_empty()
                        && key
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                })
            })
    {
        begin = end + 2;
    }
    let mut sections = Vec::new();
    let mut headings: Vec<(usize, String)> = Vec::new();
    let mut label = "(preface)".to_string();
    let mut start = begin;
    let mut fence: Option<(char, usize)> = None;
    for i in begin..lines.len() {
        let line = lines[i];
        let indent = line.len() - line.trim_start_matches(' ').len();
        if indent > 3 {
            continue;
        }
        let trimmed = line.trim_start_matches(' ');
        let first = trimmed.chars().next().unwrap_or(' ');
        let count = trimmed.chars().take_while(|c| *c == first).count();
        if let Some((marker, length)) = fence {
            if first == marker && count >= length && trimmed[count..].trim().is_empty() {
                fence = None;
            }
            continue;
        }
        if (first == '`' || first == '~')
            && count >= 3
            && (first != '`' || !trimmed[count..].contains('`'))
        {
            fence = Some((first, count));
            continue;
        }
        if first == '#'
            && count <= 6
            && (trimmed.len() == count || trimmed.as_bytes()[count].is_ascii_whitespace())
        {
            if i > start
                && (!headings.is_empty()
                    || lines[start..i].iter().any(|line| !line.trim().is_empty()))
            {
                sections.push(Section {
                    path: path.into(),
                    start_line: start + 1,
                    end_line: i,
                    heading_path: label,
                    text: lines[start..i].join("\n"),
                    level: headings.last().map_or(0, |(level, _)| *level),
                });
            }
            let title = trimmed[count..].trim();
            let without_hashes = title.trim_end_matches('#');
            let title = if without_hashes.ends_with(char::is_whitespace) {
                without_hashes.trim_end()
            } else {
                title
            };
            while headings.last().is_some_and(|(level, _)| *level >= count) {
                headings.pop();
            }
            headings.push((count, title.to_string()));
            label = headings
                .iter()
                .map(|(_, title)| title.as_str())
                .collect::<Vec<_>>()
                .join(" > ");
            start = i;
        }
    }
    if start < lines.len()
        && (!headings.is_empty() || lines[start..].iter().any(|line| !line.trim().is_empty()))
    {
        sections.push(Section {
            path: path.into(),
            start_line: start + 1,
            end_line: lines.len(),
            heading_path: label,
            text: lines[start..].join("\n"),
            level: headings.last().map_or(0, |(level, _)| *level),
        });
    }
    sections
}

#[cfg(test)]
mod tests {
    use super::split;

    fn layout(text: &str) -> Vec<(usize, usize, String, usize)> {
        split("sample.md", text)
            .into_iter()
            .map(|s| (s.start_line, s.end_line, s.heading_path, s.level))
            .collect()
    }

    #[test]
    fn markdown_backtick_info_cannot_contain_backticks() {
        assert_eq!(
            layout("```a```\n# Visible\nbody"),
            vec![(1, 1, "(preface)".into(), 0), (2, 3, "Visible".into(), 1)]
        );
    }

    #[test]
    fn markdown_tilde_info_can_contain_backticks() {
        assert_eq!(
            layout("~~~a```\n# Hidden\n~~~\n# Visible"),
            vec![(1, 3, "(preface)".into(), 0), (4, 4, "Visible".into(), 1)]
        );
    }

    #[test]
    fn markdown_bom_before_heading_preserves_lines() {
        let sections = split("sample.md", "\u{feff}# Title\nbody\n## Child\ntext");
        assert_eq!(sections[0].heading_path, "Title");
        assert_eq!(sections[0].text, "# Title\nbody");
        assert_eq!(sections[0].path, "sample.md");
        assert_eq!(
            layout("\u{feff}# Title\nbody\n## Child\ntext"),
            vec![(1, 2, "Title".into(), 1), (3, 4, "Title > Child".into(), 2)]
        );
        assert_eq!(
            split("sample.md", "text\n\u{feff}# Body")[0].text,
            "text\n\u{feff}# Body"
        );
    }

    #[test]
    fn markdown_bom_before_front_matter_preserves_lines() {
        assert_eq!(
            layout("\u{feff}---\ntitle: example\n---\n# Title\nbody"),
            vec![(4, 5, "Title".into(), 1)]
        );
        assert!(split("sample.md", "\u{feff}").is_empty());
    }

    #[test]
    fn markdown_front_matter_requires_first_nonempty_key() {
        for body in [
            "prose",
            "",
            "\n \t",
            "not a key: value",
            ": value",
            "é: value",
            "prose\nkey: value",
        ] {
            let text = format!("---\n{body}\n---\n# Title");
            let sections = split("sample.md", &text);
            assert_eq!(sections.len(), 2, "body: {body:?}");
            assert_eq!(sections[0].text, format!("---\n{body}\n---"));
            assert_eq!(sections[0].start_line, 1);
            assert_eq!(sections[1].start_line, text.lines().count());
        }
    }

    #[test]
    fn markdown_front_matter_accepts_keys_without_parsing_values() {
        for body in [
            "date: 2026-11-02",
            "title: [broken",
            "\n \t\n  a_1-b: arbitrary: [",
            "0:",
        ] {
            let text = format!("---\n{body}\n---\n# Title");
            assert_eq!(
                layout(&text),
                vec![(
                    text.lines().count(),
                    text.lines().count(),
                    "Title".into(),
                    1
                )]
            );
        }
        assert!(split("sample.md", "---\nkey: value\n---").is_empty());
    }

    #[test]
    fn markdown_unclosed_front_matter_remains_body() {
        let sections = split("sample.md", "---\nkey: value\n# Title");
        assert_eq!(sections[0].text, "---\nkey: value");
        assert_eq!(sections[1].start_line, 3);
    }

    #[test]
    fn markdown_unusual_inputs_do_not_panic() {
        let long = "x".repeat(1_000_000);
        for text in [
            "",
            " \t\n  ",
            "# Title\r\nbody\r\n",
            &long,
            "#",
            "```\n# Hidden",
            "ééé\n\u{65e5}\u{672c}\u{8a9e}\n😀",
            "\u{feff}",
        ] {
            assert!(std::panic::catch_unwind(|| split("sample.md", text)).is_ok());
        }
        assert!(split("sample.md", "").is_empty());
        assert_eq!(layout("#"), vec![(1, 1, "".into(), 1)]);
        assert_eq!(layout("```\n# Hidden"), vec![(1, 2, "(preface)".into(), 0)]);
        assert_eq!(
            layout("# Title\r\nbody\r\n"),
            vec![(1, 2, "Title".into(), 1)]
        );
    }

    #[test]
    fn markdown_seeded_random_inputs_do_not_panic() {
        let alphabet = [
            '#', '`', '~', '-', ':', ' ', '\t', '\r', '\n', 'a', '0', '_', 'é', '\u{65e5}', '😀',
            '\u{feff}', '\0',
        ];
        let mut seed = 0x5eed_u64;
        for case in 0..2000 {
            let mut text = String::new();
            for _ in 0..case % 257 {
                seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
                text.push(alphabet[(seed >> 32) as usize % alphabet.len()]);
            }
            assert!(
                std::panic::catch_unwind(|| split("sample.md", &text)).is_ok(),
                "case {case}"
            );
        }
    }
}
