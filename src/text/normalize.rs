use std::sync::OnceLock;
use unicode_normalization::{
    UnicodeNormalization,
    char::{canonical_combining_class, is_combining_mark},
};

const KANA: [char; 61] = [
    '\u{3002}', '\u{300c}', '\u{300d}', '\u{3001}', '\u{30fb}', '\u{30f2}', '\u{30a1}', '\u{30a3}',
    '\u{30a5}', '\u{30a7}', '\u{30a9}', '\u{30e3}', '\u{30e5}', '\u{30e7}', '\u{30c3}', '\u{30fc}',
    '\u{30a2}', '\u{30a4}', '\u{30a6}', '\u{30a8}', '\u{30aa}', '\u{30ab}', '\u{30ad}', '\u{30af}',
    '\u{30b1}', '\u{30b3}', '\u{30b5}', '\u{30b7}', '\u{30b9}', '\u{30bb}', '\u{30bd}', '\u{30bf}',
    '\u{30c1}', '\u{30c4}', '\u{30c6}', '\u{30c8}', '\u{30ca}', '\u{30cb}', '\u{30cc}', '\u{30cd}',
    '\u{30ce}', '\u{30cf}', '\u{30d2}', '\u{30d5}', '\u{30d8}', '\u{30db}', '\u{30de}', '\u{30df}',
    '\u{30e0}', '\u{30e1}', '\u{30e2}', '\u{30e4}', '\u{30e6}', '\u{30e8}', '\u{30e9}', '\u{30ea}',
    '\u{30eb}', '\u{30ec}', '\u{30ed}', '\u{30ef}', '\u{30f3}',
];

struct Variants {
    bmp: Box<[char]>,
    supplementary: Vec<(char, char)>,
}

fn variants() -> &'static Variants {
    static TABLE: OnceLock<Variants> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = Variants {
            bmp: vec!['\0'; 65536].into_boxed_slice(),
            supplementary: Vec::new(),
        };
        for line in include_str!("../data/variants.tsv")
            .lines()
            .filter(|line| !line.starts_with('#'))
        {
            if let Some((old, new)) = line.split_once('\t')
                && let (Some(old), Some(new)) = (old.chars().next(), new.chars().next())
            {
                if (old as usize) < table.bmp.len() {
                    table.bmp[old as usize] = new;
                } else {
                    table.supplementary.push((old, new));
                }
            }
        }
        table.supplementary.sort_unstable_by_key(|pair| pair.0);
        table
    })
}

pub fn normalize(text: &str) -> String {
    fold(text, true)
}

pub(super) fn fold(text: &str, variant: bool) -> String {
    let table = variant.then(variants);
    let mut result = String::with_capacity(text.len());
    let mut combining = false;
    for c in text.chars() {
        if c.is_ascii() {
            result.push(c.to_ascii_lowercase());
            continue;
        }
        let mapped = match c {
            '\u{0130}' => 'i',
            '\u{ff01}'..='\u{ff5e}' => char::from_u32(c as u32 - 0xfee0).unwrap_or(c),
            '\u{ff61}'..='\u{ff9d}' => KANA[c as usize - 0xff61],
            '\u{ff9e}' => '\u{3099}',
            '\u{ff9f}' => '\u{309a}',
            '\u{3000}' => ' ',
            _ => c,
        };
        let mapped = if let Some(table) = table {
            if (mapped as usize) < table.bmp.len() {
                let new = table.bmp[mapped as usize];
                if new == '\0' { mapped } else { new }
            } else {
                table
                    .supplementary
                    .binary_search_by_key(&mapped, |pair| pair.0)
                    .map_or(mapped, |i| table.supplementary[i].1)
            }
        } else {
            mapped
        };
        combining |= is_combining_mark(mapped);
        result.extend(mapped.to_lowercase());
    }
    if combining {
        let mut base_is_i = false;
        result
            .nfd()
            .filter(|&c| {
                if canonical_combining_class(c) == 0 {
                    base_is_i = c == 'i';
                }
                !(base_is_i && c == '\u{307}')
            })
            .nfc()
            .collect()
    } else {
        result
    }
}
