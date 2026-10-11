//! The `<facts>` block the model narrates (ANALYZER §1). Everything is computed and pre-formatted here, so
//! the narrative can be checked against it: every number the model writes must appear in these facts.

use crate::fmt::{bytes, count, date, date_time, pct};
use crate::report::FolderReport;

/// Facts for the folder-report narrative (about 600 tokens, never more than 1 200).
/// `shown_root` is the display form of the root (`~/Downloads`).
pub fn folder_facts(r: &FolderReport, shown_root: &str) -> String {
    let t = &r.totals;
    let mut out = String::from("<facts>\n");
    out.push_str(&format!(
        "root: {shown_root}   scanned: {}   files: {}   folders: {}   size: {}\n",
        date_time(r.scanned_at),
        count(t.files),
        count(t.dirs),
        bytes(t.bytes)
    ));
    let cats: Vec<String> = r
        .by_category
        .iter()
        .take(5)
        .map(|c| {
            format!(
                "{} {} ({})",
                c.category.label(),
                bytes(c.bytes),
                pct(c.bytes, t.bytes)
            )
        })
        .collect();
    if !cats.is_empty() {
        out.push_str(&format!("top categories by size: {}\n", cats.join(", ")));
    }
    let files: Vec<String> = r
        .largest_files
        .iter()
        .take(10)
        .enumerate()
        .map(|(i, f)| {
            format!(
                "{}) {} {} ({})",
                i + 1,
                f.rel_path,
                bytes(f.bytes),
                f.modified.map_or("date unknown".into(), date)
            )
        })
        .collect();
    if !files.is_empty() {
        out.push_str(&format!("largest files: {}\n", files.join(" ")));
    }
    let dirs: Vec<String> = r
        .largest_dirs
        .iter()
        .take(8)
        .enumerate()
        .map(|(i, d)| format!("{}) {} {}", i + 1, d.rel_path, bytes(d.bytes)))
        .collect();
    if !dirs.is_empty() {
        out.push_str(&format!("largest folders: {}\n", dirs.join(" ")));
    }
    let old = r.age.older.bytes;
    out.push_str(&format!(
        "age: older than 1 year {} ({}) · last 30 days {}\n",
        bytes(old),
        pct(old, t.bytes),
        bytes(r.age.last_7d.bytes + r.age.last_30d.bytes)
    ));
    if r.duplicates.is_empty() {
        out.push_str("duplicates: none found\n");
    } else {
        let biggest = r
            .duplicates
            .first()
            .map(|g| {
                format!(
                    "{} ×{} ({} each)",
                    g.paths.first().map_or("", |p| p.rsplit('/').next().unwrap_or(p)),
                    g.paths.len(),
                    bytes(g.bytes_each)
                )
            })
            .unwrap_or_default();
        out.push_str(&format!(
            "duplicates: {} groups, {} wasted; biggest: {biggest}\n",
            count(r.duplicates.len() as u64),
            bytes(r.wasted_by_duplicates)
        ));
    }
    if !r.junk.is_empty() {
        let mut by_rule: Vec<(String, u64, u64)> = Vec::new();
        for j in &r.junk {
            match by_rule.iter_mut().find(|x| x.0 == j.rule) {
                Some(e) => {
                    e.1 += 1;
                    e.2 += j.bytes;
                }
                None => by_rule.push((j.rule.clone(), 1, j.bytes)),
            }
        }
        by_rule.sort_by_key(|x| std::cmp::Reverse(x.2));
        let parts: Vec<String> = by_rule
            .iter()
            .take(5)
            .map(|(rule, n, b)| format!("{rule} ×{n} ({})", bytes(*b)))
            .collect();
        out.push_str(&format!("junk: {}\n", parts.join(" · ")));
    }
    if !r.naming_issues.is_empty() {
        let copy = r
            .naming_issues
            .iter()
            .filter(|n| n.kind == "copy_pattern")
            .count();
        let non_ascii = r.naming_issues.iter().filter(|n| n.kind == "non_ascii").count();
        let spaces = r.naming_issues.iter().filter(|n| n.kind == "spaces").count();
        let mut parts = Vec::new();
        if copy > 0 {
            parts.push(format!("{copy} files with \"copy\"/\"(1)\" patterns"));
        }
        if non_ascii > 0 {
            parts.push(format!("{non_ascii} non-ASCII names"));
        }
        if spaces > 0 {
            parts.push(format!("{spaces} names with spaces"));
        }
        if !parts.is_empty() {
            out.push_str(&format!("naming: {}\n", parts.join(" · ")));
        }
    }
    if !r.empty_dirs.is_empty() || !r.empty_files.is_empty() {
        out.push_str(&format!(
            "empty: {} folders, {} files\n",
            count(r.empty_dirs.len() as u64),
            count(r.empty_files.len() as u64)
        ));
    }
    if !r.warnings.is_empty() {
        out.push_str(&format!(
            "warnings: {} items unreadable (permission)\n",
            count(r.warnings.len() as u64)
        ));
    }
    if r.truncated {
        out.push_str("note: the scan stopped at its size limit, so totals are partial\n");
    }
    out.push_str("</facts>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dups::DupMode;
    use crate::scan::{ScanOptions, scan_folder};
    use tokio_util::sync::CancellationToken;

    fn tree() -> tempfile::TempDir {
        let d = tempfile::tempdir().unwrap();
        let w = |rel: &str, data: &[u8]| {
            let p = d.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, data).unwrap();
        };
        w("Movies/talk.mov", &vec![1u8; 20_000]);
        w("Movies/talk copy.mov", &vec![1u8; 20_000]);
        w("docs/report.pdf", &[0u8; 500]);
        w("node_modules/a/b.js", &[0u8; 3000]);
        w("old/é.txt", b"x");
        d
    }

    #[test]
    fn facts_cover_the_report_and_stay_small() {
        let d = tree();
        let mut o = ScanOptions::new(d.path());
        o.duplicates = DupMode::Full;
        o.min_dup_size = 1024;
        let r = scan_folder(&o, &|_| {}, &CancellationToken::new()).unwrap();
        let f = folder_facts(&r, "~/Downloads");
        assert!(f.starts_with("<facts>\nroot: ~/Downloads") && f.ends_with("</facts>"));
        assert!(
            f.contains("files: 5") && f.contains("top categories by size: Video 39.1 KB"),
            "{f}"
        );
        assert!(
            f.contains("duplicates: 1 groups, 19.5 KB wasted; biggest: talk copy.mov ×2 (19.5 KB each)"),
            "{f}"
        );
        assert!(f.contains("junk: node_modules/ ×1 (2.9 KB)"), "{f}");
        assert!(
            f.contains("1 files with \"copy\"") && f.contains("1 non-ASCII names"),
            "{f}"
        );
        assert!(f.len() / 3 < 1200);
    }

    #[test]
    fn a_huge_report_still_fits_the_budget() {
        let d = tempfile::tempdir().unwrap();
        for i in 0..300 {
            let p = d
                .path()
                .join(format!("dir{}/very-long-file-name-number-{i} copy.dat", i % 40));
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, vec![(i % 7) as u8; 6000]).unwrap();
        }
        let mut o = ScanOptions::new(d.path());
        o.duplicates = DupMode::Full;
        let r = scan_folder(&o, &|_| {}, &CancellationToken::new()).unwrap();
        let f = folder_facts(&r, "~/Big");
        assert!(f.chars().count() as f64 / 3.2 <= 1200.0, "{} chars", f.len());
    }
}
