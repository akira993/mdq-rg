use mdq_rg::text::tokenize;
use mdq_rg::text::*;
use unicode_segmentation::UnicodeSegmentation;

#[path = "common/text.rs"]
mod text;
use text::document_tokens;

#[test]
fn text_normalization_and_exact_folding() {
    for (old, new) in [
        ("髙橋", "高橋"),
        ("齋藤", "斉藤"),
        ("渡邊", "渡辺"),
        ("ＡＢＣ１２３", "abc123"),
        ("ｶﾞｲﾄﾞ", "ガイド"),
        ("か\u{3099}", "が"),
        ("e\u{301}", "é"),
        ("　Ａ！ﾊﾟﾋﾟﾌﾟﾍﾟﾎﾟ", " a!パピプペポ"),
        ("神𠮷", "神吉"),
    ] {
        assert_eq!(normalize(old), new);
    }
    assert_eq!(
        fold_for_exact("髙橋齋藤渡邊 Ａｶﾞe\u{301}"),
        "髙橋齋藤渡邊 aガé"
    );
}

#[test]
fn dotted_i_folding_preserves_dotless_i_and_combining_marks() {
    for (input, expected) in [
        ("İSTANBUL", "istanbul"),
        ("İ", "i"),
        ("ıstanbul", "ıstanbul"),
        ("I", "i"),
        ("I\u{307}STANBUL", "istanbul"),
        ("i\u{307}stanbul", "istanbul"),
        ("E\u{301}COLE", "école"),
        ("İE\u{301}", "ié"),
        ("Q\u{301}", "q\u{301}"),
    ] {
        assert_eq!(normalize(input), expected, "{input:?}");
        assert_eq!(fold_for_exact(input), expected, "{input:?}");
        assert_eq!(query_terms(input), [expected], "{input:?}");
        assert_eq!(
            document_tokens(&normalize(input)).0,
            [expected],
            "{input:?}"
        );
    }
}

#[test]
fn dotted_i_canonical_spellings_fold_together() {
    for (inputs, expected) in [
        (
            ["İSTANBUL", "I\u{307}STANBUL", "i\u{307}stanbul"],
            "istanbul",
        ),
        (["İ\u{301}", "I\u{307}\u{301}", "i\u{301}\u{307}"], "í"),
        (["İ\u{323}", "I\u{323}\u{307}", "i\u{307}\u{323}"], "ị"),
        (["Í\u{307}", "I\u{301}\u{307}", "í\u{307}"], "í"),
        (["Ị\u{307}", "I\u{307}\u{323}", "ị\u{307}"], "ị"),
    ] {
        for input in inputs {
            assert_eq!(normalize(input), expected, "normalize: {input:?}");
            assert_eq!(fold_for_exact(input), expected, "exact: {input:?}");
        }
    }
    for (input, expected) in [
        ("ı\u{307}", "ı\u{307}"),
        ("Ė", "ė"),
        ("E\u{307}", "ė"),
        ("e\u{323}\u{307}", "ẹ\u{307}"),
        ("Q\u{307}", "q\u{307}"),
        ("I\u{34f}\u{307}", "i\u{34f}\u{307}"),
    ] {
        assert_eq!(normalize(input), expected, "normalize: {input:?}");
        assert_eq!(fold_for_exact(input), expected, "exact: {input:?}");
    }
}

#[test]
fn text_unicode_words_and_graphemes() {
    assert_eq!(
        tokenize::tokenize("人々 𠮷野家 中文測試 한국어 한"),
        [
            "人々", "𠮷野", "野家", "中文", "文測", "測試", "한국", "국어", "한"
        ]
    );
    assert_eq!(
        tokenize::tokenize("café русский é e\u{301} 👨‍👩‍👧 🇯🇵 👍🏽"),
        ["café", "русский", "é", "e\u{301}", "👨‍👩‍👧", "🇯🇵", "👍🏽"]
    );
    assert_eq!(tokenize::tokenize("abc_12-xyz_9"), ["abc_12", "xyz_9"]);
    assert_eq!(
        tokenize::tokenize("abc_12東京都-xyz_9é𠮷 API設定"),
        ["abc_12", "東京", "京都", "xyz_9é", "𠮷", "API", "設定"]
    );
    assert!(tokenize::cjk('﨑'));
    assert!(tokenize::cjk('\u{323af}'));
    assert!(tokenize::cjk('\u{3347f}'));
}

