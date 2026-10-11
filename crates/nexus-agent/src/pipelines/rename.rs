//! `rename_propose` (ANALYZER §5.6): turn naming issues into validated rename proposals.
//!
//! 1. Issues that are only about case are fixed by rule (no model call).
//! 2. The rest go to the model in batches of at most 20 symbols of one kind, language, and convention,
//!    as constrained JSON with the convention's regex.
//! 3. Rust validates every answer (keywords, builtins, convention, collisions). Bad ones get one reask;
//!    then the deterministic suggestion, or `needs_manual`.

use std::collections::HashMap;

use nexus_analyze::code::CodeIndex;
use nexus_analyze::code::SymbolId;
use nexus_analyze::code::propose::{
    Proposal, ProposalSource, ProposalStatus, Sources, Validator, example_for, render_sym,
};
use nexus_analyze::code::rules::{Candidate, Case};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{JsonCheck, PipelineCx, PipelineError, llm_schema, render_prompt};

/// Symbols per model call.
pub const BATCH: usize = 20;

/// Knobs for one proposal run.
#[derive(Clone, Debug, Default)]
pub struct ProposeOptions {
    /// Words the project prefers (`health`, not `hp`).
    pub glossary: Vec<String>,
    /// Names the user already rejected, per symbol (never proposed again).
    pub rejected: HashMap<SymbolId, Vec<String>>,
    /// "Try again": temperature +0.2 and no rule shortcut.
    pub try_again: bool,
}

#[derive(Debug, Deserialize)]
struct Reply {
    proposals: Vec<Item>,
}

#[derive(Debug, Deserialize)]
struct Item {
    id: String,
    new_name: String,
    #[serde(default)]
    confidence: f32,
    #[serde(default)]
    reason: String,
}

/// What a run produced.
#[derive(Clone, Debug, PartialEq)]
pub struct ProposeResult {
    pub proposals: Vec<Proposal>,
    pub model_calls: usize,
}

fn batch_schema(ids: &[String], pattern: &str) -> Value {
    let mut schema = llm_schema(include_str!(
        "../../../../schemas/llm/rename-proposals.schema.json"
    ));
    if let Some(items) = schema.pointer_mut("/properties/proposals/items/properties") {
        items["id"]["enum"] = json!(ids);
        items["new_name"]["pattern"] = json!(pattern);
    }
    if let Some(p) = schema.pointer_mut("/properties/proposals") {
        p["maxItems"] = json!(ids.len());
    }
    schema
}

fn proposal(
    c: &Candidate,
    old: &str,
    new_name: Option<String>,
    confidence: f32,
    reason: &str,
    status: ProposalStatus,
    source: ProposalSource,
) -> Proposal {
    Proposal {
        symbol: c.symbol,
        old_name: old.to_owned(),
        new_name,
        confidence,
        reason: reason.to_owned(),
        status,
        source,
        issues: c.issues.iter().map(|i| i.rule.clone()).collect(),
    }
}

