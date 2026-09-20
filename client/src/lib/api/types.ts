export type ChatRole = "user" | "assistant";

export type MessageKind = "message" | "tool_call" | "tool_output" | "mcp_app" | "compaction";

export interface ChatMessage {
	id: string;
	role: ChatRole;
	content: string;
	created_at: string;
	kind?: MessageKind;
	tool_name?: string;
	mcp_app_html?: string;
	mcp_app_input?: string;
	model?: string;
}

export interface ChatRequest {
	instance_slug: string;
	content: string;
	chat_id?: string;
}

export interface ChatResponse {
	instance_slug: string;
	chat_id: string;
	messages: ChatMessage[];
	agent_running: boolean;
}

export interface ChatSummary {
	id: string;
	title: string;
	message_count: number;
	last_message_at: string | null;
	created_at: string;
}

/** The one companion this server owns; `exists` is false until onboarding created it. */
export interface CompanionContext {
	slug: string;
	exists: boolean;
	companion_name: string;
	soul_exists: boolean;
}

export interface LlmSummary {
	model: string | null;
	configured: boolean;
}

export interface ServerMeta {
	app: string;
	version: string;
	commit: string;
	port: number;
	workspace_dir: string;
	/** Stable slug of the one companion this server owns. */
	companion_slug: string;
	instances_count: number;
	skills_count: number;
	llm: LlmSummary;
}

export interface UpdateLlmRequest {
	api_key: string;
}

export interface Soul {
	content: string;
	exists: boolean;
}

export interface SoulTemplate {
	id: string;
	name: string;
	description: string;
	content: string;
}

export type DropKind =
	| "thought"
	| "idea"
	| "poem"
	| "observation"
	| "reflection"
	| "recommendation"
	| "story"
	| "question"
	| "note";

export interface Drop {
	id: string;
	kind: DropKind;
	title: string;
	content: string;
	mood: string;
	created_at: string;
	image_url?: string;
}

/** One proactive run (#92/#94): receipts only, never model text. */
export type Tagged = { kind: string; [key: string]: unknown };
export interface ActionReceipt { tool: string; summary: string }
export interface RunOutcome { actions: ActionReceipt[]; messages_sent: number; tokens: number }
export interface Approval { side_effect: string; allowed: boolean; reason?: string; at: number }
export interface ProactiveRun {
	version: number;
	id: string;
	trigger: Tagged;
	reason: string;
	target: Tagged;
	dedupe_key: string;
	status: Tagged;
	attempt: number;
	retry_of?: string;
	started_at: number;
	finished_at?: number;
	approvals: Approval[];
	outcome?: RunOutcome | null;
}
export interface QuietHours { start_hour: number; end_hour: number }

/** A resumable task record (#81): links and provenance, never file contents. */
export type ContinuityState = "active" | "waiting" | "ready_to_resume" | "completed" | "dismissed" | "failed";
export type ProvenanceSource = "user" | "chat" | "tool" | "server";
export interface Provenance { source: ProvenanceSource; at: number; note: string }
export type ResourceRef =
	| { kind: "upload"; id: string }
	| { kind: "memory"; path: string }
	| { kind: "machine_path"; machine_id: string; path: string };
export interface ResourceLink { resource: ResourceRef; provenance: Provenance }
export interface ContinuityStep { summary: string; provenance: Provenance }
export type BlockerKind =
	| { kind: "machine_unavailable"; machine_id: string }
	| { kind: "resource_missing"; resource: ResourceRef }
	| { kind: "other" };
export interface ContinuityBlocker { kind: BlockerKind; detail: string; provenance: Provenance }
export interface ContinuityRecord {
	version: number;
	id: string;
	goal: string;
	state: ContinuityState;
	origin: { chat_id: string; message_id?: string };
	machine_ids: string[];
	resources: ResourceLink[];
	completed_steps: ContinuityStep[];
	blockers: ContinuityBlocker[];
	next_step?: string;
	created_at: number;
	updated_at: number;
	provenance: Provenance[];
}
/** Files under continuity/ that could not be read; surfaced, never deleted. */
export interface ContinuityRecordError { file: string; reason: string }
export interface ContinuityListing { records: ContinuityRecord[]; errors: ContinuityRecordError[] }
/** One explicit change; lists are added to, never replaced. `note` is required provenance. */
export interface ContinuityUpdate {
	goal?: string;
	state?: ContinuityState;
	completed_step?: string;
	blocker?: string;
	clear_blockers?: boolean;
	next_step?: string;
	machine_ids?: string[];
	resources?: ResourceRef[];
	note: string;
}
/** A tracked promise (#85): a bounded record the server keeps, never model text. */
export type CommitmentStatus = "active" | "waiting" | "blocked" | "due" | "completed" | "dismissed" | "failed";
export type CommitmentOwner = "companion" | "user";
export type CommitmentDeadline = { kind: "at"; at: number } | { kind: "window"; start: number; end: number };
export type CommitmentWait = { kind: "until"; until: number } | { kind: "event"; event: string } | { kind: "user_reply" };
export type CommitmentProvenance =
	| { kind: "manual" }
	| { kind: "chat"; chat_id: string; message_id?: string }
	| { kind: "run"; run_id: string };
