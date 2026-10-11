//! The secrets scanner (M08 design notes): deterministic rules modeled on gitleaks' defaults, an entropy
//! check for generic assignments, and PII hints. **Values are redacted the moment they are found** — the
//! raw text never enters a finding, a report, the database, or a prompt.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::OnceLock;

use ignore::WalkBuilder;
use rayon::prelude::*;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;

/// Largest file scanned (secrets hide in config and source, not in big blobs).
const MAX_FILE_BYTES: u64 = 1024 * 1024;
const MAX_FILES: usize = 50_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Low,
    Medium,
    High,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct SecretFinding {
    /// Rule id, for example `aws-access-key`.
    pub rule: String,
    /// `secret | pii | env_file`
    pub kind: String,
    pub severity: Severity,
    /// Path relative to the scan root.
    pub file: String,
    pub line: u32,
    /// Always redacted (`AKIA****WXYZ`).
    pub masked: String,
    /// The path or value looks like a test fixture or placeholder.
    pub likely_fixture: bool,
}

struct Rule {
    id: &'static str,
    kind: &'static str,
    severity: Severity,
    re: Regex,
    /// Capture group holding the secret value (0 = whole match).
    group: usize,
    /// Require Shannon entropy >= 3.5 on the value.
    entropy: bool,
}

fn rules() -> &'static [Rule] {
    static R: OnceLock<Vec<Rule>> = OnceLock::new();
    R.get_or_init(|| {
        let mk = |id, kind, severity, pat: &str, group, entropy| Rule {
            id,
            kind,
            severity,
            re: Regex::new(pat).unwrap_or_else(|_| Regex::new("$^").unwrap_or_else(|_| unreachable!())),
            group,
            entropy,
        };
        use Severity::{High, Low, Medium};
        vec![
            mk(
                "aws-access-key",
                "secret",
                High,
                r"\b(AKIA|ASIA)[0-9A-Z]{16}\b",
                0,
                false,
            ),
            mk(
                "github-token",
                "secret",
                High,
                r"\bgh[pousr]_[A-Za-z0-9]{36,}\b",
                0,
                false,
            ),
            mk(
                "gitlab-token",
                "secret",
                High,
                r"\bglpat-[A-Za-z0-9_-]{20}\b",
                0,
                false,
            ),
            mk(
                "slack-token",
                "secret",
                High,
                r"\bxox[baprs]-[A-Za-z0-9-]{10,}\b",
                0,
                false,
            ),
            mk(
                "stripe-live-key",
                "secret",
                High,
                r"\bsk_live_[0-9a-zA-Z]{24,}\b",
                0,
                false,
            ),
            mk(
                "google-api-key",
                "secret",
                High,
                r"\bAIza[0-9A-Za-z_-]{35}\b",
                0,
                false,
            ),
            mk(
                "anthropic-key",
                "secret",
                High,
                r"\bsk-ant-[A-Za-z0-9_-]{20,}\b",
                0,
                false,
            ),
            mk(
                "openai-style-key",
                "secret",
                High,
                r"\bsk-[A-Za-z0-9_-]{20,}\b",
                0,
                false,
            ),
            mk(
                "private-key",
                "secret",
                High,
                r"-----BEGIN (RSA |EC |OPENSSH |DSA |PGP )?PRIVATE KEY( BLOCK)?-----",
                0,
                false,
            ),
            mk(
                "jwt",
                "secret",
                Medium,
                r"\beyJ[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\.[A-Za-z0-9_-]{5,}\b",
                0,
                false,
            ),
            mk(
                "generic-secret-assignment",
                "secret",
                Medium,
                r#"(?i)\b(password|passwd|secret|api_?key|token)\b\s*[:=]\s*['"]([^'"\n]{8,})['"]"#,
                2,
                true,
            ),
            mk(
                "email-address",
                "pii",
                Low,
                r"\b[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}\b",
                0,
                false,
            ),
            mk(
                "phone-number",
                "pii",
                Low,
                r"(?:\+\d{1,3}[ .-]?)?(?:\(\d{2,4}\)[ .-]?|\d{2,4}[ .-])\d{3,4}[ .-]\d{3,4}\b",
                0,
                false,
            ),
            mk(
                "iban",
                "pii",
                Low,
                r"\b[A-Z]{2}\d{2}(?: ?[A-Z0-9]{4}){3,7}(?: ?[A-Z0-9]{1,4})?\b",
                0,
                false,
            ),
        ]
    })
}

