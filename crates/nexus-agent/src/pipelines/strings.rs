//! `strings_improve` (ANALYZER §5.8): better user-facing text and/or i18n keys.
//!
//! Items go to the model in batches of at most 20 as constrained JSON. Rust validates every answer
//! (placeholders identical, `max_len`, CONSTANT_CASE keys, keys unique). Bad ones get one reask, then the
//! text stays as it is and the key comes from the first words of the text.

use std::collections::{HashMap, HashSet};

use nexus_analyze::code::strings::{StringItem, fallback_key, validate};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{JsonCheck, PipelineCx, PipelineError, llm_schema, render_prompt};

/// Strings per model call.
pub const BATCH: usize = 20;

/// What to produce.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    Improve,
    Keys,
    #[default]
    Both,
}

impl Mode {
    fn as_str(self) -> &'static str {
        match self {
            Mode::Improve => "improve",
            Mode::Keys => "keys",
            Mode::Both => "both",
        }
    }
    fn wants_text(self) -> bool {
        self != Mode::Keys
    }
    fn wants_key(self) -> bool {
        self != Mode::Improve
    }
}

#[derive(Clone, Debug, Default)]
pub struct StringsOptions {
    pub mode: Mode,
    /// "friendly", "formal", …
    pub tone: Option<String>,
    pub max_len: Option<usize>,
    /// Keys that already exist in the project's translation files.
    pub used_keys: HashSet<String>,
}

/// Where a proposal came from.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Source {
    Model,
    /// The model failed; the text is unchanged and the key comes from the text.
    Fallback,
}

/// One result, for the item with the same `id`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct StringProposal {
    pub id: u32,
    /// `None` = keep the text.
    pub improved: Option<String>,
    pub key: Option<String>,
    pub source: Source,
}

#[derive(Debug, Deserialize)]
struct Reply {
    items: Vec<Entry>,
}

#[derive(Debug, Deserialize)]
struct Entry {
    id: String,
    #[serde(default)]
    improved: Option<String>,
    #[serde(default)]
    key: Option<String>,
}

fn batch_schema(ids: &[String]) -> Value {
    let mut schema = llm_schema(include_str!(
        "../../../../schemas/llm/strings-proposals.schema.json"
    ));
    if let Some(p) = schema.pointer_mut("/properties/items/items/properties/id") {
        p["enum"] = json!(ids);
    }
    if let Some(p) = schema.pointer_mut("/properties/items") {
        p["maxItems"] = json!(ids.len());
    }
    schema
}

fn unique(base: String, used: &HashSet<String>) -> String {
    if !used.contains(&base) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}_{n}"))
        .find(|k| !used.contains(k))
        .unwrap_or(base)
}

