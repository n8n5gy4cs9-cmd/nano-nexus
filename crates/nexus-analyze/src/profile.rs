//! CSV/TSV and JSON profilers (ANALYZER §2): streaming statistics with bounded memory.

use std::collections::{BTreeMap, HashMap};
use std::io::Read;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::fmt::count;

const MAX_DISTINCT: usize = 50_000;
const MAX_JSON_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum ProfileError {
    #[error("could not read the file: {0}")]
    Io(#[from] std::io::Error),
    #[error("the file is not valid: {0}")]
    Parse(String),
    #[error("the file is larger than {0} MB")]
    TooLarge(u64),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct ColumnProfile {
    pub name: String,
    /// `integer | float | boolean | date | text | empty`
    pub kind: String,
    pub non_null: u64,
    pub nulls: u64,
    pub null_pct: f64,
    /// Distinct values (capped; see `unique_capped`).
    pub unique: u64,
    pub unique_capped: bool,
    pub min: Option<String>,
    pub max: Option<String>,
    pub mean: Option<f64>,
    pub top_values: Vec<TopValue>,
    pub examples: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct TopValue {
    pub value: String,
    pub count: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct CsvProfile {
    pub rows: u64,
    pub delimiter: String,
    pub columns: Vec<ColumnProfile>,
    pub sample_rows: Vec<Vec<String>>,
}

#[derive(Default)]
struct Col {
    name: String,
    non_null: u64,
    nulls: u64,
    ints: u64,
    floats: u64,
    bools: u64,
    dates: u64,
    texts: u64,
    freq: HashMap<String, u64>,
    capped: bool,
    num_min: f64,
    num_max: f64,
    num_sum: f64,
    num_n: u64,
    str_min: Option<String>,
    str_max: Option<String>,
    examples: Vec<String>,
}

fn is_null(v: &str) -> bool {
    matches!(
        v.trim(),
        "" | "NA" | "N/A" | "NaN" | "null" | "NULL" | "None" | "nan" | "-"
    )
}

fn is_date(v: &str) -> bool {
    let b = v.as_bytes();
    b.len() >= 8
        && b[..4].iter().all(u8::is_ascii_digit)
        && b[4] == b'-'
        && b.get(5..7).is_some_and(|m| m.iter().all(u8::is_ascii_digit))
        && b.get(7) == Some(&b'-')
        && b.get(8..10).is_some_and(|d| d.iter().all(u8::is_ascii_digit))
}

impl Col {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            num_min: f64::INFINITY,
            num_max: f64::NEG_INFINITY,
            ..Self::default()
        }
    }

    fn add(&mut self, raw: &str) {
        let v = raw.trim();
        if is_null(v) {
            self.nulls += 1;
            return;
        }
        self.non_null += 1;
        if self.examples.len() < 3 && !self.examples.iter().any(|e| e == v) {
            self.examples.push(v.chars().take(60).collect());
        }
        if let Ok(i) = v.parse::<i64>() {
            self.ints += 1;
            self.num(i as f64);
        } else if let Some(f) = v.parse::<f64>().ok().filter(|f| f.is_finite()) {
            self.floats += 1;
            self.num(f);
        } else if matches!(v.to_ascii_lowercase().as_str(), "true" | "false") {
            self.bools += 1;
        } else if is_date(v) {
            self.dates += 1;
            self.str_range(v);
        } else {
            self.texts += 1;
            self.str_range(v);
        }
        if let Some(c) = self.freq.get_mut(v) {
            *c += 1;
        } else if self.freq.len() < MAX_DISTINCT {
            self.freq.insert(v.chars().take(80).collect(), 1);
        } else {
            self.capped = true;
        }
    }

    fn num(&mut self, f: f64) {
        self.num_min = self.num_min.min(f);
        self.num_max = self.num_max.max(f);
        self.num_sum += f;
        self.num_n += 1;
    }

    fn str_range(&mut self, v: &str) {
        if self.str_min.as_deref().is_none_or(|m| v < m) {
            self.str_min = Some(v.chars().take(60).collect());
        }
        if self.str_max.as_deref().is_none_or(|m| v > m) {
            self.str_max = Some(v.chars().take(60).collect());
        }
    }

    fn finish(self, rows: u64) -> ColumnProfile {
        let total_typed = self.non_null.max(1) as f64;
        let numeric = self.ints + self.floats;
        let kind = if self.non_null == 0 {
            "empty"
        } else if numeric as f64 / total_typed >= 0.95 {
            if self.floats > 0 { "float" } else { "integer" }
        } else if self.bools as f64 / total_typed >= 0.95 {
            "boolean"
        } else if self.dates as f64 / total_typed >= 0.95 {
            "date"
        } else {
            "text"
        };
        let (min, max) = match kind {
            "integer" | "float" if self.num_n > 0 => {
                (Some(fmt_num(self.num_min)), Some(fmt_num(self.num_max)))
            }
            _ => (self.str_min.clone(), self.str_max.clone()),
        };
        let mean =
            (matches!(kind, "integer" | "float") && self.num_n > 0).then(|| self.num_sum / self.num_n as f64);
        let mut top: Vec<TopValue> = self
            .freq
            .iter()
            .map(|(value, count)| TopValue {
                value: value.clone(),
                count: *count,
            })
            .collect();
        top.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.value.cmp(&b.value)));
        top.truncate(5);
        ColumnProfile {
            name: self.name,
            kind: kind.into(),
            non_null: self.non_null,
            nulls: self.nulls,
            null_pct: if rows == 0 {
                0.0
            } else {
                self.nulls as f64 * 100.0 / rows as f64
            },
            unique: self.freq.len() as u64,
            unique_capped: self.capped,
            min,
            max,
            mean,
            top_values: top,
            examples: self.examples,
        }
    }
}

