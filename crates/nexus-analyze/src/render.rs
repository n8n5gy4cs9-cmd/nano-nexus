//! Report exports: Markdown (for notes), JSON (the raw report), and CSV tables (ANALYZER §3).

use crate::facts::folder_facts;
use crate::fmt::{bytes, count, date, date_time};
use crate::report::FolderReport;

/// The raw report.
pub fn json(r: &FolderReport) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(r)
}

fn md_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// A readable Markdown report. `narrative` is the AI summary, when there is one.
pub fn markdown(r: &FolderReport, shown_root: &str, narrative: Option<&str>) -> String {
    let t = &r.totals;
    let mut o = format!("# Folder report: {shown_root}\n\n");
    o.push_str(&format!(
        "Scanned {} · {} files · {} folders · {}\n\n",
        date_time(r.scanned_at),
        count(t.files),
        count(t.dirs),
        bytes(t.bytes)
    ));
    if let Some(n) = narrative.filter(|n| !n.trim().is_empty()) {
        o.push_str("## Summary\n\n");
        o.push_str(n.trim());
        o.push_str("\n\n");
    }
    o.push_str("## By category\n\n| Category | Files | Size | Share |\n|---|---:|---:|---:|\n");
    for c in &r.by_category {
        o.push_str(&format!(
            "| {} | {} | {} | {:.0}% |\n",
            c.category.label(),
            count(c.files),
            bytes(c.bytes),
            c.pct_bytes
        ));
    }
    o.push_str("\n## Largest files\n\n| File | Size | Modified |\n|---|---:|---|\n");
    for f in r.largest_files.iter().take(20) {
        o.push_str(&format!(
            "| {} | {} | {} |\n",
            md_cell(&f.rel_path),
            bytes(f.bytes),
            f.modified.map_or("unknown".into(), date)
        ));
    }
    o.push_str("\n## Largest folders\n\n| Folder | Size | Files |\n|---|---:|---:|\n");
    for d in r.largest_dirs.iter().take(15) {
        o.push_str(&format!(
            "| {} | {} | {} |\n",
            md_cell(&d.rel_path),
            bytes(d.bytes),
            count(d.files)
        ));
    }
    if !r.duplicates.is_empty() {
        o.push_str(&format!(
            "\n## Duplicates ({} wasted)\n\n",
            bytes(r.wasted_by_duplicates)
        ));
        for g in r.duplicates.iter().take(20) {
            o.push_str(&format!(
                "- {} each × {}{}\n",
                bytes(g.bytes_each),
                g.paths.len(),
                if g.confirmed { "" } else { " (probable)" }
            ));
            for p in g.paths.iter().take(6) {
                o.push_str(&format!("  - {}\n", md_cell(p)));
            }
        }
    }
    if !r.junk.is_empty() {
        o.push_str("\n## Junk and regenerable folders\n\n| Item | Rule | Size | Safe to trash |\n|---|---|---:|---|\n");
        for j in r.junk.iter().take(40) {
            o.push_str(&format!(
                "| {} | {} | {} | {} |\n",
                md_cell(&j.rel_path),
                md_cell(&j.rule),
                bytes(j.bytes),
                if j.safe_to_trash { "yes" } else { "no" }
            ));
        }
    }
    if !r.naming_issues.is_empty() {
        o.push_str(&format!(
            "\n## Naming issues ({})\n\n",
            count(r.naming_issues.len() as u64)
        ));
        for n in r.naming_issues.iter().take(40) {
            o.push_str(&format!("- `{}`: {} ({})\n", n.rel_path, n.detail, n.kind));
        }
    }
    o.push_str("\n---\n\n");
    o.push_str(&folder_facts(r, shown_root));
    o.push('\n');
    o
}

/// Quote one CSV field (RFC 4180). Fields that start with `= + - @` are prefixed with `'` so spreadsheets
/// do not run them as formulas.
fn csv_field(s: &str) -> String {
    let safe = if s.starts_with(['=', '+', '-', '@']) {
        format!("'{s}")
    } else {
        s.to_owned()
    };
    if safe.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", safe.replace('"', "\"\""))
    } else {
        safe
    }
}