/// Propose text and keys for `items` (strings that already go through a translation call are skipped).
pub async fn propose(
    cx: &PipelineCx<'_>,
    items: &[StringItem],
    opts: &StringsOptions,
    progress: &(dyn Fn(usize, usize) + Sync),
) -> Result<Vec<StringProposal>, PipelineError> {
    let todo: Vec<&StringItem> = items.iter().filter(|i| !i.localized).collect();
    let mut used = opts.used_keys.clone();
    let mut done: HashMap<u32, StringProposal> = HashMap::new();

    for batch in todo.chunks(BATCH) {
        cx.check_cancel()?;
        cx.say(format!("Improving {} strings…", batch.len()));
        let mut pending: Vec<&StringItem> = batch.to_vec();
        let mut problems: Vec<String> = Vec::new();
        for attempt in 0..2 {
            if pending.is_empty() {
                break;
            }
            let locals: Vec<String> = (1..=pending.len()).map(|i| format!("s{i}")).collect();
            let rows: Vec<Value> = pending
                .iter()
                .zip(&locals)
                .map(|(it, id)| {
                    json!({
                        "id": id, "text": it.text, "context": it.context,
                        "placeholders": it.placeholders, "max_len": opts.max_len,
                    })
                })
                .collect();
            let vars = json!({"mode": opts.mode.as_str(), "tone": opts.tone, "items": rows});
            let (mut prompt, sampling) = render_prompt("strings_improve", &vars)?;
            if attempt > 0 && !problems.is_empty() {
                prompt.user.push_str(&format!(
                    "\nYour previous answer had problems:\n{}\nFix them.\n",
                    problems.join("\n")
                ));
            }
            let reply = cx
                .llm
                .json(
                    prompt,
                    &batch_schema(&locals),
                    sampling,
                    &JsonCheck {
                        allowed_ids: locals.clone(),
                        ..JsonCheck::default()
                    },
                    cx.cancel,
                )
                .await
                .and_then(|v| serde_json::from_value::<Reply>(v).ok());
            let mut by_local: HashMap<String, Entry> = HashMap::new();
            for e in reply.into_iter().flat_map(|r| r.items) {
                by_local.entry(e.id.clone()).or_insert(e);
            }
            let mut failed: Vec<&StringItem> = Vec::new();
            problems.clear();
            for (it, local) in pending.iter().zip(&locals) {
                let Some(e) = by_local.remove(local) else {
                    failed.push(it);
                    problems.push(format!("- {local}: no answer was returned"));
                    continue;
                };
                let improved = e
                    .improved
                    .map(|t| t.trim().to_owned())
                    .filter(|t| opts.mode.wants_text() && !t.is_empty() && *t != it.text);
                let key = e.key.filter(|_| opts.mode.wants_key());
                if opts.mode.wants_key() && key.is_none() {
                    failed.push(it);
                    problems.push(format!("- {local}: a key is required"));
                    continue;
                }
                match validate(it, improved.as_deref(), key.as_deref(), opts.max_len, &used) {
                    Ok(()) => {
                        if let Some(k) = &key {
                            used.insert(k.clone());
                        }
                        done.insert(
                            it.id,
                            StringProposal {
                                id: it.id,
                                improved,
                                key,
                                source: Source::Model,
                            },
                        );
                    }
                    Err(why) => {
                        failed.push(it);
                        problems.push(format!("- {local}: {why}"));
                    }
                }
            }
            pending = failed;
            progress(done.len(), todo.len());
        }
        for it in pending {
            let key = opts.mode.wants_key().then(|| {
                let k = unique(fallback_key(&it.text), &used);
                used.insert(k.clone());
                k
            });
            done.insert(
                it.id,
                StringProposal {
                    id: it.id,
                    improved: None,
                    key,
                    source: Source::Fallback,
                },
            );
        }
        progress(done.len(), todo.len());
    }
    Ok(todo.iter().filter_map(|i| done.remove(&i.id)).collect())
}

#[cfg(test)]
mod tests {
    use super::super::testkit::gate_for;
    use super::*;
    use crate::internal::{PromptSampling, RenderedPrompt};
    use async_trait::async_trait;
    use std::path::Path;
    use std::sync::Mutex;
    use tokio_util::sync::CancellationToken;

    /// Answers with `answer(text, call_number)` → `(improved, key)` for every `<str>` in the prompt.
    type Answer = Box<dyn Fn(&str, usize) -> (Option<String>, Option<String>) + Send + Sync>;

    struct Scripted {
        answer: Answer,
        calls: Mutex<usize>,
        prompts: Mutex<Vec<String>>,
    }

    #[async_trait]
    impl super::super::JsonLlm for Scripted {
        async fn json(
            &self,
            prompt: RenderedPrompt,
            _schema: &Value,
            _s: PromptSampling,
            _c: &JsonCheck,
            _x: &CancellationToken,
        ) -> Option<Value> {
            let call = {
                let mut c = self.calls.lock().unwrap();
                *c += 1;
                *c
            };
            let mut out = Vec::new();
            for block in prompt.user.split("<str ").skip(1) {
                let id = block.split('"').nth(1).unwrap_or("");
                let text = block
                    .split_once('>')
                    .and_then(|(_, r)| r.split("</str>").next())
                    .unwrap_or("");
                let (improved, key) = (self.answer)(text, call);
                out.push(json!({"id": id, "improved": improved, "key": key}));
            }
            self.prompts.lock().unwrap().push(prompt.user);
            Some(json!({"items": out}))
        }
    }

    fn item(id: u32, text: &str) -> StringItem {
        StringItem {
            id,
            file: 0,
            path: "a.py".into(),
            line: 1,
            start: 0,
            end: 0,
            text: text.into(),
            placeholders: nexus_analyze::code::strings::placeholders(text),
            context: "label".into(),
            localized: false,
        }
    }