fn fmt_num(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{f:.0}")
    } else {
        format!("{f}")
    }
}

/// Pick `,`, `;`, tab, or `|` from the first lines.
pub fn sniff_delimiter(head: &str, ext: &str) -> u8 {
    if ext == "tsv" || ext == "tab" {
        return b'\t';
    }
    let lines: Vec<&str> = head.lines().take(10).collect();
    b",;\t|"
        .iter()
        .copied()
        .max_by_key(|d| {
            let counts: Vec<usize> = lines
                .iter()
                .map(|l| l.bytes().filter(|b| b == d).count())
                .collect();
            let first = counts.first().copied().unwrap_or(0);
            if first == 0 {
                0
            } else {
                counts.iter().filter(|c| **c == first).count() * 1000 + first
            }
        })
        .unwrap_or(b',')
}

/// Profile CSV/TSV text from `reader`.
pub fn profile_csv_reader<R: Read>(reader: R, delimiter: u8) -> Result<CsvProfile, ProfileError> {
    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delimiter)
        .flexible(true)
        .has_headers(true)
        .from_reader(reader);
    let headers: Vec<String> = rdr
        .headers()
        .map_err(|e| ProfileError::Parse(e.to_string()))?
        .iter()
        .map(|h| h.trim().to_owned())
        .collect();
    let mut cols: Vec<Col> = headers
        .iter()
        .enumerate()
        .map(|(i, h)| {
            Col::new(&if h.is_empty() {
                format!("column {}", i + 1)
            } else {
                h.clone()
            })
        })
        .collect();
    let mut rows = 0u64;
    let mut sample_rows = Vec::new();
    for rec in rdr.records() {
        let Ok(rec) = rec else { continue };
        rows += 1;
        if sample_rows.len() < 5 {
            sample_rows.push(rec.iter().map(|v| v.chars().take(60).collect()).collect());
        }
        for (i, col) in cols.iter_mut().enumerate() {
            col.add(rec.get(i).unwrap_or(""));
        }
    }
    Ok(CsvProfile {
        rows,
        delimiter: if delimiter == b'\t' {
            "tab".into()
        } else {
            (delimiter as char).to_string()
        },
        columns: cols.into_iter().map(|c| c.finish(rows)).collect(),
        sample_rows,
    })
}

