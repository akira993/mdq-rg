use ignore::WalkBuilder;
use std::path::{Path, PathBuf};

pub fn fnmatch(pattern: &str, path: &str) -> bool {
    Glob::new(pattern).matches(path)
}

enum Token {
    Star,
    Any,
    Literal(char),
    Class {
        negate: bool,
        ranges: Vec<(char, char)>,
    },
}

pub struct Glob(Vec<Token>);

impl Glob {
    pub fn new(pattern: &str) -> Self {
        let p: Vec<char> = pattern.chars().collect();
        let mut tokens = Vec::new();
        let mut i = 0;
        let mut no_closing_bracket = false;
        while i < p.len() {
            match p[i] {
                '*' => {
                    if !matches!(tokens.last(), Some(Token::Star)) {
                        tokens.push(Token::Star);
                    }
                }
                '?' => tokens.push(Token::Any),
                '[' if !no_closing_bracket => {
                    let negate = p.get(i + 1).is_some_and(|c| *c == '!' || *c == '^');
                    let start = i + if negate { 2 } else { 1 };
                    let search = start + usize::from(p.get(start) == Some(&']'));
                    if let Some(end) = (search..p.len()).find(|j| p[*j] == ']') {
                        let mut ranges = Vec::new();
                        let mut j = start;
                        while j < end {
                            if j + 2 < end && p[j + 1] == '-' {
                                ranges.push((p[j], p[j + 2]));
                                j += 3;
                            } else {
                                ranges.push((p[j], p[j]));
                                j += 1;
                            }
                        }
                        tokens.push(Token::Class { negate, ranges });
                        i = end;
                    } else {
                        no_closing_bracket = true;
                        tokens.push(Token::Literal('['));
                    }
                }
                c => tokens.push(Token::Literal(c)),
            }
            i += 1;
        }
        Self(tokens)
    }

    pub fn matches(&self, path: &str) -> bool {
        let s: Vec<char> = path.chars().collect();
        let (mut p, mut i) = (0, 0);
        let mut star = None;
        while i < s.len() {
            let hit = match self.0.get(p) {
                Some(Token::Star) => {
                    star = Some((p + 1, i));
                    p += 1;
                    continue;
                }
                Some(Token::Any) => true,
                Some(Token::Literal(c)) => *c == s[i],
                Some(Token::Class { negate, ranges }) => {
                    ranges.iter().any(|(a, b)| *a <= s[i] && s[i] <= *b) != *negate
                }
                None => false,
            };
            if hit {
                p += 1;
                i += 1;
            } else if let Some((next, consumed)) = star {
                i = consumed + 1;
                p = next;
                star = Some((next, i));
            } else {
                return false;
            }
        }
        while matches!(self.0.get(p), Some(Token::Star)) {
            p += 1;
        }
        p == self.0.len()
    }
}