    async fn run(llm: &Scripted, items: &[StringItem], opts: &StringsOptions) -> Vec<StringProposal> {
        let gate = gate_for(Path::new("/"));
        let cancel = CancellationToken::new();
        let say = |_: String| {};
        let cx = PipelineCx {
            llm,
            gate: &gate,
            cancel: &cancel,
            status: &say,
            home: None,
        };
        propose(&cx, items, opts, &|_, _| {}).await.unwrap()
    }

    fn scripted(
        a: impl Fn(&str, usize) -> (Option<String>, Option<String>) + Send + Sync + 'static,
    ) -> Scripted {
        Scripted {
            answer: Box::new(a),
            calls: Mutex::new(0),
            prompts: Mutex::new(Vec::new()),
        }
    }

    #[tokio::test]
    async fn improves_text_and_keys_and_skips_localized() {
        let llm = scripted(|t, _| match t {
            "Strt gme" => (Some("Start game".into()), Some("MENU_START_GAME".into())),
            _ => (Some(t.to_owned()), Some("HUD_SCORE".into())),
        });
        let mut loc = item(2, "Hello");
        loc.localized = true;
        let r = run(
            &llm,
            &[item(0, "Strt gme"), item(1, "Score: %d"), loc],
            &StringsOptions::default(),
        )
        .await;
        assert_eq!(r.len(), 2);
        assert_eq!(r[0].improved.as_deref(), Some("Start game"));
        assert_eq!(r[0].key.as_deref(), Some("MENU_START_GAME"));
        assert_eq!(r[1].improved, None, "unchanged text is not a change");
        assert_eq!(*llm.calls.lock().unwrap(), 1);
    }

    #[tokio::test]
    async fn broken_placeholders_and_duplicate_keys_get_one_reask() {
        let llm = scripted(|t, call| match (t, call) {
            ("Score: %d", 1) => (Some("Score".into()), Some("KEY_ONE".into())),
            ("Score: %d", _) => (Some("Points: %d".into()), Some("HUD_SCORE".into())),
            (_, 1) => (None, Some("KEY_ONE".into())),
            (_, _) => (None, Some("MENU_TITLE".into())),
        });
        let r = run(
            &llm,
            &[item(0, "Score: %d"), item(1, "Main menu")],
            &StringsOptions::default(),
        )
        .await;
        assert_eq!(r[0].improved.as_deref(), Some("Points: %d"));
        assert_eq!(r[0].key.as_deref(), Some("HUD_SCORE"));
        assert_eq!(
            r[1].key.as_deref(),
            Some("KEY_ONE"),
            "the first valid use of a key wins"
        );
        assert_eq!(*llm.calls.lock().unwrap(), 2);
        assert!(llm.prompts.lock().unwrap()[1].contains("placeholders must stay"));
        assert!(r.iter().all(|p| p.source == Source::Model));
    }

    #[tokio::test]
    async fn falls_back_to_the_text_and_a_key_from_the_words() {
        let llm = scripted(|_, _| (Some("no".into()), Some("bad key".into())));
        let r = run(
            &llm,
            &[item(0, "Start game now"), item(1, "Start game now")],
            &StringsOptions {
                used_keys: ["X".to_owned()].into(),
                ..StringsOptions::default()
            },
        )
        .await;
        assert_eq!(r[0].key.as_deref(), Some("START_GAME_NOW"));
        assert_eq!(r[1].key.as_deref(), Some("START_GAME_NOW_2"));
        assert!(
            r.iter()
                .all(|p| p.improved.is_none() && p.source == Source::Fallback)
        );
    }

    #[tokio::test]
    async fn improve_mode_needs_no_key() {
        let llm = scripted(|t, _| (Some(format!("{t}!")), Some("IGNORED".into())));
        let r = run(
            &llm,
            &[item(0, "Save")],
            &StringsOptions {
                mode: Mode::Improve,
                max_len: Some(10),
                ..StringsOptions::default()
            },
        )
        .await;
        assert_eq!(r[0].improved.as_deref(), Some("Save!"));
        assert_eq!(r[0].key, None);
    }
}