/// Propose names for `cands`. `progress(done, total)` is called as batches finish.
pub async fn propose(
    cx: &PipelineCx<'_>,
    index: &CodeIndex,
    cands: &[Candidate],
    opts: &ProposeOptions,
    progress: &(dyn Fn(usize, usize) + Sync),
) -> Result<ProposeResult, PipelineError> {
    let validator = Validator::new(index);
    let sources = Sources::new(index);
    let mut taken: HashMap<SymbolId, String> = HashMap::new();
    let mut done: HashMap<SymbolId, Proposal> = HashMap::new();
    let old_of = |c: &Candidate| index.symbol(c.symbol).map(|s| s.name.clone()).unwrap_or_default();

    // 1. Case-only fixes by rule.
    let mut remaining: Vec<&Candidate> = Vec::new();
    for c in cands {
        let old = old_of(c);
        let ruled = (!opts.try_again).then(|| c.suggested.clone()).flatten();
        if let Some(name) = ruled
            && !opts.rejected.get(&c.symbol).is_some_and(|r| r.contains(&name))
            && validator.check(c.symbol, &name, &taken).is_ok()
        {
            taken.insert(c.symbol, name.clone());
            done.insert(
                c.symbol,
                proposal(
                    c,
                    &old,
                    Some(name),
                    0.95,
                    "follows the naming convention",
                    ProposalStatus::Proposed,
                    ProposalSource::Rule,
                ),
            );
        } else {
            remaining.push(c);
        }
    }
    progress(done.len(), cands.len());

    // 2. Model batches, grouped by (language, kind, convention).
    type GroupKey = (String, String, Case);
    let mut groups: Vec<(GroupKey, Vec<&Candidate>)> = Vec::new();
    for c in remaining {
        let Some(s) = index.symbol(c.symbol) else { continue };
        let key = (
            s.lang
                .map(|l| l.name().to_owned())
                .unwrap_or_else(|| "any".into()),
            s.kind.label().to_owned(),
            c.convention.primary(),
        );
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, v)) => v.push(c),
            None => groups.push((key, vec![c])),
        }
    }
    let mut model_calls = 0;
    for ((lang, kind, case), group) in groups {
        for batch in group.chunks(BATCH) {
            cx.check_cancel()?;
            cx.say(format!("Proposing names for {} {kind}s…", batch.len()));
            let mut pending: Vec<&Candidate> = batch.to_vec();
            let mut problems: Vec<String> = Vec::new();
            for attempt in 0..2 {
                if pending.is_empty() {
                    break;
                }
                let locals: Vec<String> = (1..=pending.len()).map(|i| format!("s{i}")).collect();
                let blocks: Vec<String> = pending
                    .iter()
                    .zip(&locals)
                    .map(|(c, id)| render_sym(index, &sources, c, id))
                    .collect();
                let rejected: Vec<Value> = pending
                    .iter()
                    .zip(&locals)
                    .flat_map(|(c, id)| {
                        opts.rejected
                            .get(&c.symbol)
                            .into_iter()
                            .flatten()
                            .map(move |n| json!({"id": id, "name": n}))
                    })
                    .collect();
                let vars = json!({
                    "lang": lang, "kind": kind, "convention": case.label(), "convention_example": example_for(case),
                    "glossary": opts.glossary, "rejected": rejected, "symbols": blocks,
                });
                let (mut prompt, mut sampling) = render_prompt("rename_propose", &vars)?;
                if opts.try_again {
                    sampling.temperature = (sampling.temperature + 0.2).min(0.9);
                }
                if attempt > 0 && !problems.is_empty() {
                    prompt.user.push_str(&format!(
                        "\nYour previous answer had problems:\n{}\nFix them.\n",
                        problems.join("\n")
                    ));
                }
                let schema = batch_schema(&locals, case.pattern());
                model_calls += 1;
                let reply = cx
                    .llm
                    .json(
                        prompt,
                        &schema,
                        sampling,
                        &JsonCheck {
                            allowed_ids: locals.clone(),
                            ..JsonCheck::default()
                        },
                        cx.cancel,
                    )
                    .await
                    .and_then(|v| serde_json::from_value::<Reply>(v).ok());
                let mut by_local: HashMap<String, Item> = HashMap::new();
                if let Some(r) = reply {
                    for it in r.proposals {
                        by_local.entry(it.id.clone()).or_insert(it);
                    }
                }
                let mut failed: Vec<&Candidate> = Vec::new();
                problems.clear();
                for (c, local) in pending.iter().zip(&locals) {
                    let old = old_of(c);
                    let Some(it) = by_local.remove(local) else {
                        failed.push(c);
                        problems.push(format!("- {local}: no proposal was returned"));
                        continue;
                    };
                    let name = it.new_name.trim().to_owned();
                    if name == old {
                        done.insert(
                            c.symbol,
                            proposal(
                                c,
                                &old,
                                None,
                                it.confidence.clamp(0.0, 1.0),
                                "already clear",
                                ProposalStatus::Unchanged,
                                ProposalSource::Model,
                            ),
                        );
                        continue;
                    }
                    let rejected_before = opts.rejected.get(&c.symbol).is_some_and(|r| r.contains(&name));
                    let verdict = if rejected_before {
                        Err("you were told not to propose this name again".to_owned())
                    } else {
                        validator.check(c.symbol, &name, &taken)
                    };
                    match verdict {
                        Ok(()) => {
                            taken.insert(c.symbol, name.clone());
                            let reason: String = it.reason.chars().take(120).collect();
                            done.insert(
                                c.symbol,
                                proposal(
                                    c,
                                    &old,
                                    Some(name),
                                    it.confidence.clamp(0.0, 1.0),
                                    &reason,
                                    ProposalStatus::Proposed,
                                    ProposalSource::Model,
                                ),
                            );
                        }
                        Err(why) => {
                            failed.push(c);
                            problems.push(format!("- {local} → `{name}`: {why}"));
                        }
                    }
                }
                pending = failed;
                progress(done.len(), cands.len());
            }
            // 3. Fallbacks for what is still open.
            for c in pending {
                let old = old_of(c);
                let fallback = c
                    .suggested
                    .clone()
                    .filter(|n| validator.check(c.symbol, n, &taken).is_ok());
                match fallback {
                    Some(n) => {
                        taken.insert(c.symbol, n.clone());
                        done.insert(
                            c.symbol,
                            proposal(
                                c,
                                &old,
                                Some(n),
                                0.5,
                                "the model could not help; case fixed by rule",
                                ProposalStatus::Proposed,
                                ProposalSource::Fallback,
                            ),
                        );
                    }
                    None => {
                        done.insert(
                            c.symbol,
                            proposal(
                                c,
                                &old,
                                None,
                                0.0,
                                "needs a name from you",
                                ProposalStatus::NeedsManual,
                                ProposalSource::Fallback,
                            ),
                        );
                    }
                }
            }
        }
    }
    progress(done.len(), cands.len());
    let proposals = cands.iter().filter_map(|c| done.remove(&c.symbol)).collect();
    Ok(ProposeResult {
        proposals,
        model_calls,
    })
}

