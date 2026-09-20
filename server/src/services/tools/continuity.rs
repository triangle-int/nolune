//! `task_continuity_update` (#81): the one chat tool that writes continuity
//! records. It records explicit progress on work the user asked for, in the
//! conversation where that work happens. It is never handed to the check-in
//! or reflection routines and never reacts to screenshots or any other
//! passive observation.

use std::path::Path;

use schemars::JsonSchema;
use serde::Deserialize;

use crate::domain::continuity::{
    ContinuityRecord, ContinuityState, ContinuityUpdate, Origin, Provenance, ProvenanceSource,
    ResourceRef,
};
use crate::services::continuity::ContinuityStore;
use crate::services::tool::{Tool, ToolDefinition};

use super::{ToolExecError, openai_schema};

pub struct TaskContinuityUpdateTool {
    store: ContinuityStore,
    chat_id: String,
}

impl TaskContinuityUpdateTool {
    pub fn new(workspace_dir: &Path, instance_slug: &str, chat_id: &str) -> Self {
        Self {
            store: ContinuityStore::new(workspace_dir, instance_slug),
            chat_id: chat_id.to_owned(),
        }
    }
}

/// A file or folder on one of the user's computers.
#[derive(Deserialize, JsonSchema)]
pub struct MachinePathArg {
    pub machine_id: String,
    pub path: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct TaskContinuityUpdateArgs {
    /// Id of the record to update (from an earlier call). Omit to start a record for a new task.
    #[serde(default)]
    pub id: Option<String>,
    /// The user's goal in their own words. Required when starting a record.
    #[serde(default)]
    pub goal: Option<String>,
    /// New state: active, waiting, ready_to_resume, completed, dismissed, or failed.
    #[serde(default)]
    pub state: Option<String>,
    /// One step that was just finished.
    #[serde(default)]
    pub completed_step: Option<String>,
    /// Something that stops progress and needs the user (a missing file, a decision, a login).
    #[serde(default)]
    pub blocker: Option<String>,
    /// Set when the stated blockers are resolved.
    #[serde(default)]
    pub clear_blockers: bool,
    /// The next suggested step, or an empty string to clear it.
    #[serde(default)]
    pub next_step: Option<String>,
    /// Computers involved (ids from list_machines).
    #[serde(default)]
    pub machine_ids: Vec<String>,
    /// Uploaded files involved (upload ids), kept as links.
    #[serde(default)]
    pub upload_ids: Vec<String>,
    /// Memory library paths involved, kept as links.
    #[serde(default)]
    pub memory_paths: Vec<String>,
    /// Paths on a computer involved, kept as links.
    #[serde(default)]
    pub machine_paths: Vec<MachinePathArg>,
    /// Why this update is happening (recorded as provenance the user can read).
    pub note: String,
}

impl TaskContinuityUpdateArgs {
    fn update(&self) -> Result<ContinuityUpdate, ToolExecError> {
        let _ = self;
        todo!("#81 continuity tool")
    }
}

/// What the model gets back: enough to keep working, never the whole file.
fn summary(record: &ContinuityRecord) -> serde_json::Value {
    let _ = record;
    todo!("#81 continuity tool")
}

impl Tool for TaskContinuityUpdateTool {
    const NAME: &'static str = "task_continuity_update";
    type Error = ToolExecError;
    type Args = TaskContinuityUpdateArgs;
    type Output = serde_json::Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition {
            name: Self::NAME.into(),
            description: "Record explicit progress on a task the user asked you to do, so it can be resumed later in another chat or on another device: goal, state, finished steps, blockers, next step, and the computers, uploads, and memory paths involved (as links). Call it when such a task starts, finishes a step, gets blocked, changes state, or completes. Only record work the user asked for; never record things you merely observed.".into(),
            parameters: openai_schema::<TaskContinuityUpdateArgs>(),
        }
    }