/** Why it counts as done; the server refuses a completion with none of these. */
export interface CompletionEvidence { confirmed_by_user?: boolean; summary?: string; run_id?: string; at?: number }
export type CommitmentCheckOutcome =
	| { kind: "unchanged" }
	| { kind: "triggered" }
	| { kind: "failed"; error: string; retryable: boolean }
	| { kind: "observed"; event: string };
export interface CommitmentCheck { at: number; outcome: CommitmentCheckOutcome; run_id?: string; pending_event?: string }
export interface Commitment {
	version: number;
	id: string;
	promise: string;
	owner: CommitmentOwner;
	status: CommitmentStatus;
	deadline?: CommitmentDeadline | null;
	dependencies: string[];
	waiting_on?: CommitmentWait | null;
	next_check?: number | null;
	continuity_ids: string[];
	provenance: CommitmentProvenance;
	completion?: CompletionEvidence | null;
	snoozed_until?: number | null;
	snooze_count: number;
	last_check?: CommitmentCheck | null;
	created_at: number;
	updated_at: number;
	status_changed_at: number;
}
/** Absent fields are kept; the `clear_*` flags remove optional ones. */
export interface CommitmentPatch {
	promise?: string;
	owner?: CommitmentOwner;
	deadline?: CommitmentDeadline;
	clear_deadline?: boolean;
	dependencies?: string[];
	waiting_on?: CommitmentWait;
	clear_waiting_on?: boolean;
	next_check?: number;
	clear_next_check?: boolean;
	continuity_ids?: string[];
}
export type CommitmentListFilter = "open" | "closed" | "all";
export interface ProactivePolicy {
	enabled: boolean;
	quiet_hours: QuietHours | null;
	cooldown_secs: number;
	daily_reach_out_budget: number;
	retention_max: number;
	retention_days: number;
	check_in_interval_hours: number;
	reflection_enabled: boolean;
	reflection_interval_hours: number;
}


export interface SkillSource {
	repo: string;
	version: string;
}

export interface Skill {
	id: string;
	name: string;
	description: string;
	icon: string;
	builtin: boolean;
	enabled: boolean;
	kind?: "local" | "anthropic";
	anthropic_skill_id?: string;
	instructions: string;
	source?: SkillSource;
	resources?: string[];
}

export interface RegistryEntry {
	id: string;
	name: string;
	description: string;
	icon: string;
	repo: string;
	git_ref: string;
	author: string;
	path: string;
	installed: boolean;
}

export interface UploadMeta {
	id: string;
	original_name: string;
	stored_name: string;
	mime_type: string;
	size: number;
	uploaded_at: string;
}

export interface ContextSection {
	name: string;
	chars: number;
	tokens: number;
}

export interface ContextStats {
	system_prompt: ContextSection[];
	system_prompt_total_tokens: number;
	tools: string[];
	tools_count: number;
	tools_tokens_estimate: number;
	history_messages: number;
	history_tokens_estimate: number;
	total_input_tokens_estimate: number;
}

export interface MemoryEntry {
	path: string;
	summary: string;
	size: number;
}

export interface MemoryGraph {
	edges: [string, string][];
}

/** How auto-recall surfaced a memory (#84). */
export type RecallReason = "semantic" | "keyword" | "linked_to" | "matched";
/** Coarse confidence bucket; raw scores never leave the server. */
export type RecallConfidence = "high" | "medium" | "low";

/** One recalled memory as carried by the `memory_recall` event and persisted receipts. */
export interface RecalledMemory {
	/** Memory path as the library shows it (for media, the media file). */
	path: string;
	/** Canonical file whose text was recalled; media memories cite their bound text. */
	source: string;
	excerpt: string;
	reason: RecallReason;
	/** The recalled memory this one was linked from (`linked_to` only). */
	linked_from?: string;
	confidence: RecallConfidence;
	/** RFC 3339 UTC timestamp of the retrieval. */
	retrieved_at: string;
	/** Resolved on every receipt read; a deleted source is reported, not dropped. */
	source_status: "present" | "missing";
}

