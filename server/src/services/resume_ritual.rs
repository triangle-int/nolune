//! The Resume my work ritual (#83): when the user asks, comes back after a
//! break, or reconnects a computer that waiting work names, offer at most
//! one way to pick unfinished work up again.
//!
//! The ritual is opt-in and backed only by persisted continuity records and
//! the known-machine list: it never looks at a screen or at application
//! activity. Its policy (enabled, break, cooldown, snooze, dismissed
//! records) and its bounded state (when Nolune was last opened, when it
//! last suggested, the current suggestion) live in one file beside the
//! proactive policy, written atomically under one lock, so quiet hours,
//! cooldown, snooze, dismiss, and a refusal hold across restarts and
//! across triggers that land together. A suggestion is delivered as the
//! record's handoff card; nothing continues, and no computer is touched,
//! until the user accepts that card.

use std::{
    collections::HashMap,
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, OnceLock},
};

use serde::{Deserialize, Serialize};

use crate::{
    app::state::AppState,
    domain::{
        companion::CANONICAL_SLUG,
        continuity::HandoffDecision,
        events::ServerEvent,
        handoff::HandoffCard,
        machine::KnownMachine,
        resume::{
            MAX_DISMISSED, RESUME_FORMAT_VERSION, RankContext, ResumeRitualPolicy,
            ResumeSuggestion, RitualTrigger, rank,
        },
    },
    services::handoff,
};

/// The ritual's file under the companion directory, beside `proactive_policy.json`.
pub const RITUAL_FILE: &str = "resume_ritual.json";

/// What the ritual remembers between triggers. Bounded: a few moments and
/// the one current suggestion.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RitualState {
    /// When the client last reported Nolune being opened or brought back.
    pub last_opened_at: Option<i64>,
    /// When the last suggestion was made; starts the cooldown.
    pub last_suggested_at: Option<i64>,
    /// When the user last said "not now"; also starts the cooldown.
    pub last_refused_at: Option<i64>,
    /// The suggestion on offer, until it is refused, snoozed, dismissed,
    /// or its card is decided.
    pub suggestion: Option<ResumeSuggestion>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct RitualFile {
    version: u32,
    policy: ResumeRitualPolicy,
    state: RitualState,
}

impl Default for RitualFile {
    fn default() -> Self {
        Self {
            version: RESUME_FORMAT_VERSION,
            policy: ResumeRitualPolicy::default(),
            state: RitualState::default(),
        }
    }
}

/// The part of the policy the settings page edits. Snooze and dismissals
/// have routes of their own, so a stale copy of the page can never undo them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyEdit {
    pub enabled: bool,
    pub break_minutes: u32,
    pub cooldown_secs: u64,
}

/// Why a trigger produced no suggestion.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Held {
    Disabled,
    QuietHours,
    Cooldown {
        until: i64,
    },
    Snoozed {
        until: i64,
    },
    /// Opened again before the configured break had passed.
    NoBreak,
    /// No valid resumable record.
    NothingToResume,
    Storage {
        message: String,
    },
}

impl std::fmt::Display for Held {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Disabled => f.write_str("the resume ritual is off"),
            Self::QuietHours => f.write_str("quiet hours are active"),
            Self::Cooldown { .. } => f.write_str("a suggestion was made or refused recently"),
            Self::Snoozed { .. } => f.write_str("the resume ritual is snoozed"),
            Self::NoBreak => f.write_str("no break long enough has passed"),
            Self::NothingToResume => f.write_str("nothing to resume right now"),
            Self::Storage { message } => f.write_str(message),
        }
    }
}

/// What a client shows: the suggestion and the card it leads to.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResumeOffer {
    #[serde(flatten)]
    pub suggestion: ResumeSuggestion,
    pub card: HandoffCard,
}

/// The ritual as the API reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResumeStatus {
    pub policy: ResumeRitualPolicy,
    pub suggestion: Option<ResumeOffer>,
    /// The proactive policy's quiet hours are active right now.
    pub quiet_hours_active: bool,
}