    async fn call(&self, args: Self::Args) -> Result<Self::Output, Self::Error> {
        let _ = (&self.store, &self.chat_id, args);
        let _ = (
            ContinuityState::Active,
            ProvenanceSource::Tool,
            Origin {
                chat_id: String::new(),
                message_id: None,
            },
        );
        let _: Option<Provenance> = None;
        let _: Option<ResourceRef> = None;
        todo!("#81 continuity tool")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::companion::CANONICAL_SLUG;
    use crate::domain::continuity::BlockerKind;
    use crate::services::tool::ToolDyn;

    fn harness() -> (tempfile::TempDir, TaskContinuityUpdateTool, ContinuityStore) {
        let ws = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(ws.path().join("instances").join(CANONICAL_SLUG)).unwrap();
        let tool = TaskContinuityUpdateTool::new(ws.path(), CANONICAL_SLUG, "chat_7");
        let store = ContinuityStore::new(ws.path(), CANONICAL_SLUG);
        (ws, tool, store)
    }

    async fn call(tool: &TaskContinuityUpdateTool, args: serde_json::Value) -> serde_json::Value {
        let raw = ToolDyn::call(tool, args.to_string()).await.unwrap();
        serde_json::from_str(&raw).unwrap()
    }

    #[tokio::test]
    async fn starting_and_updating_a_task_records_explicit_progress_with_provenance() {
        let (_ws, tool, store) = harness();
        let started = call(
            &tool,
            serde_json::json!({
                "goal": "rename the trip photos",
                "machine_ids": ["mac-mini"],
                "upload_ids": ["upload_1"],
                "memory_paths": ["notes/trip.md"],
                "machine_paths": [{"machine_id": "mac-mini", "path": "/Volumes/Trip"}],
                "next_step": "list the folder",
                "note": "user asked for this in chat",
            }),
        )
        .await;
        let id = started["id"].as_str().unwrap().to_owned();
        assert_eq!(started["state"], "active");
        assert_eq!(started["goal"], "rename the trip photos");
        assert_eq!(started["next_step"], "list the folder");

        let record = store.get(&id).unwrap();
        assert_eq!(record.origin.chat_id, "chat_7");
        assert_eq!(record.machine_ids, vec!["mac-mini"]);
        assert_eq!(record.resources.len(), 3);
        assert_eq!(
            record.resources[0].resource,
            ResourceRef::Upload {
                id: "upload_1".into()
            }
        );
        assert_eq!(
            record.resources[2].resource,
            ResourceRef::MachinePath {
                machine_id: "mac-mini".into(),
                path: "/Volumes/Trip".into()
            }
        );
        assert!(
            record
                .provenance
                .iter()
                .all(|p| p.source == ProvenanceSource::Tool)
        );
        assert_eq!(record.provenance[0].note, "user asked for this in chat");

        let updated = call(
            &tool,
            serde_json::json!({
                "id": id,
                "state": "waiting",
                "completed_step": "listed 212 files",
                "blocker": "the external drive is not mounted",
                "next_step": "rename IMG_* to trip-*",
                "note": "drive disappeared mid-task",
            }),
        )
        .await;
        assert_eq!(updated["id"], id);
        assert_eq!(updated["state"], "waiting");
        assert_eq!(updated["completed_steps"], 1);
        assert_eq!(
            updated["blockers"],
            serde_json::json!(["the external drive is not mounted"])
        );
        let record = store.get(&id).unwrap();
        assert_eq!(record.state, ContinuityState::Waiting);
        assert_eq!(record.completed_steps[0].summary, "listed 212 files");
        assert_eq!(record.blockers[0].kind, BlockerKind::Other);
        assert_eq!(record.provenance.len(), 2);
        assert_eq!(record.provenance[1].note, "drive disappeared mid-task");

        let done = call(
            &tool,
            serde_json::json!({"id": id, "state": "completed", "clear_blockers": true, "note": "all renamed"}),
        )
        .await;
        assert_eq!(done["state"], "completed");
        assert_eq!(done["blockers"], serde_json::json!([]));
        assert!(store.resumable().is_empty());
    }

    #[tokio::test]
    async fn the_tool_refuses_writes_without_a_goal_note_or_known_record() {
        let (_ws, tool, store) = harness();
        for args in [
            serde_json::json!({"note": "no goal"}),
            serde_json::json!({"goal": "x", "note": ""}),
            serde_json::json!({"goal": "x", "note": "   "}),
            serde_json::json!({"id": "task_missing", "state": "waiting", "note": "n"}),
            serde_json::json!({"goal": "x", "state": "sideways", "note": "n"}),
            serde_json::json!({"goal": "x", "upload_ids": ["../etc"], "note": "n"}),
        ] {
            let result = ToolDyn::call(&tool, args.to_string()).await;
            assert!(result.is_err(), "{args}");
        }
        assert!(store.list().is_empty(), "nothing is written on failure");
        assert!(store.list_errors().is_empty());
    }

    #[tokio::test]
    async fn the_definition_names_explicit_task_work_only() {
        let (_ws, tool, _) = harness();
        let definition = ToolDyn::definition(&tool, String::new()).await;
        assert_eq!(definition.name, "task_continuity_update");
        assert!(definition.description.contains("the user asked"));
        assert!(definition.description.contains("never record"));
        let properties = definition.parameters["properties"].as_object().unwrap();
        for required in [
            "id",
            "goal",
            "state",
            "completed_step",
            "blocker",
            "next_step",
            "machine_ids",
            "upload_ids",
            "memory_paths",
            "machine_paths",
            "note",
        ] {
            assert!(properties.contains_key(required), "{required}");
        }
        assert!(
            !properties.contains_key("screenshot") && !properties.contains_key("observation"),
            "no passive capture input"
        );
    }
}