#[derive(Default)]
pub struct WalkOptions {
    pub quiet: bool,
    pub hidden: bool,
    pub no_ignore: bool,
    pub paths: Vec<String>,
    pub roots: Vec<String>,
    pub exclude: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SkipReason {
    InvalidUtf8,
    ContainsNul,
    ReadError,
}

pub(crate) fn forbidden(path: &Path) -> bool {
    path.components().any(|component| {
        // Case-insensitive file systems treat U+017F as s and U+00DF and U+1E9E as ss.
        let name = component
            .as_os_str()
            .to_string_lossy()
            .replace('\u{17f}', "s")
            .replace(['\u{df}', '\u{1e9e}'], "ss");
        [".git", "node_modules", ".ssh", ".aws", ".gnupg", ".env"]
            .iter()
            .any(|forbidden| name.eq_ignore_ascii_case(forbidden))
    })
}

pub fn collect(root: &Path, options: &WalkOptions) -> Vec<(String, PathBuf)> {
    collect_with_skipped(root, options).0
}

pub(crate) fn collect_with_skipped(
    root: &Path,
    options: &WalkOptions,
) -> (Vec<(String, PathBuf)>, usize) {
    let Ok(canonical_root) = root.canonicalize() else {
        return (Vec::new(), 0);
    };
    let starts = if options.roots.is_empty() {
        vec![canonical_root.clone()]
    } else {
        options
            .roots
            .iter()
            .filter_map(|name| {
                let named = Path::new(name);
                if named.is_absolute() || forbidden(named) {
                    return None;
                }
                let canonical = root.join(named).canonicalize().ok()?;
                if forbidden(canonical.strip_prefix(&canonical_root).ok()?) {
                    return None;
                }
                Some(canonical)
            })
            .collect()
    };
    let paths: Vec<_> = options.paths.iter().map(|p| Glob::new(p)).collect();
    let excludes: Vec<_> = options.exclude.iter().map(|p| Glob::new(p)).collect();
    let mut files = Vec::new();
    let mut skipped = 0;
    for start in starts {
        let mut builder = WalkBuilder::new(start);
        builder.hidden(!options.hidden);
        let base = canonical_root.clone();
        builder.filter_entry(move |entry| {
            entry
                .path()
                .strip_prefix(&base)
                .is_ok_and(|path| !forbidden(path))
        });
        if options.no_ignore {
            builder
                .parents(false)
                .git_ignore(false)
                .git_global(false)
                .git_exclude(false)
                .ignore(false);
        } else {
            builder.add_custom_ignore_filename(".rgignore");
        }
        for entry in builder.build() {
            match entry {
                Ok(entry)
                    if entry.file_type().is_some_and(|t| t.is_file())
                        && entry.path().extension().is_some_and(|e| e == "md") =>
                {
                    let Ok(relative) = entry.path().strip_prefix(&canonical_root) else {
                        continue;
                    };
                    let relative = relative.to_string_lossy().replace('\\', "/");
                    if (paths.is_empty() || paths.iter().any(|p| p.matches(&relative)))
                        && !excludes.iter().any(|p| p.matches(&relative))
                    {
                        files.push((relative, entry.into_path()));
                    }
                }
                Err(error) => {
                    skipped += 1;
                    if options.quiet {
                        continue;
                    }
                    fn error_path(error: &ignore::Error) -> Option<&Path> {
                        match error {
                            ignore::Error::WithPath { path, .. } => Some(path),
                            ignore::Error::WithDepth { err, .. } => error_path(err),
                            _ => None,
                        }
                    }
                    let path = error_path(&error)
                        .and_then(|path| path.strip_prefix(&canonical_root).ok())
                        .unwrap_or(Path::new("."))
                        .to_string_lossy()
                        .replace('\\', "/");
                    crate::search::output::report_skipped(&path, SkipReason::ReadError);
                }
                _ => {}
            }
        }
    }
    files.sort();
    files.dedup_by(|a, b| a.0 == b.0);
    (files, skipped)
}

pub fn read_document(path: &Path) -> Result<String, SkipReason> {
    let bytes = std::fs::read(path).map_err(|_| SkipReason::ReadError)?;
    let text = String::from_utf8(bytes).map_err(|_| SkipReason::InvalidUtf8)?;
    if text.contains('\0') {
        return Err(SkipReason::ContainsNul);
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        for name in [
            "plain.md",
            "docs/a.md",
            "docs/deep/b.md",
            "docs/excluded.md",
            ".hidden/a.md",
            ".hidden/deep/b.md",
            ".git/a.md",
            "node_modules/a.md",
            "docs/node_modules/b.md",
            "ignored.md",
        ] {
            let path = root.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, "text").unwrap();
        }
        std::fs::write(root.path().join(".gitignore"), "ignored.md\n").unwrap();
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(root.path())
                .status()
                .unwrap()
                .success()
        );
        root
    }

    fn names(root: &Path, options: WalkOptions) -> Vec<String> {
        collect(root, &options)
            .into_iter()
            .map(|(name, _)| name)
            .collect()
    }

    #[test]
    fn default_ignore_and_hidden_flags_and_permanent_exclusions() {
        let root = fixture();
        let normal = names(root.path(), WalkOptions::default());
        assert_eq!(
            normal,
            [
                "docs/a.md",
                "docs/deep/b.md",
                "docs/excluded.md",
                "plain.md"
            ]
        );
        let all = names(
            root.path(),
            WalkOptions {
                hidden: true,
                no_ignore: true,
                ..Default::default()
            },
        );
        assert!(all.contains(&".hidden/a.md".into()));
        assert!(all.contains(&"ignored.md".into()));
        assert!(
            !all.iter()
                .any(|p| p.contains("node_modules") || p.contains(".git/"))
        );
    }

    #[test]
    fn named_roots_paths_and_excludes_use_root_relative_names() {
        let root = fixture();
        let found = names(
            root.path(),
            WalkOptions {
                roots: vec!["docs".into(), "docs/deep".into(), ".hidden".into()],
                exclude: vec!["*excluded.md".into(), ".hidden/deep/*".into()],
                ..Default::default()
            },
        );
        assert_eq!(found, [".hidden/a.md", "docs/a.md", "docs/deep/b.md"]);
        assert_eq!(
            names(
                root.path(),
                WalkOptions {
                    roots: vec!["docs".into()],
                    paths: vec!["docs/?.md".into()],
                    ..Default::default()
                }
            ),
            ["docs/a.md"]
        );
    }

    #[test]
    fn reads_classify_invalid_utf8_nul_and_missing_files() {
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("a.md");
        assert_eq!(read_document(&path), Err(SkipReason::ReadError));
        std::fs::write(&path, [0xff]).unwrap();
        assert_eq!(read_document(&path), Err(SkipReason::InvalidUtf8));
        std::fs::write(&path, b"a\0b").unwrap();
        assert_eq!(read_document(&path), Err(SkipReason::ContainsNul));
        std::fs::write(&path, "hello\n").unwrap();
        assert_eq!(read_document(&path).unwrap(), "hello\n");
    }

    #[cfg(unix)]
    #[test]
    fn roots_follow_internal_aliases_with_canonical_names() {
        let root = fixture();
        std::os::unix::fs::symlink(root.path().join("docs"), root.path().join("alias")).unwrap();
        assert_eq!(
            names(
                root.path(),
                WalkOptions {
                    roots: vec!["alias".into()],
                    ..Default::default()
                }
            ),
            ["docs/a.md", "docs/deep/b.md", "docs/excluded.md"]
        );
    }
}