/// The one lock for a ritual file, whichever handle asks for it, so the
/// routes and the connect hook never save a copy the other has replaced.
fn file_lock(path: &Path) -> Arc<tokio::sync::Mutex<()>> {
    static LOCKS: OnceLock<std::sync::Mutex<HashMap<PathBuf, Arc<tokio::sync::Mutex<()>>>>> =
        OnceLock::new();
    let mut locks = LOCKS
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    Arc::clone(locks.entry(path.to_path_buf()).or_default())
}

/// The persisted policy and state, read on every call and written atomically.
#[derive(Clone)]
pub struct ResumeRitual {
    workspace_dir: PathBuf,
    slug: String,
    lock: Arc<tokio::sync::Mutex<()>>,
}

impl ResumeRitual {
    pub fn new(workspace_dir: &Path, slug: &str) -> Self {
        let path = workspace_dir.join("instances").join(slug).join(RITUAL_FILE);
        Self {
            workspace_dir: workspace_dir.to_path_buf(),
            slug: slug.to_owned(),
            lock: file_lock(&path),
        }
    }

    pub fn path(&self) -> PathBuf {
        self.workspace_dir
            .join("instances")
            .join(&self.slug)
            .join(RITUAL_FILE)
    }

    fn read(&self) -> RitualFile {
        fs::read_to_string(self.path())
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default()
    }

    fn write(&self, file: &RitualFile) -> io::Result<()> {
        let path = self.path();
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        write_atomic(
            &path,
            &serde_json::to_string_pretty(file).map_err(io::Error::other)?,
        )
    }

    /// Read, change, and write the file under the lock.
    async fn modify<T>(&self, change: impl FnOnce(&mut RitualFile) -> T) -> io::Result<T> {
        let _guard = self.lock.lock().await;
        self.modify_locked(change)
    }

    /// Read, change, and write the file; the caller holds the lock.
    fn modify_locked<T>(&self, change: impl FnOnce(&mut RitualFile) -> T) -> io::Result<T> {
        let mut file = self.read();
        let out = change(&mut file);
        self.write(&file)?;
        Ok(out)
    }

    pub fn policy(&self) -> ResumeRitualPolicy {
        self.read().policy
    }

    /// The bounded state on disk; production reads it with the policy in
    /// one read, so this is for tests.
    #[cfg(test)]
    pub fn state(&self) -> RitualState {
        self.read().state
    }

    /// Apply the settings page's edit. Turning the ritual off drops the
    /// current suggestion; returns whether one was dropped.
    pub async fn set_policy(&self, edit: &PolicyEdit) -> io::Result<(ResumeRitualPolicy, bool)> {
        self.modify(|file| {
            file.policy.enabled = edit.enabled;
            file.policy.break_minutes = edit.break_minutes;
            file.policy.cooldown_secs = edit.cooldown_secs;
            let dropped = !edit.enabled && file.state.suggestion.take().is_some();
            (file.policy.clone(), dropped)
        })
        .await
    }

    /// Snooze until `until`, dropping the current suggestion, or clear the
    /// snooze with `None`. Returns the policy and whether a suggestion was dropped.
    pub async fn snooze(&self, until: Option<i64>) -> io::Result<(ResumeRitualPolicy, bool)> {
        self.modify(|file| {
            file.policy.snooze_until = until;
            let dropped = until.is_some() && file.state.suggestion.take().is_some();
            (file.policy.clone(), dropped)
        })
        .await
    }

    /// Never suggest `record_id` again; drops the current suggestion when
    /// it is that record. Returns the policy and whether one was dropped.
    pub async fn dismiss(&self, record_id: &str) -> io::Result<(ResumeRitualPolicy, bool)> {
        self.modify(|file| {
            let ids = &mut file.policy.dismissed_record_ids;
            if !ids.iter().any(|id| id == record_id) {
                ids.push(record_id.to_owned());
            }
            if ids.len() > MAX_DISMISSED {
                let excess = ids.len() - MAX_DISMISSED;
                ids.drain(..excess);
            }
            let dropped = file
                .state
                .suggestion
                .as_ref()
                .is_some_and(|suggestion| suggestion.record_id == record_id)
                && file.state.suggestion.take().is_some();
            (file.policy.clone(), dropped)
        })
        .await
    }