pub fn profile_csv(path: &Path) -> Result<CsvProfile, ProfileError> {
    let mut head = vec![0u8; 8192];
    let n = std::fs::File::open(path)?.read(&mut head)?;
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let delim = sniff_delimiter(&String::from_utf8_lossy(&head[..n]), &ext);
    profile_csv_reader(std::io::BufReader::new(std::fs::File::open(path)?), delim)
}

pub fn csv_facts(p: &CsvProfile, name: &str) -> String {
    let mut o = format!(
        "<facts>\nfile: {name}   rows: {}   columns: {}   delimiter: {}\n",
        count(p.rows),
        p.columns.len(),
        p.delimiter
    );
    for c in p.columns.iter().take(40) {
        o.push_str(&format!(
            "- {} ({}): {} filled, {:.0}% empty, {}{} distinct",
            c.name,
            c.kind,
            count(c.non_null),
            c.null_pct,
            count(c.unique),
            if c.unique_capped { "+" } else { "" }
        ));
        if let (Some(min), Some(max)) = (&c.min, &c.max) {
            o.push_str(&format!(", range {min} to {max}"));
        }
        if let Some(m) = c.mean {
            o.push_str(&format!(", mean {m:.2}"));
        }
        if !c.top_values.is_empty() && c.kind == "text" || c.unique <= 8 {
            o.push_str(&format!(
                ", top: {}",
                c.top_values
                    .iter()
                    .take(3)
                    .map(|t| format!("{} ×{}", t.value, count(t.count)))
                    .collect::<Vec<_>>()
                    .join("; ")
            ));
        }
        o.push('\n');
    }
    if p.columns.len() > 40 {
        o.push_str(&format!("(and {} more columns)\n", p.columns.len() - 40));
    }
    o.push_str("sample rows:\n");
    for r in &p.sample_rows {
        o.push_str(&format!("  {}\n", r.join(" | ")));
    }
    o.push_str("</facts>");
    o
}

// ---- JSON -------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct KeyStat {
    pub key: String,
    pub present: u64,
    /// Most common value type: `string | number | boolean | null | object | array`.
    pub kind: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct JsonProfile {
    /// `object | array | scalar | lines`
    pub shape: String,
    pub depth: u32,
    /// Items in the top-level array (or lines, or keys of an object).
    pub items: u64,
    pub top_keys: Vec<KeyStat>,
    pub max_array_len: u64,
    pub total_values: u64,
}

fn kind_of(v: &serde_json::Value) -> &'static str {
    use serde_json::Value as V;
    match v {
        V::Null => "null",
        V::Bool(_) => "boolean",
        V::Number(_) => "number",
        V::String(_) => "string",
        V::Array(_) => "array",
        V::Object(_) => "object",
    }
}

fn walk(v: &serde_json::Value, depth: u32, max_depth: &mut u32, max_arr: &mut u64, total: &mut u64) {
    *total += 1;
    *max_depth = (*max_depth).max(depth);
    match v {
        serde_json::Value::Array(a) => {
            *max_arr = (*max_arr).max(a.len() as u64);
            for x in a {
                walk(x, depth + 1, max_depth, max_arr, total);
            }
        }
        serde_json::Value::Object(o) => {
            for x in o.values() {
                walk(x, depth + 1, max_depth, max_arr, total);
            }
        }
        _ => {}
    }
}

