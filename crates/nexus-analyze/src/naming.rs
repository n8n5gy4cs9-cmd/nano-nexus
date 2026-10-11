//! File and folder naming checks (ANALYZER §1, "Naming issues (files)").

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct NamingHit {
    /// `spaces | mixed_separators | non_ascii | too_long | copy_pattern | trailing | inconsistent_case | extension_mismatch`
    pub kind: String,
    pub rel_path: String,
    pub detail: String,
}

/// The patterns that mean "someone made a duplicate and never cleaned up".
fn copy_pattern(stem: &str) -> Option<String> {
    let lower = stem.to_lowercase();
    let words = lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>();
    if words.contains(&"copy") {
        return Some("contains “copy”".into());
    }
    if lower.ends_with(')')
        && lower.rfind('(').is_some_and(|i| {
            lower[i + 1..lower.len() - 1].chars().all(|c| c.is_ascii_digit()) && lower.len() - i > 2
        })
    {
        return Some("ends with a counter like “(1)”".into());
    }
    let squashed: String = lower.chars().filter(|c| c.is_alphanumeric()).collect();
    if squashed.contains("finalfinal") {
        return Some("“final_final”".into());
    }
    if words.windows(2).any(|w| {
        w[0].starts_with('v')
            && w[0][1..].chars().all(|c| c.is_ascii_digit())
            && w[0].len() > 1
            && matches!(w[1], "new" | "final" | "old")
    }) {
        return Some("version words like “v2_new”".into());
    }
    None
}

/// Checks that need only the file name. `rel_path` is what the report shows.
pub fn check_name(rel_path: &str) -> Vec<NamingHit> {
    let name = rel_path.rsplit('/').next().unwrap_or(rel_path);
    let (stem, _ext) = name
        .rsplit_once('.')
        .filter(|(s, _)| !s.is_empty())
        .unwrap_or((name, ""));
    let mut out = Vec::new();
    let mut push = |kind: &str, detail: String| {
        out.push(NamingHit {
            kind: kind.into(),
            rel_path: rel_path.to_owned(),
            detail,
        })
    };
    if stem.contains(' ') {
        push("spaces", "the name contains spaces".into());
    }
    if stem.contains('_') && stem.contains('-') {
        push("mixed_separators", "mixes “_” and “-”".into());
    }
    if !name.is_ascii() {
        push("non_ascii", "the name has non-ASCII characters".into());
    }
    if name.chars().count() > 100 {
        push("too_long", format!("{} characters long", name.chars().count()));
    }
    if let Some(why) = copy_pattern(stem) {
        push("copy_pattern", why);
    }
    if stem.ends_with(' ') || stem.ends_with('.') || name.ends_with(' ') || name.ends_with('.') {
        push(
            "trailing",
            "ends with a space or dot (Windows cannot open these)".into(),
        );
    }
    out
}

/// Naming style of one stem.
fn style(stem: &str) -> &'static str {
    let has = |c: char| stem.contains(c);
    if has(' ') {
        "words with spaces"
    } else if has('_') && !has('-') {
        if stem.chars().any(|c| c.is_uppercase()) {
            "Snake_Case"
        } else {
            "snake_case"
        }
    } else if has('-') && !has('_') {
        "kebab-case"
    } else if stem.chars().next().is_some_and(char::is_uppercase)
        && stem.chars().any(char::is_lowercase)
        && stem.chars().filter(|c| c.is_uppercase()).count() > 1
    {
        "PascalCase"
    } else if stem.chars().next().is_some_and(char::is_lowercase) && stem.chars().any(char::is_uppercase) {
        "camelCase"
    } else {
        "plain"
    }
}

/// Folders where files follow clearly different naming styles (each style used by at least 3 files).
pub fn inconsistent_case(files: &[String]) -> Vec<NamingHit> {
    let mut by_dir: BTreeMap<&str, BTreeMap<&'static str, u32>> = BTreeMap::new();
    for f in files {
        let (dir, name) = f.rsplit_once('/').unwrap_or(("", f));
        let stem = name
            .rsplit_once('.')
            .filter(|(s, _)| !s.is_empty())
            .map_or(name, |(s, _)| s);
        let s = style(stem);
        if s != "plain" {
            *by_dir.entry(dir).or_default().entry(s).or_default() += 1;
        }
    }
    by_dir
        .into_iter()
        .filter_map(|(dir, styles)| {
            let big: Vec<_> = styles.iter().filter(|(_, n)| **n >= 3).collect();
            (big.len() >= 2).then(|| NamingHit {
                kind: "inconsistent_case".into(),
                rel_path: if dir.is_empty() {
                    ".".into()
                } else {
                    dir.to_owned()
                },
                detail: format!(
                    "mixed styles: {}",
                    big.iter()
                        .map(|(s, n)| format!("{s} ×{n}"))
                        .collect::<Vec<_>>()
                        .join(", ")
                ),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(p: &str) -> Vec<String> {
        check_name(p).into_iter().map(|h| h.kind).collect()
    }

    #[test]
    fn clean_names_pass() {
        assert!(kinds("docs/report-2026.pdf").is_empty());
        assert!(kinds("a/b/my_file.txt").is_empty());
        assert!(kinds(".gitignore").is_empty());
    }

    #[test]
    fn each_problem_is_named() {
        assert_eq!(kinds("My Report.docx"), ["spaces"]);
        assert_eq!(kinds("a_b-c.txt"), ["mixed_separators"]);
        assert_eq!(kinds("café.txt"), ["non_ascii"]);
        assert_eq!(kinds(&format!("{}.txt", "x".repeat(101))), ["too_long"]);
        assert_eq!(kinds("trailing .txt"), ["spaces", "trailing"]);
        assert!(kinds("name..").contains(&"trailing".to_owned()));
    }

    #[test]
    fn copy_patterns() {
        for n in [
            "report copy.docx",
            "report (1).docx",
            "report_final_final.docx",
            "deck_v2_new.pptx",
            "Copy of notes.txt",
            "photo(12).jpg",
        ] {
            assert!(
                kinds(n).contains(&"copy_pattern".to_owned()),
                "{n}: {:?}",
                kinds(n)
            );
        }
        for n in ["copyright.txt", "photo(a).jpg", "v2.txt", "review.docx"] {
            assert!(!kinds(n).contains(&"copy_pattern".to_owned()), "{n}");
        }
    }

    #[test]
    fn styles_within_a_folder() {
        let mut files: Vec<String> = (0..3).map(|i| format!("src/my_file_{i}.rs")).collect();
        files.extend((0..3).map(|i| format!("src/MyFile{i}Thing.rs")));
        files.extend((0..2).map(|i| format!("lib/kebab-{i}.js")));
        let hits = inconsistent_case(&files);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].rel_path, "src");
        assert!(
            hits[0].detail.contains("snake_case ×3") && hits[0].detail.contains("PascalCase ×3"),
            "{}",
            hits[0].detail
        );
    }
}
