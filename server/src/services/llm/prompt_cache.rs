//! What the provider's prompt cache did for each conversation: how much of
//! every request's input was read back from the cache (a hit), how much was
//! written to it for the next request, and how much was billed at the full
//! rate, together with whether the system prompt stayed byte-identical from
//! one turn to the next.
//!
//! Providers cache the request as a prefix. A system prompt that changes
//! mid-conversation throws the cached prefix away: on Anthropic the system
//! blocks and every message after them, on OpenAI-style providers
//! everything after the first changed byte, on Codex the thread's
//! configuration. The per-turn check here names the section that changed so
//! the log says what broke the cache.
//!
//! Kept in memory per chat since the server started; nothing is persisted.

use std::collections::{HashMap, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::{LazyLock, Mutex};

use serde::Serialize;

use super::contract::Usage;

/// How many of a chat's latest requests the readout keeps.
const RECENT_REQUESTS: usize = 24;

/// One request's input as the provider accounted for it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct CacheReading {
    /// Every input token, read from the cache or not.
    pub input_tokens: u64,
    /// Input served from the prompt cache.
    pub cache_read_tokens: u64,
    /// Input written to the prompt cache for a later request to read.
    pub cache_write_tokens: u64,
    pub output_tokens: u64,
}

impl CacheReading {
    fn from_usage(usage: &Usage) -> Self {
        // Every adapter reports `input_tokens` as the whole input; a provider
        // that counted the cache apart from it must not read as over 100%.
        let input_tokens = usage
            .input_tokens
            .max(usage.cache_read_tokens + usage.cache_write_tokens);
        Self {
            input_tokens,
            cache_read_tokens: usage.cache_read_tokens,
            cache_write_tokens: usage.cache_write_tokens,
            output_tokens: usage.output_tokens,
        }
    }

    /// Input neither read from nor written to the cache.
    pub fn uncached_tokens(&self) -> u64 {
        self.input_tokens
            .saturating_sub(self.cache_read_tokens + self.cache_write_tokens)
    }

    /// Share of the input read from the cache, in whole percent.
    pub fn hit_percent(&self) -> u64 {
        if self.input_tokens == 0 {
            0
        } else {
            self.cache_read_tokens * 100 / self.input_tokens
        }
    }

    fn add(&mut self, other: &Self) {
        self.input_tokens += other.input_tokens;
        self.cache_read_tokens += other.cache_read_tokens;
        self.cache_write_tokens += other.cache_write_tokens;
        self.output_tokens += other.output_tokens;
    }
}

/// Whether the system prompt stayed the same from turn to turn.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SystemPromptStability {
    /// Turns this chat sent since the server started.
    pub turns: u64,
    /// Of those, how many sent a system prompt that differed from the turn before.
    pub changes: u64,
    /// The sections that differed the last time it changed.
    pub last_changed_sections: Vec<String>,
}

/// A chat's prompt-cache readout.
#[derive(Debug, Clone, Default, Serialize)]
pub struct PromptCacheStats {
    /// The latest request.
    pub last: Option<CacheReading>,
    /// Summed over every request since the server started.
    pub total: CacheReading,
    pub requests: u64,
    /// Requests that read anything from the cache.
    pub hits: u64,
    /// The latest requests, oldest first.
    pub recent: Vec<CacheReading>,
    pub system_prompt: SystemPromptStability,
}

#[derive(Default)]
struct Ledger {
    total: CacheReading,
    requests: u64,
    hits: u64,
    recent: VecDeque<CacheReading>,
    /// The previous turn's system prompt, section name → fingerprint.
    sections: Option<Vec<(String, u64)>>,
    system_prompt: SystemPromptStability,
}

impl Ledger {
    fn stats(&self) -> PromptCacheStats {
        PromptCacheStats {
            last: self.recent.back().copied(),
            total: self.total,
            requests: self.requests,
            hits: self.hits,
            recent: self.recent.iter().copied().collect(),
            system_prompt: self.system_prompt.clone(),
        }
    }
}

static LEDGERS: LazyLock<Mutex<HashMap<String, Ledger>>> = LazyLock::new(Default::default);

fn key(instance_slug: &str, chat_id: &str) -> String {
    format!("{instance_slug}/{chat_id}")
}

/// Records one provider request of a chat and returns the chat's readout.
pub fn record_request(instance_slug: &str, chat_id: &str, usage: &Usage) -> PromptCacheStats {
    let reading = CacheReading::from_usage(usage);
    let key = key(instance_slug, chat_id);
    log::info!(
        "[cache] {key}: {} — {} of {} input tokens read from cache ({}%), {} written, {} uncached, {} output",
        if reading.cache_read_tokens > 0 {
            "hit"
        } else {
            "miss"
        },
        reading.cache_read_tokens,
        reading.input_tokens,
        reading.hit_percent(),
        reading.cache_write_tokens,
        reading.uncached_tokens(),
        reading.output_tokens,
    );
    let mut ledgers = LEDGERS.lock().unwrap_or_else(|e| e.into_inner());
    let ledger = ledgers.entry(key).or_default();
    ledger.total.add(&reading);
    ledger.requests += 1;
    if reading.cache_read_tokens > 0 {
        ledger.hits += 1;
    }
    if ledger.recent.len() == RECENT_REQUESTS {
        ledger.recent.pop_front();
    }
    ledger.recent.push_back(reading);
    ledger.stats()
}