/// Shannon entropy in bits per character.
pub fn entropy(s: &str) -> f64 {
    let mut counts: BTreeMap<char, usize> = BTreeMap::new();
    for c in s.chars() {
        *counts.entry(c).or_default() += 1;
    }
    let n = s.chars().count() as f64;
    if n == 0.0 {
        return 0.0;
    }
    counts
        .values()
        .map(|c| *c as f64 / n)
        .map(|p| -p * p.log2())
        .sum()
}

/// Redact a value: keep the first and last four characters (`AKIA****WXYZ`); short values keep two.
pub fn mask(value: &str) -> String {
    let chars: Vec<char> = value.chars().collect();
    if chars.len() >= 12 {
        format!(
            "{}****{}",
            chars[..4].iter().collect::<String>(),
            chars[chars.len() - 4..].iter().collect::<String>()
        )
    } else if chars.len() >= 6 {
        format!("{}****", chars[..2].iter().collect::<String>())
    } else {
        "****".into()
    }
}

/// IBAN checksum (mod 97) so random digit runs are not reported.
fn iban_ok(s: &str) -> bool {
    let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    if !(15..=34).contains(&compact.len()) {
        return false;
    }
    let rearranged = format!("{}{}", &compact[4..], &compact[..4]);
    let mut rem: u32 = 0;
    for c in rearranged.chars() {
        let v = match c {
            '0'..='9' => c as u32 - '0' as u32,
            'A'..='Z' => c as u32 - 'A' as u32 + 10,
            _ => return false,
        };
        rem = if v >= 10 {
            (rem * 100 + v) % 97
        } else {
            (rem * 10 + v) % 97
        };
    }
    rem == 1
}

fn phone_ok(s: &str) -> bool {
    let digits = s.chars().filter(char::is_ascii_digit).count();
    (9..=15).contains(&digits) && !s.contains("..") && !s.chars().all(|c| c.is_ascii_digit() || c == '.') // dotted numbers are usually versions or IPs
}

fn fixture_path(path: &str) -> bool {
    let p = path.to_lowercase();
    [
        "test", "fixture", "example", "sample", "mock", "dummy", "demo", "docs/", "readme",
    ]
    .iter()
    .any(|w| p.contains(w))
}

fn fixture_value(v: &str) -> bool {
    let l = v.to_lowercase();
    [
        "example",
        "xxxx",
        "your_",
        "your-",
        "changeme",
        "placeholder",
        "dummy",
        "sample",
        "000000",
        "123456",
    ]
    .iter()
    .any(|w| l.contains(w))
}

/// Scan text. `file` is the label used in findings. Never returns a raw value.
pub fn scan_text(file: &str, text: &str) -> Vec<SecretFinding> {
    let mut out = Vec::new();
    let fixture_file = fixture_path(file);
    for (i, line) in text.lines().enumerate() {
        if line.len() > 2000 {
            continue; // minified blobs
        }
        for rule in rules() {
            for caps in rule.re.captures_iter(line) {
                let Some(m) = caps.get(rule.group).or_else(|| caps.get(0)) else {
                    continue;
                };
                let value = m.as_str();
                if rule.entropy && entropy(value) < 3.5 {
                    continue;
                }
                if rule.id == "iban" && !iban_ok(value) {
                    continue;
                }
                if rule.id == "phone-number" && !phone_ok(value) {
                    continue;
                }
                // The OpenAI-style rule also matches Anthropic keys: report each key once.
                if rule.id == "openai-style-key" && value.starts_with("sk-ant-") {
                    continue;
                }
                let masked = if rule.id == "private-key" {
                    "[private key block]".to_owned()
                } else {
                    mask(value)
                };
                out.push(SecretFinding {
                    rule: rule.id.into(),
                    kind: rule.kind.into(),
                    severity: rule.severity,
                    file: file.to_owned(),
                    line: i as u32 + 1,
                    masked,
                    likely_fixture: fixture_file || fixture_value(value),
                });
            }
        }
    }
    out
}