fn key_stats<'a>(objs: impl Iterator<Item = &'a serde_json::Map<String, serde_json::Value>>) -> Vec<KeyStat> {
    let mut m: BTreeMap<String, (u64, BTreeMap<&'static str, u64>)> = BTreeMap::new();
    for o in objs {
        for (k, v) in o {
            let e = m.entry(k.clone()).or_default();
            e.0 += 1;
            *e.1.entry(kind_of(v)).or_default() += 1;
        }
    }
    let mut v: Vec<KeyStat> = m
        .into_iter()
        .map(|(key, (present, kinds))| KeyStat {
            key,
            present,
            kind: kinds
                .into_iter()
                .max_by_key(|(_, n)| *n)
                .map_or("null", |(k, _)| k)
                .into(),
        })
        .collect();
    v.sort_by(|a, b| b.present.cmp(&a.present).then_with(|| a.key.cmp(&b.key)));
    v.truncate(20);
    v
}

pub fn profile_json_text(text: &str, lines_mode: bool) -> Result<JsonProfile, ProfileError> {
    let (mut depth, mut max_arr, mut total) = (0, 0, 0);
    if lines_mode {
        let vals: Vec<serde_json::Value> = text
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str(l).map_err(|e| ProfileError::Parse(e.to_string())))
            .collect::<Result<_, _>>()?;
        for v in &vals {
            walk(v, 1, &mut depth, &mut max_arr, &mut total);
        }
        let top_keys = key_stats(vals.iter().filter_map(|v| v.as_object()));
        return Ok(JsonProfile {
            shape: "lines".into(),
            depth,
            items: vals.len() as u64,
            top_keys,
            max_array_len: max_arr,
            total_values: total,
        });
    }
    let v: serde_json::Value = serde_json::from_str(text).map_err(|e| ProfileError::Parse(e.to_string()))?;
    walk(&v, 1, &mut depth, &mut max_arr, &mut total);
    let (shape, items, top_keys) = match &v {
        serde_json::Value::Array(a) => (
            "array",
            a.len() as u64,
            key_stats(a.iter().filter_map(|x| x.as_object())),
        ),
        serde_json::Value::Object(o) => ("object", o.len() as u64, key_stats(std::iter::once(o))),
        _ => ("scalar", 1, Vec::new()),
    };
    Ok(JsonProfile {
        shape: shape.into(),
        depth,
        items,
        top_keys,
        max_array_len: max_arr,
        total_values: total,
    })
}

pub fn profile_json(path: &Path) -> Result<JsonProfile, ProfileError> {
    if std::fs::metadata(path)?.len() > MAX_JSON_BYTES {
        return Err(ProfileError::TooLarge(MAX_JSON_BYTES / 1024 / 1024));
    }
    let text = std::fs::read_to_string(path)?;
    let lines = path.extension().is_some_and(|e| {
        matches!(
            e.to_string_lossy().to_ascii_lowercase().as_str(),
            "jsonl" | "ndjson"
        )
    });
    profile_json_text(&text, lines)
}

pub fn json_facts(p: &JsonProfile, name: &str) -> String {
    let mut o = format!(
        "<facts>\nfile: {name}   shape: {}   items: {}   depth: {}   values: {}   longest array: {}\n",
        p.shape,
        count(p.items),
        p.depth,
        count(p.total_values),
        count(p.max_array_len)
    );
    if !p.top_keys.is_empty() {
        o.push_str("keys:\n");
        for k in &p.top_keys {
            o.push_str(&format!(
                "- {} ({}) in {} item(s)\n",
                k.key,
                k.kind,
                count(k.present)
            ));
        }
    }
    o.push_str("</facts>");
    o
}

#[cfg(test)]
mod tests {
    use super::*;

    fn csv(text: &str) -> CsvProfile {
        profile_csv_reader(std::io::Cursor::new(text.to_owned()), b',').unwrap()
    }

    #[test]
    fn infers_types_and_stats() {
        let p = csv(
            "id,price,active,joined,name,empty\n1,9.5,true,2026-01-02,Ann,\n2,10.5,false,2026-03-04,Bob,\n3,,true,2026-02-01,Ann,\n4,20,true,,Cy,NA\n",
        );
        assert_eq!(p.rows, 4);
        let c = |n: &str| p.columns.iter().find(|c| c.name == n).unwrap();
        assert_eq!(c("id").kind, "integer");
        assert_eq!(
            (c("id").min.as_deref(), c("id").max.as_deref(), c("id").mean),
            (Some("1"), Some("4"), Some(2.5))
        );
        assert_eq!(c("price").kind, "float");
        assert_eq!((c("price").nulls, c("price").null_pct), (1, 25.0));
        assert_eq!(c("active").kind, "boolean");
        assert_eq!(c("joined").kind, "date");
        assert_eq!(
            (c("joined").min.as_deref(), c("joined").max.as_deref()),
            (Some("2026-01-02"), Some("2026-03-04"))
        );
        assert_eq!(c("name").kind, "text");
        assert_eq!(c("name").unique, 3);
        assert_eq!(
            c("name").top_values[0],
            TopValue {
                value: "Ann".into(),
                count: 2
            }
        );
        assert_eq!(c("empty").kind, "empty");
        assert_eq!(p.sample_rows.len(), 4);
    }