fn csv_row(fields: &[String]) -> String {
    let mut s = fields.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(",");
    s.push('\n');
    s
}

/// Largest files as CSV.
pub fn csv_largest(r: &FolderReport) -> String {
    let mut o = csv_row(&[
        "path".into(),
        "bytes".into(),
        "size".into(),
        "modified".into(),
        "category".into(),
    ]);
    for f in &r.largest_files {
        o.push_str(&csv_row(&[
            f.rel_path.clone(),
            f.bytes.to_string(),
            bytes(f.bytes),
            f.modified.map_or(String::new(), date),
            f.category.label().into(),
        ]));
    }
    o
}

/// Duplicate groups as CSV (one row per file).
pub fn csv_duplicates(r: &FolderReport) -> String {
    let mut o = csv_row(&[
        "group".into(),
        "path".into(),
        "bytes_each".into(),
        "confirmed".into(),
    ]);
    for (i, g) in r.duplicates.iter().enumerate() {
        for p in &g.paths {
            o.push_str(&csv_row(&[
                (i + 1).to_string(),
                p.clone(),
                g.bytes_each.to_string(),
                g.confirmed.to_string(),
            ]));
        }
    }
    o
}

/// Junk as CSV.
pub fn csv_junk(r: &FolderReport) -> String {
    let mut o = csv_row(&[
        "path".into(),
        "rule".into(),
        "bytes".into(),
        "safe_to_trash".into(),
        "regenerable".into(),
    ]);
    for j in &r.junk {
        o.push_str(&csv_row(&[
            j.rel_path.clone(),
            j.rule.clone(),
            j.bytes.to_string(),
            j.safe_to_trash.to_string(),
            j.regenerable.to_string(),
        ]));
    }
    o
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dups::DupMode;
    use crate::scan::{ScanOptions, scan_folder};
    use tokio_util::sync::CancellationToken;

    fn report() -> FolderReport {
        let d = tempfile::tempdir().unwrap();
        let w = |rel: &str, data: &[u8]| {
            let p = d.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, data).unwrap();
        };
        w("a/=evil.txt", &[1u8; 6000]);
        w("a/b, c.txt", &[1u8; 6000]);
        w("a/pipe|name.txt", &[2u8; 10]);
        w(".DS_Store", b"x");
        let mut o = ScanOptions::new(d.path());
        o.duplicates = DupMode::Full;
        o.include_hidden = true;
        scan_folder(&o, &|_| {}, &CancellationToken::new()).unwrap()
    }

    #[test]
    fn markdown_has_the_sections_and_the_summary() {
        let r = report();
        let md = markdown(&r, "~/Downloads", Some("You have **3** files."));
        for h in [
            "# Folder report: ~/Downloads",
            "## Summary",
            "## By category",
            "## Largest files",
            "## Duplicates",
            "## Junk and regenerable folders",
        ] {
            assert!(md.contains(h), "missing {h}\n{md}");
        }
        assert!(md.contains("pipe\\|name.txt"), "pipes in cells are escaped");
        assert!(!markdown(&r, "~", None).contains("## Summary"));
    }

    #[test]
    fn json_round_trips() {
        let r = report();
        let text = json(&r).unwrap();
        let back: FolderReport = serde_json::from_str(&text).unwrap();
        assert_eq!(back, r);
    }

    #[test]
    fn csv_is_quoted_and_formula_safe() {
        let r = report();
        let c = csv_largest(&r);
        assert!(c.starts_with("path,bytes,size,modified,category\n"));
        assert!(c.contains("\"a/b, c.txt\""));
        assert!(c.contains("'a/=evil.txt") || c.contains("a/=evil.txt"), "{c}");
        assert!(!c.lines().any(|l| l.starts_with('=')));
        assert_eq!(csv_field("=SUM(A1)"), "'=SUM(A1)");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        let d = csv_duplicates(&r);
        assert_eq!(d.lines().count(), 3);
        assert!(csv_junk(&r).contains(".DS_Store,.DS_Store,1,true,false"));
    }
}
