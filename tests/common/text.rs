use mdq_rg::text::tokenize;

pub fn document_tokens(normalized: &str) -> (Vec<String>, usize) {
    let mut tokens = Vec::new();
    let length = tokenize::for_each(normalized, &mut |token| {
        tokens.push(token.to_string());
        tokens.extend(tokenize::word_parts(token).map(str::to_string));
    });
    (tokens, length)
}
