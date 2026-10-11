//! `tidy_plan`: sort loose files in a folder into subfolders, as a reviewable move plan (never applied here).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use nexus_analyze::categories::{Category, categorize};
use nexus_core::changes::{ChangeSet, FileOp};
use nexus_core::ids::ChangeSetId;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    Artifact, JsonCheck, PipelineCx, PipelineError, PipelineOutput, authorize_read, llm_schema, render_prompt,
};

/// Files changed in the last day are left alone (someone is probably using them).
const RECENT_MS: i64 = 86_400_000;
/// Ambiguous files sent to the model in one call.
const BATCH: usize = 30;
/// The folders a tidy plan may create.
pub const TARGETS: [(&str, &str); 7] = [
    ("Images", "photos, screenshots, graphics"),
    ("Documents", "PDFs, Word, spreadsheets, slides, notes, ebooks"),
    ("Installers", "app installers and disk images"),
    ("Archives", "zip and other compressed files"),
    ("Audio", "music, recordings, podcasts"),
    ("Video", "movies, clips, screen recordings"),
    ("Code", "source code and project files"),
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Candidate {
    pub name: String,
    pub bytes: u64,
    pub modified: Option<i64>,
    pub category: Category,
}

/// Deterministic target for a category, or `None` when the file is ambiguous (the model may help).
pub fn rule_target(c: Category) -> Option<&'static str> {
    Some(match c {
        Category::Image => "Images",
        Category::Video => "Video",
        Category::Audio => "Audio",
        Category::Document | Category::Pdf | Category::Spreadsheet | Category::Presentation => "Documents",
        Category::Archive => "Archives",
        Category::Installer => "Installers",
        _ => return None,
    })
}

#[derive(Debug, Deserialize)]
struct Assignments {
    assignments: Vec<Assignment>,
}

#[derive(Debug, Deserialize)]
struct Assignment {
    id: String,
    target: String,
}

fn list_loose_files(root: &Path, now_ms: i64) -> (Vec<Candidate>, usize) {
    let mut out = Vec::new();
    let mut recent = 0;
    let Ok(rd) = std::fs::read_dir(root) else {
        return (out, 0);
    };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        let Ok(ft) = e.file_type() else { continue };
        if !ft.is_file() || ft.is_symlink() || name.starts_with('.') || name.starts_with("~$") {
            continue;
        }
        let Ok(meta) = e.metadata() else { continue };
        let modified = meta
            .modified()
            .ok()
            .and_then(|m| m.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64);
        if modified.is_some_and(|m| now_ms - m < RECENT_MS) {
            recent += 1;
            continue;
        }
        out.push(Candidate {
            category: categorize(&e.path(), None),
            name,
            bytes: meta.len(),
            modified,
        });
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    (out, recent)
}

fn sample(root: &Path, name: &str) -> String {
    use std::io::Read;
    let mut buf = vec![0u8; 400];
    let Ok(mut f) = std::fs::File::open(root.join(name)) else {
        return String::new();
    };
    let n = f.read(&mut buf).unwrap_or(0);
    if buf[..n].contains(&0) {
        return String::new();
    }
    String::from_utf8_lossy(&buf[..n])
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(160)
        .collect()
}

/// A name for `name` inside `dir` that does not exist yet (`a.pdf`, `a (2).pdf`, ...).
fn free_name(dir: &Path, name: &str, taken: &mut std::collections::HashSet<PathBuf>) -> PathBuf {
    let (stem, ext) = name
        .rsplit_once('.')
        .filter(|(s, _)| !s.is_empty())
        .map_or((name, ""), |(s, e)| (s, e));
    let mut cand = dir.join(name);
    let mut n = 2;
    while cand.exists() || taken.contains(&cand) {
        cand = dir.join(if ext.is_empty() {
            format!("{stem} ({n})")
        } else {
            format!("{stem} ({n}).{ext}")
        });
        n += 1;
    }
    taken.insert(cand.clone());
    cand
}