    /// "Not now": drop the current suggestion and start the cooldown.
    /// Returns whether there was one.
    pub async fn refuse(&self, now: i64) -> io::Result<bool> {
        self.modify(|file| {
            let had = file.state.suggestion.take().is_some();
            if had {
                file.state.last_refused_at = Some(now);
            }
            had
        })
        .await
    }

    /// Drop the suggestion with this id (its card was decided or is gone).
    pub async fn resolve(&self, suggestion_id: &str) -> io::Result<bool> {
        self.modify(|file| {
            let matches = file
                .state
                .suggestion
                .as_ref()
                .is_some_and(|suggestion| suggestion.id == suggestion_id);
            matches && file.state.suggestion.take().is_some()
        })
        .await
    }

    /// Note that Nolune was opened or brought back at `now`; answers how
    /// long it was away, or `None` the first time.
    pub async fn record_opened(&self, now: i64) -> io::Result<Option<i64>> {
        self.modify(|file| {
            let away = file.state.last_opened_at.map(|last| now - last);
            file.state.last_opened_at = Some(now);
            away
        })
        .await
    }

    /// Persist a new suggestion; starts the cooldown. `invoke` places its
    /// offer while already holding the lock, so this is for tests.
    #[cfg(test)]
    pub async fn offer(&self, suggestion: ResumeSuggestion) -> io::Result<()> {
        self.modify(|file| place(file, suggestion)).await
    }
}

/// Make `suggestion` the current offer and start the cooldown from it.
fn place(file: &mut RitualFile, suggestion: ResumeSuggestion) {
    file.state.last_suggested_at = Some(suggestion.suggested_at);
    file.state.suggestion = Some(suggestion);
}

/// Whether `trigger` may look for work now. Spontaneous triggers wait for
/// the ritual to be on, quiet hours to end, a snooze to end, and the
/// cooldown since the last suggestion or refusal to pass; the manual one
/// only needs the ritual on.
pub fn admission(
    policy: &ResumeRitualPolicy,
    state: &RitualState,
    trigger: &RitualTrigger,
    quiet_hours: bool,
    now: i64,
) -> Result<(), Held> {
    if !policy.enabled {
        return Err(Held::Disabled);
    }
    if !trigger.is_spontaneous() {
        return Ok(());
    }
    if quiet_hours {
        return Err(Held::QuietHours);
    }
    if let Some(until) = policy.snooze_until
        && policy.is_snoozed(now)
    {
        return Err(Held::Snoozed { until });
    }
    let since = state.last_suggested_at.max(state.last_refused_at);
    if let Some(until) = since
        .map(|since| since + policy.cooldown_secs as i64)
        .filter(|until| now < *until)
    {
        return Err(Held::Cooldown { until });
    }
    Ok(())
}

fn ritual(state: &AppState) -> ResumeRitual {
    ResumeRitual::new(&state.workspace_dir, CANONICAL_SLUG)
}

async fn machines(state: &AppState, now: i64) -> Vec<KnownMachine> {
    match state.machine_registry.known_at(now).await {
        Ok(machines) => machines,
        Err(error) => {
            log::warn!("[resume] machine list unavailable: {error}");
            Vec::new()
        }
    }
}

fn broadcast(state: &AppState, offer: Option<&ResumeOffer>) {
    let _ = state.events.send(ServerEvent::ResumeUpdated {
        instance_slug: CANONICAL_SLUG.to_owned(),
        suggestion: offer.cloned(),
    });
}

fn storage(error: io::Error) -> Held {
    Held::Storage {
        message: error.to_string(),
    }
}

/// The ritual as the API reports it: the policy and the current offer.
pub async fn status(state: &AppState, now: i64) -> ResumeStatus {
    ResumeStatus {
        policy: ritual(state).policy(),
        suggestion: current(state, now).await,
        quiet_hours_active: state.proactive.quiet_hours_now(now),
    }
}