/// `.env` and `.env.local` style files (but not `.env.example`) are findings by themselves.
pub fn env_file_finding(rel: &str, text: &str) -> Option<SecretFinding> {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    let is_env = name == ".env"
        || (name.starts_with(".env.")
            && !matches!(
                name,
                ".env.example" | ".env.sample" | ".env.template" | ".env.dist"
            ));
    if !is_env {
        return None;
    }
    let entries = text
        .lines()
        .filter(|l| l.contains('=') && !l.trim_start().starts_with('#'))
        .count();
    Some(SecretFinding {
        rule: "env-file".into(),
        kind: "env_file".into(),
        severity: Severity::Medium,
        file: rel.to_owned(),
        line: 1,
        masked: format!("{entries} variable{}", if entries == 1 { "" } else { "s" }),
        likely_fixture: false,
    })
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct SecretsReport {
    pub root: String,
    pub files_scanned: u64,
    pub findings: Vec<SecretFinding>,
    /// `true` when the file cap stopped the scan.
    pub truncated: bool,
}

impl SecretsReport {
    pub fn count_by_severity(&self) -> (usize, usize, usize) {
        let c = |s| {
            self.findings
                .iter()
                .filter(|f| f.severity == s && !f.likely_fixture)
                .count()
        };
        (c(Severity::High), c(Severity::Medium), c(Severity::Low))
    }
}

/// Scan a folder tree (hidden files included, since `.env` is hidden). Skips VCS metadata, dependency
/// folders, binaries, and files over 1 MB.
pub fn scan_folder(root: &Path, cancel: &CancellationToken) -> Option<SecretsReport> {
    let mut paths = Vec::new();
    let mut truncated = false;
    let walker = WalkBuilder::new(root)
        .hidden(false)
        .git_ignore(false)
        .git_exclude(false)
        .git_global(false)
        .ignore(false)
        .follow_links(false)
        .filter_entry(|e| {
            !matches!(
                e.file_name().to_str(),
                Some(".git" | "node_modules" | "target" | ".venv" | "__pycache__" | ".svn" | ".hg")
            )
        })
        .build();
    for e in walker.flatten() {
        if cancel.is_cancelled() {
            return None;
        }
        if e.file_type().is_some_and(|t| t.is_file())
            && e.metadata().map(|m| m.len() <= MAX_FILE_BYTES).unwrap_or(false)
        {
            if paths.len() >= MAX_FILES {
                truncated = true;
                break;
            }
            paths.push(e.into_path());
        }
    }
    let results: Vec<(bool, Vec<SecretFinding>)> = paths
        .par_iter()
        .map(|p| {
            if cancel.is_cancelled() {
                return (false, Vec::new());
            }
            let Ok(bytes) = std::fs::read(p) else {
                return (false, Vec::new());
            };
            if bytes.iter().take(8192).any(|b| *b == 0) {
                return (false, Vec::new());
            }
            let text = String::from_utf8_lossy(&bytes);
            let rel = p
                .strip_prefix(root)
                .unwrap_or(p)
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            let mut f = scan_text(&rel, &text);
            f.extend(env_file_finding(&rel, &text));
            (true, f)
        })
        .collect();
    if cancel.is_cancelled() {
        return None;
    }
    let files_scanned = results.iter().filter(|(ok, _)| *ok).count() as u64;
    let mut findings: Vec<SecretFinding> = results.into_iter().flat_map(|(_, f)| f).collect();
    findings.sort_by(|a, b| {
        b.severity
            .cmp(&a.severity)
            .then(a.likely_fixture.cmp(&b.likely_fixture))
            .then_with(|| a.file.cmp(&b.file))
            .then(a.line.cmp(&b.line))
    });
    Some(SecretsReport {
        root: root.display().to_string(),
        files_scanned,
        findings,
        truncated,
    })
}

/// Facts for the LLM: types, files, and counts. Masked values only.
pub fn facts(r: &SecretsReport, shown_root: &str) -> String {
    let mut by_rule: BTreeMap<&str, (usize, usize)> = BTreeMap::new();
    for f in &r.findings {
        let e = by_rule.entry(f.rule.as_str()).or_default();
        e.0 += 1;
        e.1 += usize::from(f.likely_fixture);
    }
    let (hi, med, lo) = r.count_by_severity();
    let mut out = format!(
        "<facts>\nroot: {shown_root}   files scanned: {}   findings: {} (high {hi}, medium {med}, low {lo}, ignoring likely test fixtures)\n",
        r.files_scanned,
        r.findings.len()
    );
    let mut rules: Vec<_> = by_rule.into_iter().collect();
    rules.sort_by_key(|(_, (n, _))| std::cmp::Reverse(*n));
    for (rule, (n, fx)) in rules.iter().take(12) {
        out.push_str(&format!(
            "rule {rule}: {n} finding(s), {fx} in likely test fixtures\n"
        ));
    }
    out.push_str("top findings:\n");
    for f in r.findings.iter().filter(|f| f.severity != Severity::Low).take(15) {
        out.push_str(&format!(
            "- {} at {}:{} value {}{}\n",
            f.rule,
            f.file,
            f.line,
            f.masked,
            if f.likely_fixture {
                " (likely test fixture)"
            } else {
                ""
            }
        ));
    }
    if r.truncated {
        out.push_str("note: the scan stopped at its file limit\n");
    }
    out.push_str("</facts>");
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    // Fake credentials are assembled at run time so this file never contains a complete token.
    fn aws() -> String {
        ["AKIA", "IOSFODNN7", "EXAMPLE"].concat()
    }
    fn gh() -> String {
        format!("ghp_{}", "a1B2c3D4e5F6g7H8i9J0k1L2m3N4o5P6q7R8")
    }
    fn stripe() -> String {
        format!("sk_live_{}", "4eC39HqLyjWDarjtT1zdp7dc")
    }

    #[test]
    fn well_known_formats_are_found_and_masked() {
        let text = format!("aws = {}\nGITHUB_TOKEN={}\nstripe: {}\n", aws(), gh(), stripe());
        let f = scan_text("config.txt", &text);
        let rules: Vec<&str> = f.iter().map(|x| x.rule.as_str()).collect();
        assert_eq!(rules, ["aws-access-key", "github-token", "stripe-live-key"]);
        assert_eq!(f[0].masked, "AKIA****MPLE");
        assert_eq!(f[0].line, 1);
        assert!(f.iter().all(|x| x.severity == Severity::High));
        let json = serde_json::to_string(&f).unwrap();
        for raw in [aws(), gh(), stripe()] {
            assert!(!json.contains(&raw), "a raw secret leaked into the findings");
        }
    }

    #[test]
    fn private_keys_and_jwts() {
        let pem = ["-----BEGIN RSA ", "PRIVATE KEY-----"].concat();
        let jwt = [
            "eyJhbGciOiJIUzI1NiJ9",
            "eyJzdWIiOiIxMjM0NTY3ODkwIn0",
            "dozjgNryP4J3jVmNHl0w5N_XgL0n3I9PlFUP0THsR8U",
        ]
        .join(".");
        let f = scan_text("k.pem", &format!("{pem}\nabc\nbearer {jwt}"));
        assert_eq!(f[0].rule, "private-key");
        assert_eq!(f[0].masked, "[private key block]");
        assert_eq!(f[1].rule, "jwt");
        assert!(!serde_json::to_string(&f).unwrap().contains("dozjgNry"));
    }

    #[test]
    fn generic_assignments_need_entropy() {
        let strong = "password = \"k8#Zq2!vN9xPw4Lm\"";
        let weak = "password = \"aaaaaaaaaaaa\"";
        let short = "password = \"abc\"";
        assert_eq!(scan_text("a.py", strong).len(), 1);
        assert!(scan_text("a.py", weak).is_empty());
        assert!(scan_text("a.py", short).is_empty());
        assert!(entropy("aaaa") < 0.1 && entropy("abcdefgh") > 2.9);
    }

    #[test]
    fn fixtures_are_marked_not_dropped() {
        let f = scan_text("tests/fixtures/keys.txt", &aws());
        assert!(f[0].likely_fixture);
        let f = scan_text(
            "src/main.rs",
            &format!("let k = \"{}\";", ["AKIA", "ABCDEFGHIJKLMNOP"].concat()),
        );
        assert!(!f[0].likely_fixture);
        let f = scan_text("src/main.rs", &format!("let k = \"{}\"; // example", aws()));
        assert!(f[0].likely_fixture, "the value itself says EXAMPLE");
    }

    #[test]
    fn pii_is_low_severity_and_validated() {
        let iban_ok = "DE89 3704 0044 0532 0130 00";
        let iban_bad = "DE00 1234 5678 9012 3456 78";
        let f = scan_text(
            "c.csv",
            &format!(
                "mail me at jane.doe@example.org or call +49 30 1234 5678; {iban_ok}; {iban_bad}; version 1.2.3.4.5"
            ),
        );
        let rules: Vec<&str> = f.iter().map(|x| x.rule.as_str()).collect();
        assert!(
            rules.contains(&"email-address") && rules.contains(&"phone-number") && rules.contains(&"iban"),
            "{rules:?}"
        );
        assert_eq!(
            rules.iter().filter(|r| **r == "iban").count(),
            1,
            "only the valid IBAN"
        );
        assert!(f.iter().all(|x| x.severity == Severity::Low && x.kind == "pii"));
    }

    #[test]
    fn mask_never_reveals_the_middle() {
        assert_eq!(mask("abcdefghijklmnop"), "abcd****mnop");
        assert_eq!(mask("abcdefgh"), "ab****");
        assert_eq!(mask("abc"), "****");
    }

    #[test]
    fn env_files_are_findings() {
        assert_eq!(
            env_file_finding("app/.env", "A=1\n# c\nB=2\n").unwrap().masked,
            "2 variables"
        );
        assert!(env_file_finding(".env.example", "A=1").is_none());
        assert!(env_file_finding("src/env.rs", "A=1").is_none());
        assert!(env_file_finding("x/.env.local", "A=1").is_some());
    }

    #[test]
    fn a_folder_scan_skips_junk_and_never_leaks() {
        let d = tempfile::tempdir().unwrap();
        let w = |rel: &str, data: &[u8]| {
            let p = d.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, data).unwrap();
        };
        w("src/app.js", format!("const k = '{}';", aws()).as_bytes());
        w(".env", format!("TOKEN={}\n", gh()).as_bytes());
        w("node_modules/pkg/x.js", aws().as_bytes());
        w(".git/config", aws().as_bytes());
        w("image.bin", &[0u8; 100]);
        let r = scan_folder(d.path(), &CancellationToken::new()).unwrap();
        let files: Vec<&str> = r.findings.iter().map(|f| f.file.as_str()).collect();
        assert!(files.contains(&"src/app.js") && files.contains(&".env"));
        assert!(
            !files
                .iter()
                .any(|f| f.contains("node_modules") || f.starts_with(".git/"))
        );
        assert_eq!(r.files_scanned, 2);
        let blob = serde_json::to_string(&r).unwrap() + &facts(&r, "~/proj");
        assert!(
            !blob.contains(&aws()) && !blob.contains(&gh()),
            "no raw secret in the report or the LLM facts"
        );
        assert!(facts(&r, "~/proj").contains("aws-access-key at src/app.js:1 value AKIA****MPLE"));
        let c = CancellationToken::new();
        c.cancel();
        assert!(scan_folder(d.path(), &c).is_none());
    }
}