pub async fn tidy_plan(
    cx: &PipelineCx<'_>,
    root: PathBuf,
    now_ms: i64,
) -> Result<PipelineOutput, PipelineError> {
    let root = authorize_read(cx, &root, "to plan how to tidy this folder").await?;
    if !root.is_dir() {
        return Err(PipelineError::Input("Pick a folder to tidy.".into()));
    }
    cx.say("Looking at the files…");
    let r2 = root.clone();
    let (files, recent) = tokio::task::spawn_blocking(move || list_loose_files(&r2, now_ms))
        .await
        .map_err(|e| PipelineError::Failed(e.to_string()))?;
    if files.is_empty() {
        return Ok(PipelineOutput::text(
            "There is nothing to tidy here: no loose files older than a day.",
            Artifact::None,
        ));
    }

    let mut assigned: BTreeMap<&'static str, Vec<&Candidate>> = BTreeMap::new();
    let mut ambiguous: Vec<&Candidate> = Vec::new();
    for f in &files {
        match rule_target(f.category) {
            Some(t) => assigned.entry(t).or_default().push(f),
            None => ambiguous.push(f),
        }
    }
    let mut left: Vec<&Candidate> = Vec::new();
    let mut by_model = 0usize;
    if !ambiguous.is_empty() {
        let schema = llm_schema(include_str!("../../../../schemas/llm/tidy-classify.schema.json"));
        let targets: Vec<Value> = TARGETS
            .iter()
            .map(|(n, d)| json!({"name": n, "description": d}))
            .collect();
        for (b, batch) in ambiguous.chunks(BATCH).enumerate() {
            cx.check_cancel()?;
            cx.say(format!("Sorting the unclear ones… ({})", b + 1));
            let items: Vec<Value> = batch
                .iter()
                .enumerate()
                .map(|(i, f)| json!({"id": format!("f{i}"), "name": f.name, "ext": f.name.rsplit_once('.').map_or("", |x| x.1), "size": nexus_analyze::fmt::bytes(f.bytes), "modified": f.modified.map_or("unknown".into(), nexus_analyze::fmt::date), "sample": sample(&root, &f.name)}))
                .collect();
            let (prompt, sampling) =
                render_prompt("tidy_classify", &json!({"targets": targets, "files": items}))?;
            let ids: Vec<String> = (0..batch.len()).map(|i| format!("f{i}")).collect();
            let reply = cx
                .llm
                .json(
                    prompt,
                    &schema,
                    sampling,
                    &JsonCheck {
                        allowed_ids: ids,
                        ..JsonCheck::default()
                    },
                    cx.cancel,
                )
                .await
                .and_then(|v| serde_json::from_value::<Assignments>(v).ok());
            let mut chosen: BTreeMap<usize, &str> = BTreeMap::new();
            if let Some(a) = &reply {
                for x in &a.assignments {
                    if let Some(i) =
                        x.id.strip_prefix('f')
                            .and_then(|n| n.parse::<usize>().ok())
                            .filter(|i| *i < batch.len())
                        && let Some((t, _)) = TARGETS.iter().find(|(n, _)| *n == x.target)
                    {
                        chosen.insert(i, t);
                    }
                }
            }
            for (i, f) in batch.iter().enumerate() {
                match chosen.get(&i) {
                    Some(t) => {
                        assigned.entry(t).or_default().push(f);
                        by_model += 1;
                    }
                    None => left.push(f),
                }
            }
        }
    }

    // Build the operations.
    let mut ops = Vec::new();
    let mut taken = std::collections::HashSet::new();
    let mut summary: Vec<String> = Vec::new();
    for (target, list) in &assigned {
        let dir = root.join(target);
        if !dir.exists() {
            ops.push(FileOp::Mkdir {
                path: dir.display().to_string(),
            });
        }
        for f in list {
            let to = free_name(&dir, &f.name, &mut taken);
            ops.push(FileOp::Move {
                from: root.join(&f.name).display().to_string(),
                to: to.display().to_string(),
            });
        }
        summary.push(format!(
            "- **{target}/**: {} file{}",
            list.len(),
            if list.len() == 1 { "" } else { "s" }
        ));
    }
    let moves = ops.iter().filter(|o| matches!(o, FileOp::Move { .. })).count();
    if moves == 0 {
        return Ok(PipelineOutput::text(
            "I couldn't find a safe place for any of these files, so I'm leaving them where they are.",
            Artifact::None,
        ));
    }
    let name = root.file_name().map_or_else(
        || root.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let plan = ChangeSet {
        id: ChangeSetId::new(),
        title: format!("Tidy {name}"),
        origin: "tidy".into(),
        ops,
        created_at: now_ms,
    };
    let mut text = format!(
        "I planned **{moves}** move{} in **{name}**. Nothing has changed yet: review the plan in Changes.\n\n{}",
        if moves == 1 { "" } else { "s" },
        summary.join("\n")
    );
    if !left.is_empty() {
        text.push_str(&format!(
            "\n\nLeft in place ({}): {}",
            left.len(),
            left.iter()
                .take(8)
                .map(|f| f.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if recent > 0 {
        text.push_str(&format!(
            "\n\n{recent} file{} changed in the last day, so I left {} alone.",
            if recent == 1 { "" } else { "s" },
            if recent == 1 { "it" } else { "them" }
        ));
    }
    let mut out = PipelineOutput::text(text, Artifact::Plan(Box::new(plan)));
    if by_model > 0 {
        out.notes.push(format!(
            "The model helped place {by_model} unclear file{}.",
            if by_model == 1 { "" } else { "s" }
        ));
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::super::testkit::{Scripted, gate_for, write};
    use super::*;
    use std::sync::Mutex;
    use tokio_util::sync::CancellationToken;

    const NOW: i64 = 4_000_000_000_000;

    fn cx<'a>(
        llm: &'a Scripted,
        gate: &'a crate::tools::EngineGatekeeper,
        cancel: &'a CancellationToken,
    ) -> PipelineCx<'a> {
        let log: &'static Mutex<Vec<String>> = Box::leak(Box::new(Mutex::new(vec![])));
        PipelineCx {
            llm,
            gate,
            cancel,
            status: Box::leak(Box::new(move |s: String| log.lock().unwrap().push(s))),
            home: None,
        }
    }

    fn moves(out: &PipelineOutput) -> Vec<(String, String)> {
        match &out.artifact {
            Artifact::Plan(p) => p
                .ops
                .iter()
                .filter_map(|o| {
                    if let FileOp::Move { from, to } = o {
                        Some((
                            from.rsplit('/').next().unwrap().to_owned(),
                            to.rsplit('/')
                                .take(2)
                                .collect::<Vec<_>>()
                                .into_iter()
                                .rev()
                                .collect::<Vec<_>>()
                                .join("/"),
                        ))
                    } else {
                        None
                    }
                })
                .collect(),
            _ => vec![],
        }
    }

    #[test]
    fn rules() {
        assert_eq!(rule_target(Category::Pdf), Some("Documents"));
        assert_eq!(rule_target(Category::Installer), Some("Installers"));
        assert_eq!(rule_target(Category::Other), None);
        assert_eq!(rule_target(Category::Code), None);
    }

    #[tokio::test]
    async fn rules_place_the_obvious_files_and_the_model_the_rest() {
        let d = tempfile::tempdir().unwrap();
        for f in [
            "photo.jpg",
            "report.pdf",
            "setup.dmg",
            "song.mp3",
            "mystery.xyz",
            "build.sh",
            "notes",
        ] {
            write(d.path(), f, b"data");
        }
        let llm = Scripted::new(vec![Some(
            json!({"assignments": [{"id": "f0", "target": "Code"}, {"id": "f1", "target": "leave"}, {"id": "f2", "target": "Documents"}]}),
        )]);
        let gate = gate_for(d.path());
        let cancel = CancellationToken::new();
        let out = tidy_plan(&cx(&llm, &gate, &cancel), d.path().to_owned(), NOW)
            .await
            .unwrap();
        let m = moves(&out);
        assert!(
            m.contains(&("photo.jpg".into(), "Images/photo.jpg".into()))
                && m.contains(&("report.pdf".into(), "Documents/report.pdf".into()))
                && m.contains(&("setup.dmg".into(), "Installers/setup.dmg".into()))
                && m.contains(&("song.mp3".into(), "Audio/song.mp3".into()))
        );
        // Ambiguous files sorted by name: build.sh (f0) -> Code, mystery.xyz (f1) -> leave, notes (f2) -> Documents.
        assert!(
            m.contains(&("build.sh".into(), "Code/build.sh".into()))
                && m.contains(&("notes".into(), "Documents/notes".into()))
        );
        assert!(!m.iter().any(|(f, _)| f == "mystery.xyz"));
        assert!(
            out.text
                .as_ref()
                .unwrap()
                .contains("Left in place (1): mystery.xyz")
        );
        assert_eq!(out.notes.len(), 1);
        let Artifact::Plan(p) = &out.artifact else {
            panic!()
        };
        assert_eq!(p.origin, "tidy");
        assert!(
            p.ops.iter().filter(|o| matches!(o, FileOp::Mkdir { .. })).count() >= 4,
            "new folders are created first"
        );
        // Nothing moved on disk.
        assert!(d.path().join("photo.jpg").exists());
    }

    #[tokio::test]
    async fn model_failure_means_leave_in_place() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "a.pdf", b"x");
        write(d.path(), "weird.bin2", b"x");
        let llm = Scripted::new(vec![None]);
        let (gate, cancel) = (gate_for(d.path()), CancellationToken::new());
        let out = tidy_plan(&cx(&llm, &gate, &cancel), d.path().to_owned(), NOW)
            .await
            .unwrap();
        assert_eq!(moves(&out).len(), 1);
        assert!(out.text.unwrap().contains("Left in place (1): weird.bin2"));
    }

    #[tokio::test]
    async fn names_never_collide_and_subfolders_and_recent_files_are_untouched() {
        let d = tempfile::tempdir().unwrap();
        write(d.path(), "a.pdf", b"x");
        write(d.path(), "Documents/a.pdf", b"already here");
        write(d.path(), "sub/inner.pdf", b"x");
        write(d.path(), ".hidden.pdf", b"x");
        let llm = Scripted::new(vec![]);
        let (gate, cancel) = (gate_for(d.path()), CancellationToken::new());
        let out = tidy_plan(&cx(&llm, &gate, &cancel), d.path().to_owned(), NOW)
            .await
            .unwrap();
        assert_eq!(
            moves(&out),
            [("a.pdf".to_string(), "Documents/a (2).pdf".to_string())]
        );
        let Artifact::Plan(p) = &out.artifact else {
            panic!()
        };
        assert!(
            !p.ops.iter().any(|o| matches!(o, FileOp::Mkdir { .. })),
            "Documents already exists"
        );
        // With "now" right after the files were written, everything counts as recent.
        let real_now = std::time::SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        let out = tidy_plan(&cx(&llm, &gate, &cancel), d.path().to_owned(), real_now)
            .await
            .unwrap();
        assert!(out.stream.is_none() && out.text.unwrap().contains("nothing to tidy"));
    }
}
