//! Commitment tools (#85): the chat's bounded way to create, edit, snooze,
//! complete, cancel, and list the commitments the companion follows through
//! on. They write through the same store as the API, so the evidence rule
//! (completion needs the user's confirmation or recorded evidence, and a
//! `run_id` must name a real activity record) holds here too. They are chat
//! tools only: the check-in and reflection routines never carry them, so a
//! proactive run can state a commitment but never close one.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::{DateTime, NaiveDate, NaiveDateTime, TimeZone, Utc};
use schemars::JsonSchema;
use serde::Deserialize;
use tokio::sync::broadcast;

use crate::domain::commitment::{
    Commitment, CommitmentStatus, CompletionEvidence, Deadline, Owner, Provenance, WaitCondition,
};
use crate::domain::events::ServerEvent;
use crate::services::commitments::{CommitmentPatch, CommitmentStore, ListFilter, NewCommitment};
use crate::services::proactive::ProactiveLoop;
use crate::services::tool::{Tool, ToolDefinition, ToolDyn};

use super::{ToolExecError, openai_schema};

/// Most records a list answers with.
pub const MAX_LIST: usize = 50;
const DEFAULT_LIST: usize = 20;

/// What every commitment tool shares.
struct Context {
    store: CommitmentStore,
    proactive: ProactiveLoop,
    instance_dir: PathBuf,
    chat_id: String,
}

impl Context {
    fn tz(&self) -> chrono_tz::Tz {
        crate::services::commitment_evaluator::instance_timezone(&self.instance_dir)
    }

    fn when(&self, text: &str) -> Result<i64, ToolExecError> {
        parse_when(text, self.tz()).map_err(ToolExecError)
    }
}

/// The six commitment tools, in registration order.
pub fn commitment_tools(
    workspace_dir: &Path,
    instance_slug: &str,
    chat_id: &str,
    events: broadcast::Sender<ServerEvent>,
) -> Vec<Box<dyn ToolDyn>> {
    let context = Arc::new(Context {
        store: CommitmentStore::new(workspace_dir, instance_slug).with_events(events),
        proactive: ProactiveLoop::new(workspace_dir, instance_slug),
        instance_dir: workspace_dir.join("instances").join(instance_slug),
        chat_id: chat_id.to_owned(),
    });
    vec![
        Box::new(CommitmentCreateTool(context.clone())),
        Box::new(CommitmentUpdateTool(context.clone())),
        Box::new(CommitmentCompleteTool(context.clone())),
        Box::new(CommitmentCancelTool(context.clone())),
        Box::new(CommitmentSnoozeTool(context.clone())),
        Box::new(CommitmentListTool(context)),
    ]
}

/// A moment the model states: RFC 3339 with an offset, or a local date and
/// time (`2026-01-06T09:00`, `2026-01-06 09:00`) or a date alone (the start
/// of that day) in the companion's timezone.
pub fn parse_when(text: &str, tz: chrono_tz::Tz) -> Result<i64, String> {
    let _ = (text, tz);
    todo!("commitment tools (#85, PR B)")
}

/// What the model gets back: enough to keep working, never the whole file.
fn summary(commitment: &Commitment, tz: chrono_tz::Tz) -> serde_json::Value {
    let _ = (commitment, tz);
    todo!("commitment tools (#85, PR B)")
}

// ---------------------------------------------------------------------------
// commitment_create
// ---------------------------------------------------------------------------

pub struct CommitmentCreateTool(Arc<Context>);

#[derive(Deserialize, JsonSchema)]
pub struct CommitmentCreateArgs {
    /// The promise in plain words, as the user would recognise it.
    pub promise: String,
    /// Who promised: "companion" (you promised the user; default) or "user" (they asked you to hold them to it).
    #[serde(default)]
    pub owner: Option<String>,
    /// When it is due: RFC 3339, or a local date and time like 2026-01-06T09:00, or a date alone.
    #[serde(default)]
    pub deadline: Option<String>,
    /// With `deadline`, the end of a window it may be done in.
    #[serde(default)]
    pub deadline_end: Option<String>,
    /// Hold it until this moment (same formats as `deadline`).
    #[serde(default)]
    pub wait_until: Option<String>,
    /// Hold it until a named event is observed, for example machine_connected:<machine_id>.
    #[serde(default)]
    pub wait_for_event: Option<String>,
    /// Hold it until the user answers.
    #[serde(default)]
    pub wait_for_user_reply: bool,
    /// Ids of commitments that must be completed first.
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// When to look at it next, if not the deadline or the end of the wait.
    #[serde(default)]
    pub next_check: Option<String>,
}

