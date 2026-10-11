//! The folder report model and the pure aggregation that builds it from scanned entries.

use std::collections::{BTreeMap, HashMap};

use serde::{Deserialize, Serialize};

use crate::categories::Category;
use crate::junk::JunkHit;
use crate::naming::NamingHit;

const DAY_MS: i64 = 86_400_000;

/// One scanned file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileEntry {
    /// Relative to the scan root, `/` separators.
    pub rel: String,
    pub bytes: u64,
    /// Unix milliseconds.
    pub modified: Option<i64>,
    pub category: Category,
    /// Lowercase extension without the dot (empty if none).
    pub ext: String,
    pub hidden: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct FolderTotals {
    pub files: u64,
    pub dirs: u64,
    pub bytes: u64,
    pub hidden_files: u64,
    pub symlinks: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct CategoryStat {
    pub category: Category,
    pub files: u64,
    pub bytes: u64,
    pub pct_bytes: f64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ExtStat {
    pub ext: String,
    pub files: u64,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct FileRef {
    pub rel_path: String,
    pub bytes: u64,
    pub modified: Option<i64>,
    pub category: Category,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct DirRef {
    pub rel_path: String,
    pub bytes: u64,
    pub files: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct AgeStat {
    pub files: u64,
    pub bytes: u64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct AgeBuckets {
    pub last_7d: AgeStat,
    pub last_30d: AgeStat,
    pub last_365d: AgeStat,
    pub older: AgeStat,
    pub unknown: AgeStat,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct DupGroup {
    pub bytes_each: u64,
    pub hash: String,
    pub paths: Vec<String>,
    /// `false` when only the size and first 64 KiB matched (fast mode).
    pub confirmed: bool,
}

impl DupGroup {
    pub fn wasted(&self) -> u64 {
        self.bytes_each * (self.paths.len() as u64).saturating_sub(1)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct ScanWarning {
    pub path: String,
    pub error: String,
}

/// A node of the pruned treemap.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct TreeNode {
    pub name: String,
    pub rel_path: String,
    pub bytes: u64,
    pub files: u64,
    /// Dominant category (by bytes) for color.
    pub category: Category,
    /// Several small items merged together.
    pub other: bool,
    pub children: Vec<TreeNode>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct FolderReport {
    pub id: String,
    pub root: String,
    /// Unix milliseconds.
    pub scanned_at: i64,
    pub duration_ms: u64,
    pub totals: FolderTotals,
    pub by_category: Vec<CategoryStat>,
    pub by_extension: Vec<ExtStat>,
    pub largest_files: Vec<FileRef>,
    pub largest_dirs: Vec<DirRef>,
    pub age: AgeBuckets,
    pub duplicates: Vec<DupGroup>,
    pub wasted_by_duplicates: u64,
    pub empty_dirs: Vec<String>,
    pub empty_files: Vec<String>,
    pub junk: Vec<JunkHit>,
    pub naming_issues: Vec<NamingHit>,
    pub max_depth: u32,
    pub deepest_path: String,
    pub treemap: TreeNode,
    pub warnings: Vec<ScanWarning>,
    pub truncated: bool,
}

// ---- aggregation ------------------------------------------------------------------------------

pub fn by_category(files: &[FileEntry]) -> Vec<CategoryStat> {
    let total: u64 = files.iter().map(|f| f.bytes).sum();
    let mut m: BTreeMap<Category, (u64, u64)> = BTreeMap::new();
    for f in files {
        let e = m.entry(f.category).or_default();
        e.0 += 1;
        e.1 += f.bytes;
    }
    let mut v: Vec<CategoryStat> = m
        .into_iter()
        .map(|(category, (files, bytes))| CategoryStat {
            category,
            files,
            bytes,
            pct_bytes: if total == 0 {
                0.0
            } else {
                bytes as f64 * 100.0 / total as f64
            },
        })
        .collect();
    v.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.category.cmp(&b.category)));
    v
}

pub fn by_extension(files: &[FileEntry], top: usize) -> Vec<ExtStat> {
    let mut m: HashMap<&str, (u64, u64)> = HashMap::new();
    for f in files {
        let e = m.entry(f.ext.as_str()).or_default();
        e.0 += 1;
        e.1 += f.bytes;
    }
    let mut v: Vec<ExtStat> = m
        .into_iter()
        .map(|(ext, (files, bytes))| ExtStat {
            ext: if ext.is_empty() {
                "(none)".into()
            } else {
                ext.to_owned()
            },
            files,
            bytes,
        })
        .collect();
    v.sort_by(|a, b| b.bytes.cmp(&a.bytes).then(a.ext.cmp(&b.ext)));
    v.truncate(top);
    v
}

pub fn largest_files(files: &[FileEntry], top: usize) -> Vec<FileRef> {
    let mut idx: Vec<&FileEntry> = files.iter().collect();
    idx.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.rel.cmp(&b.rel)));
    idx.into_iter()
        .take(top)
        .map(|f| FileRef {
            rel_path: f.rel.clone(),
            bytes: f.bytes,
            modified: f.modified,
            category: f.category,
        })
        .collect()
}

/// Recursive size and file count of every directory (the root, `""`, is included).
pub fn dir_sizes(files: &[FileEntry]) -> HashMap<String, (u64, u64)> {
    let mut m: HashMap<String, (u64, u64)> = HashMap::new();
    for f in files {
        let mut end = f.rel.len();
        loop {
            let dir = match f.rel[..end].rfind('/') {
                Some(i) => &f.rel[..i],
                None => "",
            };
            let e = m.entry(dir.to_owned()).or_default();
            e.0 += f.bytes;
            e.1 += 1;
            if dir.is_empty() {
                break;
            }
            end = dir.len();
        }
    }
    m
}

pub fn largest_dirs(sizes: &HashMap<String, (u64, u64)>, top: usize) -> Vec<DirRef> {
    let mut v: Vec<DirRef> = sizes
        .iter()
        .filter(|(p, _)| !p.is_empty())
        .map(|(p, (b, n))| DirRef {
            rel_path: p.clone(),
            bytes: *b,
            files: *n,
        })
        .collect();
    v.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.rel_path.cmp(&b.rel_path)));
    v.truncate(top);
    v
}

pub fn age_buckets(files: &[FileEntry], now_ms: i64) -> AgeBuckets {
    let mut a = AgeBuckets::default();
    for f in files {
        let slot = match f.modified {
            None => &mut a.unknown,
            Some(m) => match (now_ms - m) / DAY_MS {
                d if d <= 7 => &mut a.last_7d,
                d if d <= 30 => &mut a.last_30d,
                d if d <= 365 => &mut a.last_365d,
                _ => &mut a.older,
            },
        };
        slot.files += 1;
        slot.bytes += f.bytes;
    }
    a
}

// ---- treemap ----------------------------------------------------------------------------------

#[derive(Default)]
struct Arena {
    names: Vec<String>,
    paths: Vec<String>,
    children: Vec<Vec<usize>>,
    bytes: Vec<u64>,
    files: Vec<u64>,
    cat_bytes: Vec<[u64; 17]>,
    is_file: Vec<bool>,
}

impl Arena {
    fn push(&mut self, name: &str, path: String, is_file: bool) -> usize {
        self.names.push(name.to_owned());
        self.paths.push(path);
        self.children.push(Vec::new());
        self.bytes.push(0);
        self.files.push(0);
        self.cat_bytes.push([0; 17]);
        self.is_file.push(is_file);
        self.names.len() - 1
    }
}

fn cat_index(c: Category) -> usize {
    Category::ALL.iter().position(|x| *x == c).unwrap_or(16)
}

fn dominant(c: &[u64; 17]) -> Category {
    let (i, _) = c.iter().enumerate().max_by_key(|(_, b)| **b).unwrap_or((16, &0));
    Category::ALL[i]
}

fn convert(a: &Arena, i: usize, min_bytes: u64, count: &mut usize) -> TreeNode {
    *count += 1;
    let mut kids: Vec<usize> = a.children[i].clone();
    kids.sort_by(|x, y| {
        a.bytes[*y]
            .cmp(&a.bytes[*x])
            .then_with(|| a.names[*x].cmp(&a.names[*y]))
    });
    let mut children = Vec::new();
    let (mut other_bytes, mut other_files, mut other_n) = (0u64, 0u64, 0u64);
    let mut other_cats = [0u64; 17];
    for k in kids {
        if a.bytes[k] < min_bytes {
            other_bytes += a.bytes[k];
            other_files += a.files[k];
            other_n += 1;
            for (o, c) in other_cats.iter_mut().zip(a.cat_bytes[k].iter()) {
                *o += c;
            }
        } else {
            children.push(convert(a, k, min_bytes, count));
        }
    }
    if other_n > 0 {
        *count += 1;
        children.push(TreeNode {
            name: format!("other ({other_n} items)"),
            rel_path: a.paths[i].clone(),
            bytes: other_bytes,
            files: other_files,
            category: dominant(&other_cats),
            other: true,
            children: Vec::new(),
        });
    }
    TreeNode {
        name: a.names[i].clone(),
        rel_path: a.paths[i].clone(),
        bytes: a.bytes[i],
        files: a.files[i],
        category: dominant(&a.cat_bytes[i]),
        other: false,
        children,
    }
}

/// Build the treemap: items under 0.5% of the total merge into "other", and the whole tree is pruned
/// further until it has at most `max_nodes` nodes.
pub fn treemap(root_name: &str, files: &[FileEntry], max_nodes: usize) -> TreeNode {
    let mut a = Arena::default();
    let root = a.push(root_name, String::new(), false);
    let mut index: HashMap<String, usize> = HashMap::new();
    index.insert(String::new(), root);
    for f in files {
        let mut parent = root;
        let mut path = String::new();
        let parts: Vec<&str> = f.rel.split('/').collect();
        let cat = cat_index(f.category);
        a.bytes[root] += f.bytes;
        a.files[root] += 1;
        a.cat_bytes[root][cat] += f.bytes;
        for (n, part) in parts.iter().enumerate() {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(part);
            let is_file = n + 1 == parts.len();
            let id = match index.get(&path) {
                Some(id) => *id,
                None => {
                    let id = a.push(part, path.clone(), is_file);
                    a.children[parent].push(id);
                    index.insert(path.clone(), id);
                    id
                }
            };
            if !is_file || n + 1 == parts.len() {
                a.bytes[id] += f.bytes;
                a.files[id] += 1;
                a.cat_bytes[id][cat] += f.bytes;
            }
            parent = id;
        }
    }
    let total = a.bytes[root].max(1);
    let mut min = (total / 200).max(1); // 0.5%
    loop {
        let mut count = 0;
        let t = convert(&a, root, min, &mut count);
        if count <= max_nodes || min >= total {
            return t;
        }
        min = min.saturating_mul(3) / 2 + 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn f(rel: &str, bytes: u64, cat: Category, modified: Option<i64>) -> FileEntry {
        FileEntry {
            rel: rel.into(),
            bytes,
            modified,
            category: cat,
            ext: rel.rsplit_once('.').map(|x| x.1.to_owned()).unwrap_or_default(),
            hidden: false,
        }
    }

    fn sample() -> Vec<FileEntry> {
        vec![
            f("Movies/talk.mov", 6_000, Category::Video, Some(1_000)),
            f("Movies/clip.mp4", 3_000, Category::Video, Some(2_000)),
            f("docs/a.pdf", 500, Category::Pdf, None),
            f("docs/deep/b.pdf", 400, Category::Pdf, None),
            f("readme.md", 100, Category::Document, None),
        ]
    }

    #[test]
    fn category_and_extension_stats() {
        let s = by_category(&sample());
        assert_eq!(
            (s[0].category, s[0].bytes, s[0].files),
            (Category::Video, 9_000, 2)
        );
        assert!((s[0].pct_bytes - 90.0).abs() < 1e-9);
        let e = by_extension(&sample(), 2);
        assert_eq!(e.len(), 2);
        assert_eq!(e[0].ext, "mov");
        assert_eq!(
            by_extension(&[f("Makefile", 1, Category::Code, None)], 5)[0].ext,
            "(none)"
        );
    }

    #[test]
    fn largest_files_and_dirs() {
        let l = largest_files(&sample(), 2);
        assert_eq!(
            (l[0].rel_path.as_str(), l[1].rel_path.as_str()),
            ("Movies/talk.mov", "Movies/clip.mp4")
        );
        let sizes = dir_sizes(&sample());
        assert_eq!(sizes[""], (10_000, 5));
        assert_eq!(sizes["docs"], (900, 2));
        assert_eq!(sizes["docs/deep"], (400, 1));
        let d = largest_dirs(&sizes, 2);
        assert_eq!((d[0].rel_path.as_str(), d[0].bytes), ("Movies", 9_000));
        assert!(d.iter().all(|x| !x.rel_path.is_empty()));
    }

    #[test]
    fn age_buckets_split_by_days() {
        let now = 400 * DAY_MS;
        let mk = |days: i64| f("x", 10, Category::Other, Some(now - days * DAY_MS));
        let a = age_buckets(
            &[
                mk(1),
                mk(7),
                mk(8),
                mk(30),
                mk(31),
                mk(365),
                mk(366),
                f("u", 5, Category::Other, None),
            ],
            now,
        );
        assert_eq!(
            (
                a.last_7d.files,
                a.last_30d.files,
                a.last_365d.files,
                a.older.files,
                a.unknown.files
            ),
            (2, 2, 2, 1, 1)
        );
        assert_eq!(a.unknown.bytes, 5);
    }

    #[test]
    fn treemap_merges_small_items_and_keeps_totals() {
        let mut files = sample();
        for i in 0..50 {
            files.push(f(&format!("tiny/f{i}.txt"), 1, Category::Document, None));
        }
        let t = treemap("root", &files, 2000);
        assert_eq!(t.bytes, 10_050);
        assert_eq!(t.files, 55);
        assert_eq!(t.category, Category::Video);
        let movies = t.children.iter().find(|c| c.name == "Movies").unwrap();
        assert_eq!(movies.children.len(), 2);
        let tiny_total: u64 = t
            .children
            .iter()
            .filter(|c| c.name == "tiny" || c.other)
            .map(|c| c.bytes)
            .sum();
        assert!(tiny_total >= 50, "small stuff is merged, not lost");
        fn sum(n: &TreeNode) -> u64 {
            if n.children.is_empty() {
                n.bytes
            } else {
                n.children.iter().map(sum).sum()
            }
        }
        assert_eq!(sum(&t), t.bytes, "children add up to the parent");
    }

    #[test]
    fn treemap_node_count_is_capped() {
        let files: Vec<FileEntry> = (0..5000)
            .map(|i| {
                f(
                    &format!("d{}/f{i}.bin", i % 50),
                    1000 + i as u64,
                    Category::Data,
                    None,
                )
            })
            .collect();
        fn count(n: &TreeNode) -> usize {
            1 + n.children.iter().map(count).sum::<usize>()
        }
        let t = treemap("r", &files, 300);
        assert!(count(&t) <= 300, "{}", count(&t));
        assert_eq!(t.bytes, files.iter().map(|x| x.bytes).sum::<u64>());
    }

    #[test]
    fn duplicate_waste() {
        let g = DupGroup {
            bytes_each: 100,
            hash: "h".into(),
            paths: vec!["a".into(), "b".into(), "c".into()],
            confirmed: true,
        };
        assert_eq!(g.wasted(), 200);
    }
}
