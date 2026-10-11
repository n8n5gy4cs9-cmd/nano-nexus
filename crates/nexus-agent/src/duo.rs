//! Duo (v0.1): one assistant, two models. Easy questions go to the light model, hard ones to the
//! strong model. The choice is a cheap, deterministic heuristic (no extra model call), so it is
//! predictable and testable. The user picks which installed model is which in Settings.

/// Which model answers a turn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tier {
    Light,
    Strong,
}

/// What the heuristic looks at.
#[derive(Clone, Copy, Debug, Default)]
pub struct DuoSignals<'a> {
    pub text: &'a str,
    /// Tokens of attached files/folders (0 when none).
    pub attachment_tokens: u32,
    /// The user turned on "Think harder".
    pub think: bool,
}

/// Words that usually mean the question needs real reasoning or long output.
const HARD_WORDS: &[&str] = &[
    "debug",
    "refactor",
    "architecture",
    "design",
    "optimi",
    "prove",
    "algorithm",
    "analy",
    "compare",
    "trade-off",
    "tradeoff",
    "step by step",
    "why does",
    "root cause",
    "implement",
    "migrate",
    "review",
    "security",
    "regex",
    "sql",
    "concurren",
    "race condition",
    "complexity",
    "explain in depth",
];

const LONG_TEXT_CHARS: usize = 600;
const BIG_ATTACHMENT_TOKENS: u32 = 1500;

/// Pick a tier. Strong when the user asked to think harder, attached a lot, wrote a long
/// question, included code, or used a "hard" keyword; otherwise light.
pub fn classify(s: &DuoSignals<'_>) -> Tier {
    if s.think || s.attachment_tokens >= BIG_ATTACHMENT_TOKENS || s.text.chars().count() >= LONG_TEXT_CHARS {
        return Tier::Strong;
    }
    if s.text.contains("```") {
        return Tier::Strong;
    }
    let lower = s.text.to_lowercase();
    if HARD_WORDS.iter().any(|w| lower.contains(w)) {
        return Tier::Strong;
    }
    Tier::Light
}

/// The model id for a tier. An unset (empty) choice falls back to `fallback` (the active model),
/// and a light model that is unset falls back to the strong one, so Duo always works.
pub fn pick_model<'a>(tier: Tier, light: &'a str, strong: &'a str, fallback: &'a str) -> &'a str {
    let non_empty = |s: &'a str| (!s.trim().is_empty()).then_some(s);
    match tier {
        Tier::Light => non_empty(light).or(non_empty(strong)).unwrap_or(fallback),
        Tier::Strong => non_empty(strong).or(non_empty(light)).unwrap_or(fallback),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(text: &str) -> Tier {
        classify(&DuoSignals {
            text,
            ..Default::default()
        })
    }

    #[test]
    fn short_chat_is_light() {
        assert_eq!(t("What's the capital of France?"), Tier::Light);
        assert_eq!(t("Translate 'hello' to Spanish"), Tier::Light);
    }

    #[test]
    fn hard_signals_are_strong() {
        assert_eq!(t("Help me debug this crash"), Tier::Strong);
        assert_eq!(t("```rust\nfn main() {}\n```"), Tier::Strong);
        assert_eq!(t(&"word ".repeat(200)), Tier::Strong);
        let think = DuoSignals {
            text: "hi",
            think: true,
            ..Default::default()
        };
        assert_eq!(classify(&think), Tier::Strong);
        let big = DuoSignals {
            text: "summarize",
            attachment_tokens: 4000,
            ..Default::default()
        };
        assert_eq!(classify(&big), Tier::Strong);
    }

    #[test]
    fn model_fallbacks() {
        assert_eq!(pick_model(Tier::Light, "small", "big", "active"), "small");
        assert_eq!(pick_model(Tier::Strong, "small", "big", "active"), "big");
        assert_eq!(pick_model(Tier::Light, "", "big", "active"), "big");
        assert_eq!(pick_model(Tier::Strong, "small", " ", "active"), "small");
        assert_eq!(pick_model(Tier::Light, "", "", "active"), "active");
    }
}