#[cfg(test)]
mod tests {
    use super::super::testkit::gate_for;
    use super::*;
    use crate::internal::{PromptSampling, RenderedPrompt};
    use async_trait::async_trait;
    use nexus_analyze::code::index::{IndexOptions, build_index};
    use nexus_analyze::code::rules::{RuleOptions, detect};
    use std::path::Path;
    use std::sync::Mutex;
    use tokio_util::sync::CancellationToken;

    /// Answers by looking at the prompt: `answer(old_name, call_number)` gives the new name for the symbol
    /// whose `<sym>` block mentions that old name, or `None` to leave it out.
    type Answer = Box<dyn Fn(&str, usize) -> Option<String> + Send + Sync>;

    struct Smart {
        names: Vec<&'static str>,
        answer: Answer,
        prompts: Mutex<Vec<RenderedPrompt>>,
        calls: Mutex<HashMap<String, usize>>,
    }

    impl Smart {
        fn new(
            names: &[&'static str],
            answer: impl Fn(&str, usize) -> Option<String> + Send + Sync + 'static,
        ) -> Self {
            Self {
                names: names.to_vec(),
                answer: Box::new(answer),
                prompts: Mutex::new(Vec::new()),
                calls: Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl super::super::JsonLlm for Smart {
        async fn json(
            &self,
            prompt: RenderedPrompt,
            _schema: &Value,
            _s: PromptSampling,
            _c: &JsonCheck,
            _x: &CancellationToken,
        ) -> Option<Value> {
            let mut out = Vec::new();
            for block in prompt.user.split("<sym ").skip(1) {
                let id = block.split('"').nth(1).unwrap_or("").to_owned();
                let head = block.lines().nth(1).unwrap_or("");
                for n in &self.names {
                    if head.contains(n) {
                        let nth = {
                            let mut c = self.calls.lock().unwrap();
                            let e = c.entry((*n).to_owned()).or_default();
                            *e += 1;
                            *e
                        };
                        if let Some(new) = (self.answer)(n, nth) {
                            out.push(json!({"id": id, "new_name": new, "confidence": 0.9, "reason": "fits"}));
                        }
                    }
                }
            }
            self.prompts.lock().unwrap().push(prompt);
            Some(json!({"proposals": out}))
        }
    }

    fn fixture_index(name: &str) -> CodeIndex {
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures")
            .join(name);
        build_index(&p, &IndexOptions::default(), None, &CancellationToken::new()).unwrap()
    }

    async fn run(index: &CodeIndex, llm: &Smart, opts: &ProposeOptions) -> ProposeResult {
        let cands = detect(index, &RuleOptions::default());
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
        propose(&cx, index, &cands, opts, &|_, _| {}).await.unwrap()
    }

    fn find<'a>(r: &'a ProposeResult, old: &str) -> &'a Proposal {
        r.proposals
            .iter()
            .find(|p| p.old_name == old)
            .unwrap_or_else(|| panic!("no proposal for {old}"))
    }

    #[tokio::test]
    async fn case_fixes_need_no_model_and_the_rest_is_validated() {
        let idx = fixture_index("py_app");
        let llm = Smart::new(&["doStuff2", "tmp"], |old, _| {
            Some(
                if old == "tmp" {
                    "discounted_price"
                } else {
                    "apply_discount"
                }
                .to_owned(),
            )
        });
        let r = run(&idx, &llm, &ProposeOptions::default()).await;
        let class = find(&r, "cart_item");
        assert_eq!(
            (class.new_name.as_deref(), class.source),
            (Some("CartItem"), ProposalSource::Rule)
        );
        assert_eq!(find(&r, "Helper_Funcs").new_name.as_deref(), Some("helper_funcs"));
        let m = find(&r, "doStuff2");
        assert_eq!(
            (m.new_name.as_deref(), m.source),
            (Some("apply_discount"), ProposalSource::Model)
        );
        assert_eq!(find(&r, "tmp").new_name.as_deref(), Some("discounted_price"));
        // Case-only symbols never reached the model.
        let prompts = llm.prompts.lock().unwrap();
        assert!(
            prompts
                .iter()
                .all(|p| !p.user.contains("Helper_Funcs") && !p.user.contains("def __init__"))
        );
        // Batches hold one kind each and name the convention.
        assert!(prompts.iter().all(|p| p.system.contains("snake_case")));
    }

    #[tokio::test]
    async fn invalid_answers_get_one_reask_then_needs_manual() {
        let idx = fixture_index("py_app");
        let llm = Smart::new(&["doStuff2"], |_, nth| {
            Some(if nth == 1 { "lambda" } else { "print" }.to_owned())
        });
        let r = run(&idx, &llm, &ProposeOptions::default()).await;
        let m = find(&r, "doStuff2");
        assert_eq!(m.status, ProposalStatus::NeedsManual);
        assert!(m.new_name.is_none());
        let prompts = llm.prompts.lock().unwrap();
        assert!(
            prompts
                .iter()
                .any(|p| p.user.contains("`lambda`: `lambda` is a keyword in python")),
            "the reask carries the validator's words"
        );
        assert!(
            prompts
                .iter()
                .all(|p| !p.user.contains("`print`: ") || p.user.contains("`lambda`")),
            "only one reask"
        );
    }

    #[tokio::test]
    async fn collisions_and_bad_conventions_are_rejected() {
        let idx = fixture_index("py_app");
        // `price` and `name` are fields of the same class: a method may not take them; CamelCase breaks snake_case.
        let llm = Smart::new(&["doStuff2"], |_, nth| {
            Some(["price", "applyDiscount", "apply_discount"][nth.min(3) - 1].to_owned())
        });
        let r = run(&idx, &llm, &ProposeOptions::default()).await;
        assert_eq!(
            find(&r, "doStuff2").status,
            ProposalStatus::NeedsManual,
            "the reask gets only one more try"
        );
        let prompts = llm.prompts.lock().unwrap();
        assert!(
            prompts
                .iter()
                .any(|p| p.user.contains("already exists in this scope"))
        );
    }

    #[tokio::test]
    async fn rejected_names_are_not_proposed_again() {
        let idx = fixture_index("py_app");
        let sym = idx.symbols.iter().find(|s| s.name == "doStuff2").unwrap().id;
        let mut opts = ProposeOptions {
            try_again: true,
            ..ProposeOptions::default()
        };
        opts.rejected.insert(sym, vec!["apply_discount".into()]);
        let llm = Smart::new(&["doStuff2"], |_, nth| {
            Some(if nth == 1 { "apply_discount" } else { "scale_price" }.to_owned())
        });
        let r = run(&idx, &llm, &opts).await;
        assert_eq!(find(&r, "doStuff2").new_name.as_deref(), Some("scale_price"));
        let prompts = llm.prompts.lock().unwrap();
        assert!(
            prompts
                .iter()
                .any(|p| p.user.contains("Do not propose these again: s1→apply_discount"))
        );
    }

    #[tokio::test]
    async fn files_and_folders_are_proposed_too_and_unchanged_is_kept() {
        let idx = fixture_index("go_app");
        let llm = Smart::new(&["util_funcs", "doStuff2"], |old, _| {
            Some(if old == "util_funcs" {
                "money".to_owned()
            } else {
                old.to_owned()
            })
        });
        let r = run(&idx, &llm, &ProposeOptions::default()).await;
        // Go packages are lowercase without separators: the rule fixes it without the model.
        let f = find(&r, "util_funcs");
        assert_eq!(
            (f.new_name.as_deref(), f.source),
            (Some("utilfuncs"), ProposalSource::Rule)
        );
        assert_eq!(find(&r, "doStuff2").status, ProposalStatus::Unchanged);
    }
}
