use crate::markdown::Section;
use crate::text::fold_for_exact;
use std::collections::BTreeMap;

pub struct ExactQuery {
    pub phrase: String,
    pub words: Vec<String>,
}

impl ExactQuery {
    pub fn new(text: &str) -> Self {
        let phrase = fold_for_exact(text).trim().to_string();
        let words = phrase.split_whitespace().map(str::to_string).collect();
        Self { phrase, words }
    }

    pub fn is_empty(&self) -> bool {
        self.words.is_empty()
    }
}

pub struct Hit {
    pub index: usize,
    pub score: usize,
    pub kind: &'static str,
}

pub fn search(sections: &[Section], query: &ExactQuery) -> (Vec<Hit>, BTreeMap<String, usize>) {
    let mut words: BTreeMap<_, _> = query.words.iter().map(|w| (w.clone(), 0)).collect();
    let mut hits = Vec::new();
    if query.is_empty() {
        return (hits, words);
    }
    for (index, section) in sections.iter().enumerate() {
        let text = fold_for_exact(&section.text);
        let mut all_words = true;
        let mut word_score = 0;
        for (word, sections) in &mut words {
            let count = text.matches(word.as_str()).count();
            *sections += usize::from(count > 0);
            all_words &= count > 0;
            word_score += count * query.words.iter().filter(|w| *w == word).count();
        }
        let count = text.matches(&query.phrase).count();
        if count > 0 {
            hits.push(Hit {
                index,
                score: count,
                kind: "phrase",
            });
        } else if query.phrase.contains(char::is_whitespace) && all_words {
            hits.push(Hit {
                index,
                score: word_score,
                kind: "words",
            });
        }
    }
    hits.sort_by(|a, b| {
        a.kind
            .cmp(b.kind)
            .then_with(|| b.score.cmp(&a.score))
            .then_with(|| sections[a.index].path.cmp(&sections[b.index].path))
            .then_with(|| {
                sections[a.index]
                    .start_line
                    .cmp(&sections[b.index].start_line)
            })
    });
    (hits, words)
}

pub fn output_lines(
    sections: &[Section],
    hits: &[Hit],
    query: &ExactQuery,
    top_k: usize,
    budget: usize,
) -> Vec<String> {
    use super::{
        excerpt::exact_excerpt,
        output::{json_string, line_tokens},
    };
    let mut used = 0;
    let mut lines = Vec::new();
    for hit in hits.iter().take(top_k) {
        let section = &sections[hit.index];
        let line = format!(
            "{{\"path\":{},\"lines\":[{},{}],\"heading_path\":{},\"score\":{},\"snippet\":{},\"match\":{}}}",
            json_string(&section.path),
            section.start_line,
            section.end_line,
            json_string(&section.heading_path),
            hit.score,
            json_string(&exact_excerpt(section, &query.words)),
            json_string(hit.kind)
        );
        used += line_tokens(&line);
        if !lines.is_empty() && used > budget {
            break;
        }
        lines.push(line);
    }
    lines
}
