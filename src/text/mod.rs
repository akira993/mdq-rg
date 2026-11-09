pub mod normalize;
pub mod tokenize;

use unicode_segmentation::UnicodeSegmentation;

pub use normalize::normalize;

fn document_each(normalized: &str, emit: &mut dyn FnMut(&str)) -> usize {
    tokenize::for_each(normalized, &mut |token| {
        emit(token);
        for part in tokenize::word_parts(token) {
            emit(part);
        }
    })
}

pub fn document_counts(
    normalized: &str,
    filter: &tokenize::Filter<'_>,
) -> (std::collections::HashMap<String, usize>, usize) {
    let mut frequencies = std::collections::HashMap::new();
    let length = document_each(normalized, &mut |token| {
        if filter.keep(token) {
            *frequencies.entry(token.to_string()).or_insert(0) += 1;
        }
    });
    (frequencies, length)
}

pub struct QueryTerms {
    pub surface: Vec<String>,
    pub expanded: Vec<String>,
}

impl QueryTerms {
    pub fn new(query: &str) -> Self {
        let surface = tokenize::tokenize(&normalize(query));
        let mut expanded = surface.clone();
        expand_word_parts(&mut expanded);
        Self { surface, expanded }
    }
}

pub fn query_terms(query: &str) -> Vec<String> {
    QueryTerms::new(query).expanded
}

fn expand_word_parts(terms: &mut Vec<String>) {
    let parts: Vec<_> = terms
        .iter()
        .flat_map(|term| tokenize::word_parts(term))
        .map(str::to_string)
        .collect();
    terms.extend(parts);
}

pub fn fold_for_exact(text: &str) -> String {
    normalize::fold(text, false)
}

pub fn truncate_graphemes(text: &str, max_chars: usize) -> String {
    if text.chars().count() > max_chars {
        if max_chars == 0 {
            String::new()
        } else {
            let mut result = String::new();
            let mut length = 0;
            for grapheme in text.graphemes(true) {
                let count = grapheme.chars().count();
                if length + count >= max_chars {
                    break;
                }
                result.push_str(grapheme);
                length += count;
            }
            result.push('\u{2026}');
            result
        }
    } else {
        text.to_string()
    }
}