/// The current suggestion with its card, or nothing while the ritual is
/// off. A suggestion whose record is gone, no longer offered, or accepted
/// is resolved here.
pub async fn current(state: &AppState, now: i64) -> Option<ResumeOffer> {
    let ritual = ritual(state);
    let file = ritual.read();
    if !file.policy.enabled {
        return None;
    }
    let suggestion = file.state.suggestion?;
    let card = match handoff::card(state, &suggestion.record_id, now).await {
        Ok(card) if !answered(&card, &suggestion) => card,
        Ok(_) | Err(handoff::HandoffError::NotFound) => {
            if let Err(error) = ritual.resolve(&suggestion.id).await {
                log::warn!(
                    "[resume] could not resolve suggestion {}: {error}",
                    suggestion.id
                );
            }
            return None;
        }
        Err(error) => {
            log::warn!(
                "[resume] card for {} unavailable: {error}",
                suggestion.record_id
            );
            return None;
        }
    };
    Some(ResumeOffer { suggestion, card })
}

/// Whether the card answers the suggestion: it is no longer offered, its
/// continuation is running, or it was accepted since the suggestion was
/// made. The same line `rank` draws: a running continuation is never
/// suggested, and a finished one leaves the card open again.
fn answered(card: &HandoffCard, suggestion: &ResumeSuggestion) -> bool {
    !card.offered
        || card
            .decision
            .as_ref()
            .is_some_and(|decision| match decision {
                HandoffDecision::Accepted { at, .. } => {
                    decision.is_continuing() || *at >= suggestion.suggested_at
                }
                HandoffDecision::Kept { .. } | HandoffDecision::Dismissed { .. } => true,
            })
}

/// Look for work on behalf of `trigger`: admit it, rank the offered
/// records against the known machines, persist and broadcast the pick as
/// the current suggestion. Triggers are taken one at a time under the
/// ritual's lock, so two that land together (the client's open report and
/// a desktop registering, say) share one cooldown check, and an answer
/// given while one is ranking is applied after its offer, never lost.
pub async fn invoke(
    state: &AppState,
    trigger: RitualTrigger,
    now: i64,
) -> Result<ResumeOffer, Held> {
    let ritual = ritual(state);
    let _looking = ritual.lock.lock().await;
    let file = ritual.read();
    admission(
        &file.policy,
        &file.state,
        &trigger,
        state.proactive.quiet_hours_now(now),
        now,
    )?;

    let records = handoff::offered_records(state, now).await;
    let machines = machines(state, now).await;
    let only_naming = match &trigger {
        RitualTrigger::MachineConnected { machine_id } => Some(machine_id.as_str()),
        RitualTrigger::Manual | RitualTrigger::OpenedAfterBreak { .. } => None,
    };
    let candidate = rank(
        &records,
        &RankContext {
            now,
            machines: &machines,
            dismissed: &file.policy.dismissed_record_ids,
            only_naming,
        },
    )
    .ok_or(Held::NothingToResume)?;

    let card = handoff::card(state, &candidate.record_id, now)
        .await
        .map_err(|error| Held::Storage {
            message: error.to_string(),
        })?;
    let suggestion = ResumeSuggestion {
        id: new_suggestion_id(now),
        record_id: candidate.record_id,
        goal: candidate.goal,
        why_now: trigger.why_now(&machines),
        trigger,
        why_this: candidate.why_this,
        destination_id: candidate.destination_id,
        suggested_at: now,
    };
    ritual
        .modify_locked(|file| place(file, suggestion.clone()))
        .map_err(storage)?;
    let offer = ResumeOffer { suggestion, card };
    broadcast(state, Some(&offer));
    Ok(offer)
}