impl Tool for CommitmentCreateTool {
    const NAME: &'static str = "commitment_create";
    type Error = ToolExecError;
    type Args = CommitmentCreateArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description: "Record a commitment to follow through on: something you promised the user, or something the user asked you to hold them to. Give it a deadline, a wait (a time, a named event, or the user's reply), or dependencies, and the companion will check on it and reach out when that condition arrives. Only record promises made in the conversation; never invent one.".into(),
            parameters: openai_schema::<CommitmentCreateArgs>(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let _ = args;
        todo!("commitment tools (#85, PR B)")
    }
}

// ---------------------------------------------------------------------------
// commitment_update
// ---------------------------------------------------------------------------

pub struct CommitmentUpdateTool(Arc<Context>);

#[derive(Deserialize, JsonSchema)]
pub struct CommitmentUpdateArgs {
    /// Id of the commitment (from commitment_list or an earlier call).
    pub id: String,
    /// New wording of the promise.
    #[serde(default)]
    pub promise: Option<String>,
    /// "companion" or "user".
    #[serde(default)]
    pub owner: Option<String>,
    /// New deadline (same formats as commitment_create).
    #[serde(default)]
    pub deadline: Option<String>,
    /// With `deadline`, the end of a window.
    #[serde(default)]
    pub deadline_end: Option<String>,
    /// Remove the deadline.
    #[serde(default)]
    pub clear_deadline: bool,
    /// Hold it until this moment.
    #[serde(default)]
    pub wait_until: Option<String>,
    /// Hold it until a named event is observed.
    #[serde(default)]
    pub wait_for_event: Option<String>,
    /// Hold it until the user answers.
    #[serde(default)]
    pub wait_for_user_reply: bool,
    /// Stop waiting (for example, the user just replied).
    #[serde(default)]
    pub clear_wait: bool,
    /// Replace the ids of commitments that must be completed first.
    #[serde(default)]
    pub dependencies: Option<Vec<String>>,
    /// When to look at it next.
    #[serde(default)]
    pub next_check: Option<String>,
    /// Stop looking at it on a schedule (the deadline and waits still count).
    #[serde(default)]
    pub clear_next_check: bool,
}

impl Tool for CommitmentUpdateTool {
    const NAME: &'static str = "commitment_update";
    type Error = ToolExecError;
    type Args = CommitmentUpdateArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description: "Edit an open commitment: reword it, move its deadline, change what it waits on (clear_wait when the user has replied or the wait is over), or change its dependencies. Absent fields are kept. Completed, dismissed, and failed commitments cannot be edited.".into(),
            parameters: openai_schema::<CommitmentUpdateArgs>(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let _ = args;
        todo!("commitment tools (#85, PR B)")
    }
}

// ---------------------------------------------------------------------------
// commitment_complete
// ---------------------------------------------------------------------------

pub struct CommitmentCompleteTool(Arc<Context>);

#[derive(Deserialize, JsonSchema)]
pub struct CommitmentCompleteArgs {
    /// Id of the commitment.
    pub id: String,
    /// The user said in this conversation that it is done.
    #[serde(default)]
    pub confirmed_by_user: bool,
    /// What you observed that shows it is done (a reply, a file, a receipt), if the user did not confirm it.
    #[serde(default)]
    pub summary: Option<String>,
    /// Id of the proactive run whose receipts show the work, if any.
    #[serde(default)]
    pub run_id: Option<String>,
}

