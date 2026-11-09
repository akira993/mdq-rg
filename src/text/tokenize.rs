use std::collections::HashSet;
use unicode_segmentation::UnicodeSegmentation;

pub fn tokenize(normalized: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    for_each(normalized, &mut |token| tokens.push(token.to_string()));
    tokens
}

pub fn cjk(c: char) -> bool {
    matches!(c,
        '\u{3040}'..='\u{30ff}' | '\u{31f0}'..='\u{31ff}'
        | '\u{3005}' | '\u{3006}' | '\u{3007}' | '\u{303b}'
        | '\u{3400}'..='\u{4dbf}' | '\u{4e00}'..='\u{9fff}'
        | '\u{f900}'..='\u{faff}' | '\u{20000}'..='\u{3ffff}'
        | '\u{1100}'..='\u{11ff}' | '\u{3130}'..='\u{318f}'
        | '\u{a960}'..='\u{a97f}' | '\u{ac00}'..='\u{d7ff}')
}

fn word(c: char) -> bool {
    if c.is_ascii() {
        c.is_ascii_alphanumeric() || c == '_'
    } else {
        c.is_alphanumeric() && !cjk(c)
    }
}

pub fn word_parts(token: &str) -> impl Iterator<Item = &str> {
    let split = token.contains('_');
    token
        .split('_')
        .filter(move |part| split && !part.is_empty() && !part.chars().all(char::is_numeric))
}

pub fn for_each(normalized: &str, emit: &mut dyn FnMut(&str)) -> usize {
    let mut length = 0;
    let mut emit = |token: &str| {
        length += 1;
        emit(token);
    };
    let mut chars = normalized.char_indices().peekable();
    while let Some((start, c)) = chars.next() {
        if matches!(c, '#' | '*' | '0'..='9')
            && chars
                .peek()
                .is_some_and(|(_, next)| matches!(next, '\u{fe0f}' | '\u{20e3}'))
            && let Some(grapheme) = normalized[start..].graphemes(true).next()
        {
            let end = start + grapheme.len();
            while chars.peek().is_some_and(|(offset, _)| *offset < end) {
                chars.next();
            }
            emit(grapheme);
            continue;
        }
        if cjk(c) {
            let mut previous = start;
            let mut current = start;
            let mut end = start + c.len_utf8();
            while let Some(&(offset, next)) = chars.peek() {
                let next_cjk = cjk(next);
                if matches!(next, '\u{3099}' | '\u{309a}' | '\u{1100}'..='\u{11ff}' | '\u{a960}'..='\u{a97f}' | '\u{d7b0}'..='\u{d7ff}')
                    || (!next_cjk
                        && next >= '\u{300}'
                        && (next == '\u{200d}'
                            || unicode_normalization::char::is_combining_mark(next)))
                {
                    // Delay emission until the last grapheme is complete.
                    end = current + normalized[current..].graphemes(true).next().unwrap().len();
                    while chars.peek().is_some_and(|(offset, _)| *offset < end) {
                        chars.next();
                    }
                    if end > offset {
                        continue;
                    }
                }
                if !next_cjk {
                    break;
                }
                if previous != current {
                    emit(&normalized[previous..end]);
                }
                previous = current;
                current = offset;
                end = offset + next.len_utf8();
                chars.next();
            }
            emit(&normalized[previous..end]);
        } else if word(c) {
            let mut end = start + c.len_utf8();
            while let Some(&(offset, next)) = chars.peek() {
                if !word(next)
                    && (next < '\u{300}' || !unicode_normalization::char::is_combining_mark(next))
                {
                    break;
                }
                end = offset + next.len_utf8();
                chars.next();
            }
            emit(&normalized[start..end]);
        } else if c.is_ascii() || c.is_whitespace() {
            continue;
        } else {
            if let Some(grapheme) = normalized[start..].graphemes(true).next() {
                let end = start + grapheme.len();
                while chars.peek().is_some_and(|(offset, _)| *offset < end) {
                    chars.next();
                }
                if grapheme.chars().any(|c| {
                    matches!(c, '\u{1f000}'..='\u{1faff}' | '\u{2600}'..='\u{27bf}'
                        | '\u{2b00}'..='\u{2bff}' | '\u{fe0f}' | '\u{20e3}')
                }) {
                    emit(grapheme);
                }
            }
        }
    }
    length
}

pub fn expands_single(term: &str) -> bool {
    term.starts_with(cjk) && term.graphemes(true).count() == 1
}

pub fn contains_single(token: &str, term: &str) -> bool {
    let mut graphemes = token.graphemes(true);
    let first = graphemes.next();
    let second = graphemes.next();
    graphemes.next().is_none() && (first == Some(term) || second == Some(term))
}

pub struct Filter<'a> {
    terms: HashSet<&'a str>,
    singles: Vec<&'a str>,
}

impl<'a> Filter<'a> {
    pub fn new(terms: impl Iterator<Item = &'a String>) -> Self {
        let terms: HashSet<_> = terms.map(String::as_str).collect();
        let singles = terms
            .iter()
            .copied()
            .filter(|term| expands_single(term))
            .collect();
        Self { terms, singles }
    }

    pub fn keep(&self, token: &str) -> bool {
        self.terms.contains(token) || self.singles.iter().any(|term| contains_single(token, term))
    }
}