export type MachinePlatform = "macos" | "windows" | "linux";
export type MachineLocation = "server_local" | "desktop";
/** `healthy`, `degraded` (open socket, stale heartbeat), or `unavailable` (offline). */
export type MachineHealth = "healthy" | "degraded" | "unavailable";
export type MachinePermission = "granted" | "denied" | "prompt_required" | "unavailable";
export interface MachinePermissions {
	accessibility: MachinePermission;
	screen_capture: MachinePermission;
}

/**
 * A computer that has ever connected to the companion through the Nolune
 * desktop app (#80). Disconnected computers stay listed as offline; a
 * `machine_updated` server event carries the same shape on every change
 * (including a heartbeat that goes stale), and `machine_forgotten` names a
 * row to drop.
 */
export interface MachineInfo {
	/** Stable id the desktop persists; survives reconnects and hostname changes. */
	machine_id: string;
	/** The user's name when set, otherwise the hostname. */
	display_name: string;
	/** The user's name, `null` while the hostname is shown. */
	custom_name: string | null;
	hostname: string;
	os: string;
	platform: MachinePlatform | null;
	location: MachineLocation;
	screen_width: number;
	screen_height: number;
	/** Reported at the last registration; `null` when the desktop did not report it. */
	permissions: MachinePermissions | null;
	/** Action names the desktop accepts. */
	capabilities: string[];
	/** Unix seconds of the first registration. */
	first_seen: number;
	/** Unix seconds of the last heartbeat or disconnect. */
	last_seen: number;
	instance_slug: string | null;
	online: boolean;
	health: MachineHealth;
	/** Reserved for the Cua driver (#18); `null` means not reported. */
	driver_version: string | null;
	/** Reserved for the Cua driver's own health (#18); `null` means not reported. */
	cua_health: MachineHealth | null;
}

export type ServerEvent =
	| {
			type: "chat_message_created";
			instance_slug: string;
			chat_id: string;
			message: ChatMessage;
	  }
	| {
			type: "mood_updated";
			instance_slug: string;
			mood: string;
	  }
	| {
			type: "agent_running";
			instance_slug: string;
			chat_id: string;
	  }
	| {
			type: "agent_stopped";
			instance_slug: string;
			chat_id: string;
	  }
	| {
			type: "tool_activity";
			instance_slug: string;
			chat_id: string;
			tool_name: string;
			summary: string;
	  }
	| {
			type: "drop_created";
			instance_slug: string;
			drop: Drop;
	  }
	| {
			type: "activity_updated";
			instance_slug: string;
			run: ProactiveRun;
	  }
	| {
			/** A commitment was created or changed (#85): the whole record, no model text. */
			type: "commitment_updated";
			instance_slug: string;
			commitment: Commitment;
	  }
	| {
			type: "machine_updated";
			instance_slug: string;
			machine: MachineInfo;
	  }
	| {
			/** An offline computer was forgotten, migrated into its stable id, or evicted: drop its row. */
			type: "machine_forgotten";
			instance_slug: string;
			machine_id: string;
	  }
	| {
			type: "context_compacting";
			instance_slug: string;
			chat_id: string;
			messages_compacted: number;
	  }
	| {
			type: "chat_stream_delta";
			instance_slug: string;
			chat_id: string;
			message_id: string;
			delta: string;
	  }
	| {
			type: "secret_request";
			instance_slug: string;
			id: string;
			prompt: string;
			target: string;
	  }
	| {
			type: "tool_output_chunk";
			instance_slug: string;
			chat_id: string;
			chunk: string;
	  }
	| {
			type: "mcp_app_result";
			instance_slug: string;
			chat_id: string;
			message_id: string;
			tool_output: string;
	  }
	| {
			type: "mcp_app_start";
			instance_slug: string;
			chat_id: string;
			tool_name: string;
			html: string;
	  }
	| {
			type: "chat_audio_ready";
			instance_slug: string;
			chat_id: string;
			audio_base64: string;
			message_ids: string[];
	  }
	| {
			type: "mcp_app_input_delta";
			instance_slug: string;
			chat_id: string;
			delta: string;
	  }
	| {
			type: "memory_recall";
			instance_slug: string;
			chat_id: string;
			memories: RecalledMemory[];
	  }
	| {
			type: "computer_use_request";
			instance_slug: string;
			request_id: string;
			action: string;
			coordinate?: [number, number];
			text?: string;
			key?: string;
			scroll_delta?: [number, number];
	  }
;