/// The client reported Nolune being opened or brought back: note the
/// moment and, when the configured break has passed, look for work.
pub async fn opened(state: &AppState, now: i64) -> Result<ResumeOffer, Held> {
    let ritual = ritual(state);
    let policy = ritual.policy();
    if !policy.enabled {
        return Err(Held::Disabled);
    }
    let away = ritual.record_opened(now).await.map_err(storage)?;
    match away {
        Some(away_secs) if away_secs >= i64::from(policy.break_minutes) * 60 => {
            invoke(state, RitualTrigger::OpenedAfterBreak { away_secs }, now).await
        }
        _ => Err(Held::NoBreak),
    }
}

/// A computer connected: offer waiting work that names it, if any. Called
/// beside the connect check-in; nothing here touches the computer.
pub async fn on_machine_connected(state: &AppState, machine_id: &str) {
    let now = chrono::Utc::now().timestamp();
    match invoke(
        state,
        RitualTrigger::MachineConnected {
            machine_id: machine_id.to_owned(),
        },
        now,
    )
    .await
    {
        Ok(offer) => log::info!(
            "[resume] '{machine_id}' connected: offered {}",
            offer.suggestion.record_id
        ),
        Err(Held::Disabled | Held::NothingToResume) => {}
        Err(held) => log::info!("[resume] '{machine_id}' connected: held ({held})"),
    }
}

pub async fn refuse(state: &AppState, now: i64) -> io::Result<()> {
    if ritual(state).refuse(now).await? {
        broadcast(state, None);
    }
    Ok(())
}

pub async fn snooze(state: &AppState, until: Option<i64>) -> io::Result<ResumeRitualPolicy> {
    let (policy, dropped) = ritual(state).snooze(until).await?;
    if dropped {
        broadcast(state, None);
    }
    Ok(policy)
}

pub async fn dismiss(state: &AppState, record_id: &str) -> io::Result<ResumeRitualPolicy> {
    let (policy, dropped) = ritual(state).dismiss(record_id).await?;
    if dropped {
        broadcast(state, None);
    }
    Ok(policy)
}

pub async fn set_policy(state: &AppState, edit: &PolicyEdit) -> io::Result<ResumeRitualPolicy> {
    let (policy, dropped) = ritual(state).set_policy(edit).await?;
    if dropped {
        broadcast(state, None);
    }
    Ok(policy)
}

fn new_suggestion_id(now: i64) -> String {
    let suffix: String = uuid::Uuid::new_v4().simple().to_string()[..8].to_owned();
    format!("sug_{now}_{suffix}")
}

