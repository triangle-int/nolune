//! Task handoffs to a paired companion (#111): the pure planner that turns
//! one of this owner's continuity records (#81) into the bounded
//! [`TaskHandoff`] the wire carries. What travels is the record's id as
//! provenance, its goal, the most recent completed steps, the next step,
//! the blockers, a reference to each linked resource (its kind and the
//! label the record itself shows: an upload id, a memory path, a path on
//! a computer), and the record's own provenance entries (the creating one
//! and the most recent ones). Every list is cut to the wire's bound and
//! every label to its length, and the result is validated before it
//! leaves, so the outbox never queues a handoff the peer would refuse.
//!
//! Nothing here opens a file, a memory, or an upload: a reference is the
//! record's label for it, never what it points at, and the peer's server
//! has no way to follow one. The outbox calls this with the record the
//! chat tool read; nothing else reaches a record from the federation side.

use crate::domain::{
    continuity::{ContinuityRecord, ProvenanceSource, ResourceRef},
    federation_intent::{
        HandoffProvenance, HandoffResource, HandoffResourceKind, HandoffSource, IntentError,
        MAX_HANDOFF_BLOCKERS, MAX_HANDOFF_PROVENANCE, MAX_HANDOFF_RESOURCE_CHARS,
        MAX_HANDOFF_RESOURCES, MAX_HANDOFF_STEPS, TaskHandoff,
    },
    federation_policy::PeerText,
};

/// The bounded handoff for `record`: the most recent
/// [`MAX_HANDOFF_STEPS`] steps and [`MAX_HANDOFF_BLOCKERS`] blockers, the
/// first [`MAX_HANDOFF_RESOURCES`] resources with labels cut to
/// [`MAX_HANDOFF_RESOURCE_CHARS`], and the creating provenance entry plus
/// the most recent ones up to [`MAX_HANDOFF_PROVENANCE`]. Fails as the
/// wire would when the record's texts do not fit a handoff (its goal is
/// bounded like a handoff's, so a stored record always does).
pub fn handoff_from_record(record: &ContinuityRecord) -> Result<TaskHandoff, IntentError> {
    let text = |field: &'static str, value: &str| {
        PeerText::new(value.to_owned()).map_err(|_| IntentError::InvalidHandoff { field })
    };
    let completed_steps = last(&record.completed_steps, MAX_HANDOFF_STEPS)
        .map(|step| text("completed_steps", &step.summary))
        .collect::<Result<Vec<_>, _>>()?;
    let blockers = last(&record.blockers, MAX_HANDOFF_BLOCKERS)
        .map(|blocker| text("blockers", &blocker.detail))
        .collect::<Result<Vec<_>, _>>()?;
    let resources = record
        .resources
        .iter()
        .take(MAX_HANDOFF_RESOURCES)
        .map(|link| {
            let label: String = link
                .resource
                .describe()
                .chars()
                .take(MAX_HANDOFF_RESOURCE_CHARS)
                .collect();
            Ok(HandoffResource {
                kind: match link.resource {
                    ResourceRef::Upload { .. } => HandoffResourceKind::Upload,
                    ResourceRef::Memory { .. } => HandoffResourceKind::Memory,
                    ResourceRef::MachinePath { .. } => HandoffResourceKind::MachinePath,
                },
                label: text("resources", &label)?,
            })
        })
        .collect::<Result<Vec<_>, IntentError>>()?;
    let provenance = record
        .provenance
        .first()
        .into_iter()
        .chain(last(
            record.provenance.get(1..).unwrap_or_default(),
            MAX_HANDOFF_PROVENANCE.saturating_sub(1),
        ))
        .map(|entry| {
            Ok(HandoffProvenance {
                source: match entry.source {
                    ProvenanceSource::User => HandoffSource::User,
                    ProvenanceSource::Chat => HandoffSource::Chat,
                    ProvenanceSource::Tool => HandoffSource::Tool,
                    ProvenanceSource::Server => HandoffSource::Server,
                },
                at: u64::try_from(entry.at).unwrap_or(0),
                note: text("provenance", &entry.note)?,
            })
        })
        .collect::<Result<Vec<_>, IntentError>>()?;
    let task = TaskHandoff {
        record_id: record.id.clone(),
        goal: text("goal", &record.goal)?,
        completed_steps,
        next_step: record
            .next_step
            .as_deref()
            .map(|step| text("next_step", step))
            .transpose()?,
        blockers,
        resources,
        provenance,
    };
    task.validate()?;
    Ok(task)
}