impl Tool for CommitmentCompleteTool {
    const NAME: &'static str = "commitment_complete";
    type Error = ToolExecError;
    type Args = CommitmentCompleteArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description: "Mark a commitment completed. Refused without the user's explicit confirmation in this conversation or recorded evidence (a summary of what you observed, or the id of a run whose receipts show the work). Never complete one on a hunch.".into(),
            parameters: openai_schema::<CommitmentCompleteArgs>(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let _ = args;
        todo!("commitment tools (#85, PR B)")
    }
}

// ---------------------------------------------------------------------------
// commitment_cancel
// ---------------------------------------------------------------------------

pub struct CommitmentCancelTool(Arc<Context>);

#[derive(Deserialize, JsonSchema)]
pub struct CommitmentCancelArgs {
    /// Id of the commitment.
    pub id: String,
}

impl Tool for CommitmentCancelTool {
    const NAME: &'static str = "commitment_cancel";
    type Error = ToolExecError;
    type Args = CommitmentCancelArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description: "Dismiss an open commitment the user no longer wants followed through. Only when the user says so.".into(),
            parameters: openai_schema::<CommitmentCancelArgs>(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let _ = args;
        todo!("commitment tools (#85, PR B)")
    }
}

// ---------------------------------------------------------------------------
// commitment_snooze
// ---------------------------------------------------------------------------

pub struct CommitmentSnoozeTool(Arc<Context>);

#[derive(Deserialize, JsonSchema)]
pub struct CommitmentSnoozeArgs {
    /// Id of the commitment.
    pub id: String,
    /// When to bring it up again (same formats as commitment_create); must be in the future.
    pub until: String,
}

impl Tool for CommitmentSnoozeTool {
    const NAME: &'static str = "commitment_snooze";
    type Error = ToolExecError;
    type Args = CommitmentSnoozeArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description: "Put an open commitment off until a later moment the user asked for. It is not brought up before then.".into(),
            parameters: openai_schema::<CommitmentSnoozeArgs>(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let _ = args;
        todo!("commitment tools (#85, PR B)")
    }
}

// ---------------------------------------------------------------------------
// commitment_list
// ---------------------------------------------------------------------------

pub struct CommitmentListTool(Arc<Context>);

#[derive(Deserialize, JsonSchema)]
pub struct CommitmentListArgs {
    /// "open" (default), "closed", or "all".
    #[serde(default)]
    pub status: Option<String>,
    /// Most records to answer with (default 20, at most 50), newest first.
    #[serde(default)]
    pub limit: Option<usize>,
}