    #[test]
    fn ragged_rows_quotes_and_delimiters() {
        let p = profile_csv_reader(std::io::Cursor::new("a;b\n1;\"x;y\"\n2\n3;z;extra\n"), b';').unwrap();
        assert_eq!(p.rows, 3);
        assert_eq!(p.columns.len(), 2);
        assert_eq!(p.columns[1].non_null, 2);
        assert_eq!(sniff_delimiter("a;b;c\n1;2;3\n", "csv"), b';');
        assert_eq!(sniff_delimiter("a,b,c\n1,2,3\n", "csv"), b',');
        assert_eq!(sniff_delimiter("a\tb\n1\t2\n", "txt"), b'\t');
        assert_eq!(sniff_delimiter("whatever", "tsv"), b'\t');
        assert_eq!(sniff_delimiter("a|b|c\n1|2|3\n", "csv"), b'|');
    }

    #[test]
    fn big_files_stay_bounded() {
        let mut s = String::from("id,code\n");
        for i in 0..200_000 {
            s.push_str(&format!("{i},c{i}\n"));
        }
        let p = csv(&s);
        assert_eq!(p.rows, 200_000);
        assert!(p.columns[1].unique_capped && p.columns[1].unique as usize <= MAX_DISTINCT);
        assert_eq!(p.columns[0].max.as_deref(), Some("199999"));
    }

    #[test]
    fn files_on_disk_sniff_their_delimiter() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("x.tsv");
        std::fs::write(&p, "a\tb\n1\t2\n").unwrap();
        let prof = profile_csv(&p).unwrap();
        assert_eq!((prof.delimiter.as_str(), prof.columns.len()), ("tab", 2));
        assert!(profile_csv(&d.path().join("nope.csv")).is_err());
    }

    #[test]
    fn csv_facts_are_readable() {
        let f = csv_facts(&csv("id,city\n1,Paris\n2,Paris\n3,Rome\n"), "cities.csv");
        assert!(f.contains("rows: 3   columns: 2") && f.contains("- city (text): 3 filled, 0% empty, 2 distinct, range Paris to Rome, top: Paris ×2; Rome ×1"), "{f}");
        assert!(f.contains("1 | Paris"));
    }

    #[test]
    fn json_shapes() {
        let p = profile_json_text(
            r#"[{"id":1,"name":"a","tags":["x","y","z"]},{"id":2,"name":null},{"id":3}]"#,
            false,
        )
        .unwrap();
        assert_eq!(
            (p.shape.as_str(), p.items, p.depth, p.max_array_len),
            ("array", 3, 4, 3)
        );
        assert_eq!(
            p.top_keys[0],
            KeyStat {
                key: "id".into(),
                present: 3,
                kind: "number".into()
            }
        );
        assert!(
            p.top_keys
                .iter()
                .any(|k| k.key == "tags" && k.present == 1 && k.kind == "array")
        );
        let o = profile_json_text(r#"{"a":{"b":{"c":1}},"d":[1,2]}"#, false).unwrap();
        assert_eq!((o.shape.as_str(), o.items, o.depth), ("object", 2, 4));
        let l = profile_json_text("{\"x\":1}\n{\"x\":2,\"y\":3}\n\n", true).unwrap();
        assert_eq!((l.shape.as_str(), l.items), ("lines", 2));
        assert_eq!(l.top_keys[0].present, 2);
        assert!(profile_json_text("{broken", false).is_err());
        assert!(json_facts(&p, "d.json").contains("- id (number) in 3 item(s)"));
    }
}