fn write_atomic(path: &Path, content: &str) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, content)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-01-05 (Monday) 09:00:00 UTC
    const T0: i64 = 1_767_603_600;

    fn harness() -> (tempfile::TempDir, ResumeRitual) {
        let ws = tempfile::tempdir().unwrap();
        fs::create_dir_all(ws.path().join("instances").join(CANONICAL_SLUG)).unwrap();
        let ritual = ResumeRitual::new(ws.path(), CANONICAL_SLUG);
        (ws, ritual)
    }

    fn suggestion(record_id: &str, at: i64) -> ResumeSuggestion {
        ResumeSuggestion {
            id: format!("sug_{at}_0badcafe"),
            record_id: record_id.into(),
            goal: "rename the trip photos".into(),
            trigger: RitualTrigger::Manual,
            why_now: "You asked to resume your work.".into(),
            why_this: "the most recent task, studio can take it".into(),
            destination_id: "studio-id".into(),
            suggested_at: at,
        }
    }

    fn edit(enabled: bool) -> PolicyEdit {
        PolicyEdit {
            enabled,
            break_minutes: 45,
            cooldown_secs: 900,
        }
    }

    #[tokio::test]
    async fn policy_snooze_and_dismissals_persist_beside_the_proactive_policy_across_a_fresh_instance()
     {
        let (ws, ritual) = harness();
        assert_eq!(ritual.policy(), ResumeRitualPolicy::default());
        assert!(!ritual.path().exists(), "reading never writes");

        ritual.set_policy(&edit(true)).await.unwrap();
        ritual.snooze(Some(T0 + 3_600)).await.unwrap();
        ritual.dismiss("task_1_aaaaaaaa").await.unwrap();
        ritual.dismiss("task_1_aaaaaaaa").await.unwrap();
        ritual.dismiss("task_2_bbbbbbbb").await.unwrap();

        let dir = ws.path().join("instances").join(CANONICAL_SLUG);
        assert_eq!(ritual.path(), dir.join(RITUAL_FILE));
        assert!(dir.join(RITUAL_FILE).is_file());
        assert!(
            !dir.join("resume_ritual.tmp").exists(),
            "written atomically"
        );

        let fresh = ResumeRitual::new(ws.path(), CANONICAL_SLUG);
        let policy = fresh.policy();
        assert!(policy.enabled);
        assert_eq!(policy.break_minutes, 45);
        assert_eq!(policy.cooldown_secs, 900);
        assert_eq!(policy.snooze_until, Some(T0 + 3_600));
        assert_eq!(
            policy.dismissed_record_ids,
            vec!["task_1_aaaaaaaa", "task_2_bbbbbbbb"],
            "dismissals are kept once each"
        );

        fresh.snooze(None).await.unwrap();
        assert_eq!(
            ResumeRitual::new(ws.path(), CANONICAL_SLUG)
                .policy()
                .snooze_until,
            None
        );

        // The dismissed list is bounded: the oldest makes room.
        for index in 0..MAX_DISMISSED + 5 {
            fresh
                .dismiss(&format!("task_{index}_cccccccc"))
                .await
                .unwrap();
        }
        let ids = fresh.policy().dismissed_record_ids;
        assert_eq!(ids.len(), MAX_DISMISSED);
        assert!(!ids.iter().any(|id| id == "task_1_aaaaaaaa"));
        assert_eq!(
            ids.last().unwrap(),
            &format!("task_{}_cccccccc", MAX_DISMISSED + 4)
        );
    }

    #[tokio::test]
    async fn a_stale_settings_edit_cannot_undo_a_snooze_or_a_dismissal() {
        let (_ws, ritual) = harness();
        ritual.snooze(Some(T0 + 600)).await.unwrap();
        ritual.dismiss("task_1_aaaaaaaa").await.unwrap();
        ritual.set_policy(&edit(true)).await.unwrap();
        let policy = ritual.policy();
        assert_eq!(policy.snooze_until, Some(T0 + 600));
        assert_eq!(policy.dismissed_record_ids, vec!["task_1_aaaaaaaa"]);
        assert!(policy.enabled);
    }

    #[tokio::test]
    async fn the_current_suggestion_is_dropped_by_refusal_snooze_dismissal_or_turning_off() {
        let (ws, ritual) = harness();
        ritual.set_policy(&edit(true)).await.unwrap();

        ritual
            .offer(suggestion("task_1_aaaaaaaa", T0))
            .await
            .unwrap();
        let state = ResumeRitual::new(ws.path(), CANONICAL_SLUG).state();
        assert_eq!(
            state.suggestion.as_ref().map(|s| s.record_id.as_str()),
            Some("task_1_aaaaaaaa")
        );
        assert_eq!(state.last_suggested_at, Some(T0));

        assert!(ritual.refuse(T0 + 10).await.unwrap());
        assert!(
            !ritual.refuse(T0 + 11).await.unwrap(),
            "nothing left to refuse"
        );
        let state = ritual.state();
        assert_eq!(state.suggestion, None);
        assert_eq!(state.last_refused_at, Some(T0 + 10));

        ritual
            .offer(suggestion("task_1_aaaaaaaa", T0 + 20))
            .await
            .unwrap();
        let (_, dropped) = ritual.snooze(Some(T0 + 4_000)).await.unwrap();
        assert!(dropped);
        assert_eq!(ritual.state().suggestion, None);

        ritual
            .offer(suggestion("task_1_aaaaaaaa", T0 + 30))
            .await
            .unwrap();
        let (_, dropped) = ritual.dismiss("task_9_zzzzzzzz").await.unwrap();
        assert!(!dropped, "dismissing another record leaves the offer");
        let (_, dropped) = ritual.dismiss("task_1_aaaaaaaa").await.unwrap();
        assert!(dropped);
        assert_eq!(ritual.state().suggestion, None);

        ritual
            .offer(suggestion("task_1_aaaaaaaa", T0 + 40))
            .await
            .unwrap();
        assert!(!ritual.resolve("sug_other").await.unwrap());
        assert!(
            ritual
                .resolve(&format!("sug_{}_0badcafe", T0 + 40))
                .await
                .unwrap()
        );
        assert_eq!(ritual.state().suggestion, None);

        ritual
            .offer(suggestion("task_1_aaaaaaaa", T0 + 50))
            .await
            .unwrap();
        let (policy, dropped) = ritual.set_policy(&edit(false)).await.unwrap();
        assert!(dropped && !policy.enabled);
        assert_eq!(ritual.state().suggestion, None);
    }

    #[tokio::test]
    async fn opening_measures_the_break_since_the_last_open() {
        let (ws, ritual) = harness();
        assert_eq!(
            ritual.record_opened(T0).await.unwrap(),
            None,
            "no earlier open to measure from"
        );
        let fresh = ResumeRitual::new(ws.path(), CANONICAL_SLUG);
        assert_eq!(fresh.record_opened(T0 + 3_600).await.unwrap(), Some(3_600));
        assert_eq!(fresh.state().last_opened_at, Some(T0 + 3_600));
    }

    #[test]
    fn admission_holds_spontaneous_triggers_but_not_the_manual_one() {
        let on = ResumeRitualPolicy {
            enabled: true,
            cooldown_secs: 600,
            ..Default::default()
        };
        let idle = RitualState::default();
        let opened = RitualTrigger::OpenedAfterBreak { away_secs: 9_000 };
        let connected = RitualTrigger::MachineConnected {
            machine_id: "studio-id".into(),
        };

        // Off: nothing, not even on request.
        let off = ResumeRitualPolicy::default();
        for trigger in [&RitualTrigger::Manual, &opened, &connected] {
            assert_eq!(
                admission(&off, &idle, trigger, false, T0),
                Err(Held::Disabled)
            );
        }

        // On and idle: everything may look.
        for trigger in [&RitualTrigger::Manual, &opened, &connected] {
            assert_eq!(admission(&on, &idle, trigger, false, T0), Ok(()));
        }

        // Quiet hours hold the spontaneous triggers.
        assert_eq!(
            admission(&on, &idle, &opened, true, T0),
            Err(Held::QuietHours)
        );
        assert_eq!(
            admission(&on, &idle, &connected, true, T0),
            Err(Held::QuietHours)
        );
        assert_eq!(
            admission(&on, &idle, &RitualTrigger::Manual, true, T0),
            Ok(())
        );

        // A snooze holds them until it ends.
        let snoozed = ResumeRitualPolicy {
            snooze_until: Some(T0 + 100),
            ..on.clone()
        };
        assert_eq!(
            admission(&snoozed, &idle, &opened, false, T0),
            Err(Held::Snoozed { until: T0 + 100 })
        );
        assert_eq!(admission(&snoozed, &idle, &opened, false, T0 + 100), Ok(()));
        assert_eq!(
            admission(&snoozed, &idle, &RitualTrigger::Manual, false, T0),
            Ok(())
        );

        // The cooldown runs from the last suggestion or the last refusal, whichever is later.
        let suggested = RitualState {
            last_suggested_at: Some(T0 - 300),
            ..Default::default()
        };
        assert_eq!(
            admission(&on, &suggested, &connected, false, T0),
            Err(Held::Cooldown { until: T0 + 300 })
        );
        assert_eq!(
            admission(&on, &suggested, &connected, false, T0 + 300),
            Ok(())
        );
        let refused = RitualState {
            last_suggested_at: Some(T0 - 3_000),
            last_refused_at: Some(T0 - 100),
            ..Default::default()
        };
        assert_eq!(
            admission(&on, &refused, &opened, false, T0),
            Err(Held::Cooldown { until: T0 + 500 })
        );
        assert_eq!(
            admission(&on, &refused, &RitualTrigger::Manual, false, T0),
            Ok(())
        );
    }
}