impl Tool for CommitmentListTool {
    const NAME: &'static str = "commitment_list";
    type Error = ToolExecError;
    type Args = CommitmentListArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description: "List the commitments being followed through on, newest first, with their ids, status, deadline, and what they wait on. Use it to find the id before editing, snoozing, completing, or cancelling one.".into(),
            parameters: openai_schema::<CommitmentListArgs>(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let _ = args;
        todo!("commitment tools (#85, PR B)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::companion::CANONICAL_SLUG;
    use crate::domain::proactive::{Target, Trigger};
    use crate::services::proactive::Admission;

    /// 2099-01-06 09:00 JST = 2099-01-06 00:00 UTC: always in the future.
    const DUE_UTC: i64 = 4_071_340_800;

    struct Harness {
        _ws: tempfile::TempDir,
        tools: Vec<Box<dyn ToolDyn>>,
        store: CommitmentStore,
        proactive: ProactiveLoop,
    }

    impl Harness {
        fn new() -> Self {
            let ws = tempfile::tempdir().unwrap();
            let instance_dir = ws.path().join("instances").join(CANONICAL_SLUG);
            std::fs::create_dir_all(&instance_dir).unwrap();
            std::fs::write(
                instance_dir.join("project_state.json"),
                r#"{"timezone":"Asia/Tokyo"}"#,
            )
            .unwrap();
            let (events, _) = broadcast::channel(16);
            let tools = commitment_tools(ws.path(), CANONICAL_SLUG, "chat_7", events);
            Self {
                store: CommitmentStore::new(ws.path(), CANONICAL_SLUG),
                proactive: ProactiveLoop::new(ws.path(), CANONICAL_SLUG),
                _ws: ws,
                tools,
            }
        }

        fn tool(&self, name: &str) -> &dyn ToolDyn {
            self.tools
                .iter()
                .find(|tool| tool.name() == name)
                .unwrap_or_else(|| panic!("no tool {name}"))
                .as_ref()
        }

        async fn call(&self, name: &str, args: serde_json::Value) -> serde_json::Value {
            let raw = self
                .tool(name)
                .call(args.to_string())
                .await
                .unwrap_or_else(|error| panic!("{name} failed: {error}"));
            serde_json::from_str(&raw).unwrap()
        }

        async fn refused(&self, name: &str, args: serde_json::Value) -> String {
            match self.tool(name).call(args.to_string()).await {
                Ok(answer) => panic!("{name} accepted {args}: {answer}"),
                Err(error) => error.to_string(),
            }
        }

        fn now(&self) -> i64 {
            Utc::now().timestamp()
        }
    }

    #[tokio::test]
    async fn the_user_can_create_update_snooze_complete_and_cancel_from_chat() {
        let h = Harness::new();
        let created = h
            .call(
                "commitment_create",
                serde_json::json!({
                    "promise": "send the draft",
                    "deadline": "2099-01-06T09:00",
                }),
            )
            .await;
        let id = created["id"].as_str().unwrap().to_owned();
        assert!(id.starts_with("cmt_"));
        assert_eq!(created["status"], "active");
        assert_eq!(created["owner"], "companion");
        assert_eq!(created["deadline"]["at"], "2099-01-06 09:00 JST");
        let record = h.store.get(&id, h.now()).unwrap();
        assert_eq!(record.deadline, Some(Deadline::At { at: DUE_UTC }));
        assert_eq!(record.next_check, Some(DUE_UTC));
        assert_eq!(
            record.provenance,
            Provenance::Chat {
                chat_id: "chat_7".into(),
                message_id: None,
            }
        );

        let held = h
            .call(
                "commitment_create",
                serde_json::json!({
                    "promise": "resume the photo export",
                    "owner": "user",
                    "wait_for_event": "machine_connected:mac",
                    "dependencies": [id],
                }),
            )
            .await;
        let held_id = held["id"].as_str().unwrap().to_owned();
        assert_eq!(held["status"], "blocked");
        assert_eq!(held["owner"], "user");
        assert_eq!(held["waiting_on"]["event"], "machine_connected:mac");
        assert_eq!(held["dependencies"], serde_json::json!([id]));

        let updated = h
            .call(
                "commitment_update",
                serde_json::json!({
                    "id": held_id,
                    "promise": "resume the photo export on the mac",
                    "dependencies": [],
                    "clear_wait": true,
                    "wait_for_user_reply": true,
                }),
            )
            .await;
        assert_eq!(updated["promise"], "resume the photo export on the mac");
        assert_eq!(updated["status"], "waiting");
        assert_eq!(updated["waiting_on"]["kind"], "user_reply");
        let after = h
            .call(
                "commitment_update",
                serde_json::json!({"id": held_id, "clear_wait": true}),
            )
            .await;
        assert_eq!(after["status"], "active");
        assert!(after["waiting_on"].is_null());

        let snoozed = h
            .call(
                "commitment_snooze",
                serde_json::json!({"id": id, "until": "2099-01-05T20:00"}),
            )
            .await;
        assert_eq!(snoozed["snoozed_until"], "2099-01-05 20:00 JST");
        assert_eq!(snoozed["snooze_count"], 1);
        assert_eq!(
            h.store.get(&id, h.now()).unwrap().next_check,
            Some(DUE_UTC - 13 * 3600)
        );

        let listed = h.call("commitment_list", serde_json::json!({})).await;
        let ids: Vec<&str> = listed["commitments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, vec![held_id.as_str(), id.as_str()], "newest first");

        let done = h
            .call(
                "commitment_complete",
                serde_json::json!({"id": id, "confirmed_by_user": true}),
            )
            .await;
        assert_eq!(done["status"], "completed");
        assert_eq!(done["completion"]["confirmed_by_user"], true);
        let cancelled = h
            .call("commitment_cancel", serde_json::json!({"id": held_id}))
            .await;
        assert_eq!(cancelled["status"], "dismissed");
        assert_eq!(
            h.store.get(&held_id, h.now()).unwrap().status,
            CommitmentStatus::Dismissed
        );

        let open = h.call("commitment_list", serde_json::json!({})).await;
        assert_eq!(open["commitments"].as_array().unwrap().len(), 0);
        let closed = h
            .call("commitment_list", serde_json::json!({"status": "closed"}))
            .await;
        assert_eq!(closed["commitments"].as_array().unwrap().len(), 2);
        assert_eq!(
            h.refused(
                "commitment_snooze",
                serde_json::json!({"id": id, "until": "2099-02-01"})
            )
            .await
            .contains("already completed"),
            true
        );
    }

    #[tokio::test]
    async fn completion_from_chat_needs_confirmation_or_evidence_and_a_real_run() {
        let h = Harness::new();
        let created = h
            .call(
                "commitment_create",
                serde_json::json!({"promise": "water the plants"}),
            )
            .await;
        let id = created["id"].as_str().unwrap().to_owned();

        let refused = h
            .refused("commitment_complete", serde_json::json!({"id": id}))
            .await;
        assert!(refused.contains("confirmation"), "{refused}");
        let refused = h
            .refused(
                "commitment_complete",
                serde_json::json!({"id": id, "summary": "   "}),
            )
            .await;
        assert!(refused.contains("confirmation"), "{refused}");
        let refused = h
            .refused(
                "commitment_complete",
                serde_json::json!({"id": id, "run_id": "run_does_not_exist"}),
            )
            .await;
        assert!(refused.contains("unknown run"), "{refused}");
        let untouched = h.store.get(&id, h.now()).unwrap();
        assert_eq!(untouched.status, CommitmentStatus::Active);
        assert_eq!(untouched.completion, None);

        let run = match h.proactive.begin_at(
            Trigger::Manual {
                agent: "companion".into(),
            },
            "test",
            Target::Companion,
            h.now(),
        ) {
            Admission::Admitted(handle) => handle,
            Admission::Skipped(_) => panic!("admitted"),
        };
        let done = h
            .call(
                "commitment_complete",
                serde_json::json!({"id": id, "run_id": run.id()}),
            )
            .await;
        assert_eq!(done["status"], "completed");
        assert_eq!(done["completion"]["run_id"], run.id());

        let observed = h
            .call(
                "commitment_create",
                serde_json::json!({"promise": "send the photos"}),
            )
            .await;
        let done = h
            .call(
                "commitment_complete",
                serde_json::json!({"id": observed["id"], "summary": "the reply with the photos arrived"}),
            )
            .await;
        assert_eq!(
            done["completion"]["summary"],
            "the reply with the photos arrived"
        );
    }

    #[tokio::test]
    async fn times_are_parsed_in_the_companions_timezone_and_bad_input_writes_nothing() {
        let tokyo = chrono_tz::Asia::Tokyo;
        assert_eq!(parse_when("2099-01-06T09:00", tokyo), Ok(DUE_UTC));
        assert_eq!(parse_when("2099-01-06 09:00", tokyo), Ok(DUE_UTC));
        assert_eq!(parse_when("2099-01-06T09:00:00", tokyo), Ok(DUE_UTC));
        assert_eq!(
            parse_when("2099-01-06T00:00:00Z", tokyo),
            Ok(DUE_UTC),
            "an offset wins over the timezone"
        );
        assert_eq!(parse_when("2099-01-06T02:00:00+02:00", tokyo), Ok(DUE_UTC));
        assert_eq!(
            parse_when("2099-01-06", tokyo),
            Ok(DUE_UTC - 9 * 3600),
            "a date alone is the start of that day"
        );
        assert_eq!(parse_when(" 2099-01-06T09:00 ", tokyo), Ok(DUE_UTC));
        for bad in [
            "",
            "tomorrow",
            "2099-13-01T09:00",
            "in 3 days",
            "1767603600",
        ] {
            assert!(parse_when(bad, tokyo).is_err(), "{bad:?} must be refused");
        }

        let h = Harness::new();
        for (name, args) in [
            (
                "commitment_create",
                serde_json::json!({"promise": "x", "deadline": "next week"}),
            ),
            (
                "commitment_create",
                serde_json::json!({"promise": "x", "owner": "boss"}),
            ),
            (
                "commitment_create",
                serde_json::json!({"promise": "x", "deadline_end": "2099-01-06"}),
            ),
            (
                "commitment_create",
                serde_json::json!({"promise": "x", "wait_until": "2099-01-06", "wait_for_user_reply": true}),
            ),
            ("commitment_create", serde_json::json!({"promise": "   "})),
            (
                "commitment_create",
                serde_json::json!({"promise": "x", "dependencies": ["cmt_missing"]}),
            ),
            (
                "commitment_update",
                serde_json::json!({"id": "cmt_missing", "promise": "y"}),
            ),
            (
                "commitment_snooze",
                serde_json::json!({"id": "cmt_missing", "until": "2099-01-06"}),
            ),
            ("commitment_cancel", serde_json::json!({"id": "../soul"})),
        ] {
            h.refused(name, args).await;
        }
        assert!(
            h.store.list(ListFilter::All, h.now()).is_empty(),
            "nothing is written on a refusal"
        );

        let created = h
            .call(
                "commitment_create",
                serde_json::json!({"promise": "ship it", "deadline": "2099-01-06", "deadline_end": "2099-01-07"}),
            )
            .await;
        assert_eq!(created["deadline"]["kind"], "window");
        let id = created["id"].as_str().unwrap();
        let refused = h
            .refused(
                "commitment_snooze",
                serde_json::json!({"id": id, "until": "2020-01-01T09:00"}),
            )
            .await;
        assert!(refused.contains("future"), "{refused}");
    }

    #[tokio::test]
    async fn the_definitions_are_bounded_and_the_list_is_capped() {
        let h = Harness::new();
        let names: Vec<String> = h.tools.iter().map(|tool| tool.name()).collect();
        assert_eq!(
            names,
            [
                "commitment_create",
                "commitment_update",
                "commitment_complete",
                "commitment_cancel",
                "commitment_snooze",
                "commitment_list",
            ]
        );
        let complete = h
            .tool("commitment_complete")
            .definition(String::new())
            .await;
        assert!(complete.description.contains("confirmation"));
        assert!(complete.description.contains("evidence"));
        let create = h.tool("commitment_create").definition(String::new()).await;
        assert!(create.description.contains("never invent"));
        let properties = create.parameters["properties"].as_object().unwrap();
        for required in [
            "promise",
            "owner",
            "deadline",
            "deadline_end",
            "wait_until",
            "wait_for_event",
            "wait_for_user_reply",
            "dependencies",
            "next_check",
        ] {
            assert!(properties.contains_key(required), "{required}");
        }

        for index in 0..(MAX_LIST + 5) {
            h.store
                .create(
                    NewCommitment {
                        promise: format!("promise {index}"),
                        ..Default::default()
                    },
                    index as i64,
                )
                .unwrap();
        }
        let capped = h
            .call("commitment_list", serde_json::json!({"limit": 1000}))
            .await;
        assert_eq!(capped["commitments"].as_array().unwrap().len(), MAX_LIST);
        assert_eq!(capped["total"], MAX_LIST + 5);
        let default = h.call("commitment_list", serde_json::json!({})).await;
        assert_eq!(
            default["commitments"].as_array().unwrap().len(),
            DEFAULT_LIST
        );
        assert_eq!(
            default["commitments"][0]["promise"],
            format!("promise {}", MAX_LIST + 4),
            "newest first"
        );
        h.refused("commitment_list", serde_json::json!({"status": "sideways"}))
            .await;
    }
}