/// The last `count` items of `items`, in order.
fn last<T>(items: &[T], count: usize) -> impl Iterator<Item = &T> {
    items[items.len().saturating_sub(count)..].iter()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::continuity::{ContinuityState, ContinuityUpdate, Origin, Provenance};

    const T0: i64 = 1_800_000_000;

    fn provenance(source: ProvenanceSource, at: i64, note: &str) -> Provenance {
        Provenance {
            source,
            at,
            note: note.into(),
        }
    }

    fn record() -> ContinuityRecord {
        let mut record = ContinuityRecord::new(
            "task_1_abc".into(),
            "Print the zine",
            Origin {
                chat_id: "default".into(),
                message_id: None,
            },
            provenance(ProvenanceSource::Chat, T0, "Started from the plan"),
            T0,
        )
        .unwrap();
        record
            .apply(
                &ContinuityUpdate {
                    next_step: Some("Send the draft to the printer".into()),
                    blocker: Some("Waiting on the cover art".into()),
                    resources: vec![
                        ResourceRef::Memory {
                            path: "plan.md".into(),
                        },
                        ResourceRef::MachinePath {
                            machine_id: "studio-mac".into(),
                            path: "/Users/bob/zine/draft.pdf".into(),
                        },
                        ResourceRef::Upload {
                            id: "8f0d2c3e".into(),
                        },
                    ],
                    ..ContinuityUpdate::default()
                },
                provenance(ProvenanceSource::Tool, T0 + 1, "Linked the draft"),
                T0 + 1,
            )
            .unwrap();
        for step in 0..(MAX_HANDOFF_STEPS + 3) {
            record
                .apply(
                    &ContinuityUpdate {
                        completed_step: Some(format!("Step {step} done")),
                        ..ContinuityUpdate::default()
                    },
                    provenance(
                        ProvenanceSource::Tool,
                        T0 + 10 + step as i64,
                        &format!("Recorded step {step}"),
                    ),
                    T0 + 10 + step as i64,
                )
                .unwrap();
        }
        record
    }

    #[test]
    fn the_handoff_carries_the_most_recent_steps_the_references_and_the_provenance() {
        let record = record();
        let task = handoff_from_record(&record).unwrap();
        assert_eq!(task.validate(), Ok(()));
        assert_eq!(task.record_id, "task_1_abc");
        assert_eq!(
            task.completed_steps.len(),
            MAX_HANDOFF_STEPS,
            "the most recent steps"
        );
        let wire = serde_json::to_string(&task).unwrap();
        assert!(
            wire.contains("Step 10 done") && !wire.contains("Step 0 done"),
            "{wire}"
        );
        assert!(wire.contains("Send the draft") && wire.contains("cover art"));
        assert_eq!(
            task.resources.iter().map(|r| r.kind).collect::<Vec<_>>(),
            [
                HandoffResourceKind::Memory,
                HandoffResourceKind::MachinePath,
                HandoffResourceKind::Upload
            ]
        );
        assert!(wire.contains("memory plan.md") && wire.contains("draft.pdf on studio-mac"));
        // The creating entry first, then the most recent ones, bounded.
        assert_eq!(task.provenance.len(), MAX_HANDOFF_PROVENANCE);
        assert_eq!(task.provenance[0].source, HandoffSource::Chat);
        assert_eq!(task.provenance[0].at, T0 as u64);
        assert!(wire.contains("Started from the plan"));
        assert!(wire.contains("Recorded step 10") && !wire.contains("Recorded step 1\""));
        assert_eq!(task.provenance.last().unwrap().source, HandoffSource::Tool);
        // No field of the wire shape could hold what a resource contains.
        assert!(!wire.contains("contents") && !wire.contains("bytes"));
    }

    #[test]
    fn a_bare_record_and_an_over_long_path_still_fit_the_wire() {
        let mut bare = ContinuityRecord::new(
            "task_2".into(),
            "Nothing yet",
            Origin {
                chat_id: "default".into(),
                message_id: None,
            },
            provenance(ProvenanceSource::User, T0, "Started"),
            T0,
        )
        .unwrap();
        let task = handoff_from_record(&bare).unwrap();
        assert!(task.completed_steps.is_empty() && task.next_step.is_none());
        assert_eq!(task.provenance.len(), 1);
        bare.apply(
            &ContinuityUpdate {
                resources: vec![ResourceRef::MachinePath {
                    machine_id: "studio-mac".into(),
                    path: format!("/{}", "x".repeat(400)),
                }],
                state: Some(ContinuityState::Waiting),
                ..ContinuityUpdate::default()
            },
            provenance(ProvenanceSource::User, T0 + 1, "Linked"),
            T0 + 1,
        )
        .unwrap();
        let task = handoff_from_record(&bare).unwrap();
        assert_eq!(task.resources[0].label.chars(), MAX_HANDOFF_RESOURCE_CHARS);
        assert_eq!(task.validate(), Ok(()));
    }
}