#[test]
fn text_variant_queries_match_document_terms() {
    for (old, new) in [
        ("髙橋", "高橋"),
        ("齋藤", "斉藤"),
        ("渡邊", "渡辺"),
        ("ＡＢＣ１２３", "abc123"),
        ("ｶﾞｲﾄﾞ", "ガイド"),
    ] {
        let document = document_tokens(&normalize(new)).0;
        assert_eq!(query_terms(old), query_terms(new));
        assert_eq!(document_tokens(&normalize(old)).0, document);
    }
}

#[test]
fn text_canonical_compatibility_mappings_are_complete() {
    for code in 0xf900..=0xfaff {
        let c = char::from_u32(code).unwrap();
        let mut canonical = String::new();
        unicode_normalization::char::decompose_canonical(c, |d| canonical.push(d));
        assert_eq!(normalize(&c.to_string()), normalize(&canonical));
    }
}

#[test]
fn text_truncation_keeps_complete_graphemes_and_character_budget() {
    for text in ["ab👨‍👩‍👧cd", "a🇯🇵b", "a👍🏽b", "ae\u{301}b", "あいう"] {
        for max in 0..=text.chars().count() + 1 {
            let truncated = truncate_graphemes(text, max);
            assert!(truncated.chars().count() <= max);
            if text.chars().count() > max && max > 0 {
                let prefix = truncated.strip_suffix('…').unwrap();
                assert!(
                    text.grapheme_indices(true)
                        .any(|(offset, _)| offset == prefix.len())
                );
            } else if text.chars().count() <= max {
                assert_eq!(truncated, text);
            }
        }
    }
}

#[test]
fn text_variant_table_is_terminal_and_valid() {
    let mut entries = std::collections::BTreeMap::new();
    for line in include_str!("../src/data/variants.tsv")
        .lines()
        .filter(|line| !line.starts_with('#'))
    {
        let (old, new) = line.split_once('\t').unwrap();
        assert_eq!(old.chars().count(), 1);
        assert_eq!(new.chars().count(), 1);
        assert_ne!(old, new);
        assert!(entries.insert(old, new).is_none());
        assert_eq!(normalize(old), new);
    }
    assert!(entries.len() >= 364);
    assert!(entries.values().all(|new| !entries.contains_key(new)));
}

#[test]
fn text_variant_initialization_budget() {
    let start = std::time::Instant::now();
    let normalized = normalize("髙");
    let elapsed = start.elapsed();
    eprintln!(
        "variant initialization: {elapsed:?}, output characters: {}",
        normalized.chars().count()
    );
    assert!(elapsed < std::time::Duration::from_secs(10));
}

#[test]
fn text_punctuation_is_discarded_but_emoji_and_words_remain() {
    assert_eq!(
        tokenize::tokenize("、。「」（）→… café ☀️ ⬆ 🇯🇵"),
        ["café", "☀️", "⬆", "🇯🇵"]
    );
}

#[test]
fn text_ascii_keycap_emoji_remain_complete_graphemes() {
    assert_eq!(tokenize::tokenize("#️⃣ *️⃣ 1️⃣"), ["#️⃣", "*️⃣", "1️⃣"]);
}

#[test]
fn text_japanese_range_includes_middle_dot() {
    assert_eq!(
        tokenize::tokenize("東京・大阪"),
        ["東京", "京・", "・大", "大阪"]
    );
    for code in 0x3040..=0x30ff {
        assert!(tokenize::cjk(char::from_u32(code).unwrap()));
    }
}

#[test]
fn underscore_parts_preserve_surfaces_lengths_and_query_occurrences() {
    let (tokens, length) = document_tokens("release__log 123_456 release_release");
    assert_eq!(
        tokens,
        [
            "release__log",
            "release",
            "log",
            "123_456",
            "release_release",
            "release",
            "release"
        ]
    );
    assert_eq!(length, 3);

    let query = query_terms("release__log release_release release");
    assert_eq!(
        query,
        [
            "release__log",
            "release_release",
            "release",
            "release",
            "log",
            "release",
            "release"
        ]
    );

    for token in ["123_456", "___", "plain"] {
        assert_eq!(tokenize::word_parts(token).count(), 0, "{token}");
    }
    assert_eq!(
        tokenize::word_parts("10_release_²_é").collect::<Vec<_>>(),
        ["release", "é"]
    );
}

#[test]
fn underscore_parts_keep_combining_marks_in_words() {
    assert_eq!(tokenize::tokenize("foo_bar\u{3099}"), ["foo_bar\u{3099}"]);
    assert_eq!(
        tokenize::word_parts("foo_bar\u{3099}").collect::<Vec<_>>(),
        ["foo", "bar\u{3099}"]
    );
}