/// Records the system prompt a chat turn is about to send, section by
/// section, and returns the names of the sections that differ from the
/// previous turn's (empty on the first turn and when nothing changed).
pub fn record_system_prompt(
    instance_slug: &str,
    chat_id: &str,
    sections: &[(&str, &str)],
) -> Vec<String> {
    let fingerprints: Vec<(String, u64)> = sections
        .iter()
        .map(|(name, text)| {
            let mut hasher = DefaultHasher::new();
            text.hash(&mut hasher);
            ((*name).to_owned(), hasher.finish())
        })
        .collect();
    let key = key(instance_slug, chat_id);
    let mut ledgers = LEDGERS.lock().unwrap_or_else(|e| e.into_inner());
    let ledger = ledgers.entry(key.clone()).or_default();
    let changed = match &ledger.sections {
        Some(previous) => changed_sections(previous, &fingerprints),
        None => Vec::new(),
    };
    ledger.system_prompt.turns += 1;
    if !changed.is_empty() {
        ledger.system_prompt.changes += 1;
        ledger.system_prompt.last_changed_sections = changed.clone();
        log::warn!(
            "[cache] {key}: system prompt changed since the last turn ({}); the cached prefix from the system prompt on is invalidated",
            changed.join(", ")
        );
    }
    ledger.sections = Some(fingerprints);
    changed
}

/// Sections whose text differs, plus any that were added or removed, in
/// the order they appear.
fn changed_sections(previous: &[(String, u64)], current: &[(String, u64)]) -> Vec<String> {
    let mut changed: Vec<String> = current
        .iter()
        .filter(|section| !previous.contains(section))
        .map(|(name, _)| name.clone())
        .collect();
    for (name, _) in previous {
        if !current.iter().any(|(current, _)| current == name) {
            changed.push(name.clone());
        }
    }
    changed
}

/// The chat's readout, if it sent anything since the server started.
pub fn stats(instance_slug: &str, chat_id: &str) -> Option<PromptCacheStats> {
    LEDGERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key(instance_slug, chat_id))
        .map(Ledger::stats)
}

/// Drops a chat's readout, when its context is cleared.
pub fn forget(instance_slug: &str, chat_id: &str) {
    LEDGERS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(&key(instance_slug, chat_id));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(input: u64, read: u64, write: u64) -> Usage {
        Usage {
            input_tokens: input,
            output_tokens: 10,
            cache_read_tokens: read,
            cache_write_tokens: write,
            cost: None,
        }
    }

    #[test]
    fn a_request_reads_as_hit_or_miss_with_its_share_of_cached_input() {
        let chat = uuid::Uuid::new_v4().to_string();
        let first = record_request("moon", &chat, &usage(10_000, 0, 9_000));
        let last = first.last.unwrap();
        assert_eq!((first.requests, first.hits), (1, 0));
        assert_eq!(last.hit_percent(), 0);
        assert_eq!(last.uncached_tokens(), 1_000);

        let second = record_request("moon", &chat, &usage(10_500, 9_000, 400));
        let last = second.last.unwrap();
        assert_eq!((second.requests, second.hits), (2, 1));
        assert_eq!(last.hit_percent(), 85);
        assert_eq!(last.uncached_tokens(), 1_100);
        assert_eq!(second.total.input_tokens, 20_500);
        assert_eq!(second.total.cache_read_tokens, 9_000);
        assert_eq!(second.recent.len(), 2);
    }

    #[test]
    fn cache_counted_apart_from_input_never_reads_as_over_everything() {
        let chat = uuid::Uuid::new_v4().to_string();
        let stats = record_request("moon", &chat, &usage(100, 900, 0));
        let last = stats.last.unwrap();
        assert_eq!(last.input_tokens, 900);
        assert_eq!(last.hit_percent(), 100);
    }

    #[test]
    fn only_the_latest_requests_are_kept_and_the_totals_cover_all() {
        let chat = uuid::Uuid::new_v4().to_string();
        for i in 0..(RECENT_REQUESTS as u64 + 5) {
            record_request("moon", &chat, &usage(100 + i, 50, 0));
        }
        let stats = stats("moon", &chat).unwrap();
        assert_eq!(stats.recent.len(), RECENT_REQUESTS);
        assert_eq!(
            stats.recent[0].input_tokens, 105,
            "the oldest ones went first"
        );
        assert_eq!(stats.requests, RECENT_REQUESTS as u64 + 5);
    }

    #[test]
    fn a_system_prompt_change_names_the_sections_that_changed() {
        let chat = uuid::Uuid::new_v4().to_string();
        let turn = |skills: &str, github: Option<&str>| {
            let mut sections = vec![("soul", "you are moon"), ("skills", skills)];
            if let Some(github) = github {
                sections.push(("github", github));
            }
            record_system_prompt("moon", &chat, &sections)
        };

        assert!(
            turn("- poems", None).is_empty(),
            "the first turn has nothing to differ from"
        );
        assert!(turn("- poems", None).is_empty());
        assert_eq!(turn("- poems\n- charts", None), vec!["skills"]);
        assert_eq!(turn("- poems\n- charts", Some("token set")), vec!["github"]);
        assert_eq!(turn("- poems\n- charts", None), vec!["github"]);

        let stability = stats("moon", &chat).unwrap().system_prompt;
        assert_eq!((stability.turns, stability.changes), (5, 3));
        assert_eq!(stability.last_changed_sections, vec!["github"]);
    }

    #[test]
    fn forgetting_a_chat_starts_its_readout_over() {
        let chat = uuid::Uuid::new_v4().to_string();
        record_request("moon", &chat, &usage(100, 0, 0));
        record_system_prompt("moon", &chat, &[("soul", "a")]);
        forget("moon", &chat);
        assert!(stats("moon", &chat).is_none());
        assert!(record_system_prompt("moon", &chat, &[("soul", "b")]).is_empty());
    }
}
